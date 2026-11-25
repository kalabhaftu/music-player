//! Windows OS integration (Wave 5): single-instance guard, system-tray icon
//! with the original menu strings, and global media-key handling.
//!
//! Everything runs on a dedicated native-message thread; commands are handed
//! back to the Slint UI thread through a channel drained by the 250 ms clock.

use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::ptr;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
    ERROR_ALREADY_EXISTS,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryA};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NOTIFYICONDATAW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, HHOOK, HICON, HWND_MESSAGE,
    IDI_APPLICATION, LoadIconW, LoadImageW, IMAGE_ICON, LR_DEFAULTCOLOR, LR_LOADFROMFILE, SM_CXSMICON, SM_CYSMICON,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SendMessageTimeoutW,
    SetForegroundWindow, SetTimer, SetWindowsHookExW, TrackPopupMenu, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, HWND_BROADCAST,
    MF_SEPARATOR, MF_STRING, SMTO_ABORTIFHUNG, TPM_LEFTALIGN, TPM_RETURNCMD,
    WH_KEYBOARD_LL, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSEXW,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    Open,
    Previous,
    Play,
    Pause,
    PlayPause,
    Next,
    Quit,
}

#[derive(Debug, Clone, Default)]
pub struct TrayStatus {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

const VK_MEDIA_NEXT_TRACK: u32 = 0xB0;
const VK_MEDIA_PREV_TRACK: u32 = 0xB1;
const VK_MEDIA_PLAY_PAUSE: u32 = 0xB3;

const WM_TRAY: u32 = WM_APP + 1;
const WM_TRAY_TIMER: usize = 1;
const WM_DISABLE_MEDIA_KEYS: u32 = WM_APP + 3;
const TM_OPEN: usize = 1;
const TM_PREVIOUS: usize = 2;
const TM_PLAY_PAUSE: usize = 3;
const TM_NEXT: usize = 4;
const TM_QUIT: usize = 5;
static ACTIVATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static TRAY: RefCell<TrayState> = RefCell::new(TrayState::default());
}

#[derive(Default)]
struct TrayState {
    hwnd: HWND,
    icon: HICON,
    commands: Option<Sender<MediaCommand>>,
    status_rx: Option<Receiver<TrayStatus>>,
    status: TrayStatus,
    show_message: u32,
    taskbar_created_msg: u32,
    hook: HHOOK,
}

#[derive(Debug, Clone)]
pub struct ActivationPayload {
    pub verb: String,
    pub paths: Vec<String>,
}

pub fn parse_activation_args(args: &[String]) -> (String, Vec<String>) {
    let mut verb = "play".to_string();
    let mut paths = Vec::new();
    for arg in args {
        let lower = arg.to_lowercase();
        if lower == "--add-to-queue" || lower == "addtoqueue" {
            verb = "addtoqueue".to_string();
        } else if lower == "--create-playlist" || lower == "createplaylist" {
            verb = "createplaylist".to_string();
        } else if !arg.starts_with("--") {
            paths.push(arg.clone());
        }
    }
    (verb, paths)
}

pub fn write_activation_handoff(args: &[String]) {
    let (verb, paths) = parse_activation_args(args);
    if paths.is_empty() {
        return;
    }
    let base = directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("MusicPlayer").join("Activations"))
        .unwrap_or_else(|| std::path::PathBuf::from("activations"));
    let _ = std::fs::create_dir_all(&base);
    let id = format!(
        "{}_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        ACTIVATION_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    );
    let temporary_path = base.join(format!("{}.tmp", id));
    let final_path = base.join(format!("{}.json", id));
    let json = serde_json::json!({
        "verb": verb,
        "paths": paths,
    });
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        file.write_all(json.to_string().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary_path, &final_path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(temporary_path);
    }
}

pub fn poll_activations() -> Vec<ActivationPayload> {
    let base = directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("MusicPlayer").join("Activations"))
        .unwrap_or_else(|| std::path::PathBuf::from("activations"));
    if !base.is_dir() {
        return Vec::new();
    }
    let mut collected = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        let verb = val.get("verb").and_then(|v| v.as_str()).unwrap_or("play").to_string();
                        let mut paths = Vec::new();
                        if let Some(arr) = val.get("paths").and_then(|p| p.as_array()) {
                            for item in arr {
                                if let Some(s) = item.as_str() {
                                    paths.push(s.to_string());
                                }
                            }
                        }
                        if !paths.is_empty() {
                            collected.push(ActivationPayload { verb, paths });
                        }
                    }
                }
                let _ = std::fs::remove_file(path);
            }
        }
    }
    collected
}

/// Returns `false` when another instance owns the named mutex. In that case a
/// "show window" broadcast is sent so the running instance is restored, and any
/// command-line files are handed off via `%LOCALAPPDATA%\MusicPlayer\Activations`.
pub fn acquire_single_instance(args: &[String]) -> bool {
    unsafe {
        let name: Vec<u16> = "Local\\MusicPlayer.SingleInstance\0".encode_utf16().collect();
        SetLastError(0);
        let mutex = CreateMutexW(ptr::null(), 1, name.as_ptr());
        if mutex.is_null() {
            // Keep the application usable if Windows cannot allocate the guard.
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if !args.is_empty() {
                write_activation_handoff(args);
            }
            let show_name: Vec<u16> = "MusicPlayer.ShowMainWindow\0".encode_utf16().collect();
            let message = RegisterWindowMessageW(show_name.as_ptr());
            if message != 0 {
                let mut result: usize = 0;
                SendMessageTimeoutW(
                    HWND_BROADCAST,
                    message,
                    0,
                    0,
                    SMTO_ABORTIFHUNG,
                    300,
                    &mut result,
                );
            }
            CloseHandle(mutex);
            return false;
        }
        // The mutex and its handle deliberately live for the process lifetime.
        true
    }
}

pub struct OsIntegration {
    commands: Receiver<MediaCommand>,
    command_tx: Sender<MediaCommand>,
    status_tx: Sender<TrayStatus>,
    tray_hwnd: std::sync::Arc<AtomicIsize>,
    thread: Option<JoinHandle<()>>,
}

impl OsIntegration {
    pub fn start() -> Option<Self> {
        if !cfg!(windows) {
            return None;
        }
        let (command_tx, commands) = mpsc::channel();
        let (status_tx, status_rx) = mpsc::channel();
        let show_name: Vec<u16> = "MusicPlayer.ShowMainWindow\0".encode_utf16().collect();
        let show_message = unsafe { RegisterWindowMessageW(show_name.as_ptr()) };
        let tray_hwnd = std::sync::Arc::new(AtomicIsize::new(0));
        let thread_hwnd = tray_hwnd.clone();
        let thread_commands = command_tx.clone();
        let thread = std::thread::Builder::new()
            .name("music-player-tray".into())
            .spawn(move || unsafe {
                run_tray_thread(thread_commands, status_rx, show_message, thread_hwnd);
            })
            .ok()?;
        Some(Self {
            commands,
            command_tx,
            status_tx,
            tray_hwnd,
            thread: Some(thread),
        })
    }

    pub fn try_command(&self) -> Option<MediaCommand> {
        self.commands.try_recv().ok()
    }

    /// Cloneable sender used by the window-proc/SMTC integrations.
    pub fn command_sender(&self) -> Sender<MediaCommand> {
        self.command_tx.clone()
    }

    /// Drops the low-level keyboard hook once SMTC owns media keys.
    pub fn disable_media_key_hook(&self) {
        let hwnd = self.tray_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_DISABLE_MEDIA_KEYS, 0, 0);
            }
        }
    }

    pub fn set_status(&self, status: TrayStatus) {
        let _ = self.status_tx.send(status);
    }
}

impl Drop for OsIntegration {
    fn drop(&mut self) {
        let hwnd = self.tray_hwnd.load(Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

unsafe fn run_tray_thread(
    commands: Sender<MediaCommand>,
    status_rx: Receiver<TrayStatus>,
    show_message: u32,
    hwnd_slot: std::sync::Arc<AtomicIsize>,
) {
    let class_name: Vec<u16> = "MusicPlayerTrayClass\0".encode_utf16().collect();
    let window_name: Vec<u16> = "Music Player Tray\0".encode_utf16().collect();
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(tray_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: std::ptr::null_mut(),
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    let atom = RegisterClassExW(&class);
    if atom == 0 {
        return;
    }
    let hwnd = CreateWindowExW(
        0,
        atom as *const u16,
        window_name.as_ptr(),
        0,
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null(),
    );
    if hwnd.is_null() {
        return;
    }
    hwnd_slot.store(hwnd as isize, Ordering::SeqCst);

    let hinstance = GetModuleHandleW(std::ptr::null());
    let mut icon = LoadImageW(
        hinstance,
        1 as *const u16,
        IMAGE_ICON,
        GetSystemMetrics(SM_CXSMICON),
        GetSystemMetrics(SM_CYSMICON),
        LR_DEFAULTCOLOR,
    );
    if icon.is_null() {
        icon = LoadIconW(hinstance, 1 as *const u16);
    }
    if icon.is_null() {
        if let Ok(exe_dir) = std::env::current_exe().map(|p| p.parent().unwrap_or(std::path::Path::new("")).to_path_buf()) {
            let candidates = [
                exe_dir.join("assets/MusicPlayer.ico"),
                exe_dir.join("../../crates/app/assets/MusicPlayer.ico"),
                exe_dir.join("../../../packaging/icon.ico"),
                std::path::PathBuf::from("crates/app/assets/MusicPlayer.ico"),
            ];
            for path in candidates {
                if path.is_file() {
                    let mut path_u16: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
                    path_u16.push(0);
                    icon = LoadImageW(
                        std::ptr::null_mut(),
                        path_u16.as_ptr(),
                        IMAGE_ICON,
                        GetSystemMetrics(SM_CXSMICON),
                        GetSystemMetrics(SM_CYSMICON),
                        LR_LOADFROMFILE | LR_DEFAULTCOLOR,
                    );
                    if !icon.is_null() {
                        break;
                    }
                }
            }
        }
    }
    if icon.is_null() {
        icon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
    }
    let taskbar_created_name: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
    let taskbar_created_msg = RegisterWindowMessageW(taskbar_created_name.as_ptr());

    TRAY.with(|cell| {
        let mut state = cell.borrow_mut();
        state.hwnd = hwnd;
        state.icon = icon;
        state.commands = Some(commands);
        state.status_rx = Some(status_rx);
        state.show_message = show_message;
        state.taskbar_created_msg = taskbar_created_msg;
    });

    let data = notify_data(hwnd, icon, "Music Player");
    let added = Shell_NotifyIconW(NIM_ADD, &data);
    if added != 0 {
        SetTimer(hwnd, WM_TRAY_TIMER, 500, None);
    }

    // Global media-key hook. The hook owner must pump messages on this thread.
    TRAY.with(|cell| {
        let mut state = cell.borrow_mut();
        state.hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0);
    });

    let mut message = std::mem::zeroed::<windows_sys::Win32::UI::WindowsAndMessaging::MSG>();
    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    if added != 0 {
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
    TRAY.with(|cell| {
        let mut state = cell.borrow_mut();
        if !state.hook.is_null() {
            UnhookWindowsHookEx(state.hook);
            state.hook = std::ptr::null_mut();
        }
        if !state.icon.is_null() {
            DestroyIcon(state.icon);
        }
    });
}

fn notify_data(hwnd: HWND, icon: HICON, tip: &str) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    for (slot, ch) in data.szTip.iter_mut().zip(tip.encode_utf16()) {
        *slot = ch;
    }
    data
}

unsafe extern "system" fn tray_wnd_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_CREATE => 0,
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_TIMER if wparam == WM_TRAY_TIMER => {
            let pending: Vec<TrayStatus> = TRAY.with(|cell| {
                let mut state = cell.borrow_mut();
                let mut items = Vec::new();
                // The receiver is moved into the WM_TIMER handler via a channel
                // stored on the thread state below.
                if let Some(rx) = state.status_rx.take() {
                    while let Ok(status) = rx.try_recv() {
                        items.push(status);
                    }
                    state.status_rx = Some(rx);
                }
                items
            });
            if let Some(status) = pending.last() {
                TRAY.with(|cell| {
                    let mut state = cell.borrow_mut();
                    state.status = status.clone();
                    let data = notify_data(state.hwnd, state.icon, "Music Player");
                    Shell_NotifyIconW(NIM_MODIFY, &data);
                });
            }
            0
        }
        WM_TRAY => match lparam as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                send(MediaCommand::Open);
                0
            }
            WM_RBUTTONUP => {
                show_tray_menu(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        WM_COMMAND => {
            match (wparam & 0xffff) as usize {
                TM_OPEN => send(MediaCommand::Open),
                TM_PREVIOUS => send(MediaCommand::Previous),
                TM_PLAY_PAUSE => send(MediaCommand::PlayPause),
                TM_NEXT => send(MediaCommand::Next),
                TM_QUIT => send(MediaCommand::Quit),
                _ => {}
            }
            0
        }
        // Deferred menu cleanup: posted by show_tray_menu after the
        // TrackPopupMenu call returns.
        _ if message == WM_APP + 2 => {
            if wparam != 0 {
                DestroyMenu(wparam as _);
            }
            0
        }
        _ if message == WM_DISABLE_MEDIA_KEYS => {
            TRAY.with(|cell| {
                let mut state = cell.borrow_mut();
                if !state.hook.is_null() {
                    UnhookWindowsHookEx(state.hook);
                    state.hook = std::ptr::null_mut();
                }
            });
            0
        }
        _ if TRAY.with(|cell| cell.borrow().show_message) == message => {
            send(MediaCommand::Open);
            0
        }
        _ if TRAY.with(|cell| cell.borrow().taskbar_created_msg) == message && message != 0 => {
            TRAY.with(|cell| {
                let state = cell.borrow();
                let data = notify_data(state.hwnd, state.icon, "Music Player");
                Shell_NotifyIconW(NIM_ADD, &data);
            });
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn send(command: MediaCommand) {
    let sent = TRAY.with(|cell| {
        if let Some(sender) = &cell.borrow().commands {
            sender.send(command).is_ok()
        } else {
            false
        }
    });
    if sent {
        let _ = slint::invoke_from_event_loop(|| {
            crate::with_app(|app| {
                app.poll_os_commands();
            });
        });
    }
}

unsafe fn enable_dark_mode_support(hwnd: HWND) {
    let uxtheme_str = b"uxtheme.dll\0";
    let uxtheme = LoadLibraryA(uxtheme_str.as_ptr());
    if !uxtheme.is_null() {
        let is_light = crate::native_io::prefers_light_theme();
        let app_mode = if is_light { 3 } else { 2 }; // 3 = ForceLight, 2 = ForceDark

        // Ordinal 135: SetPreferredAppMode (Windows 10 1903+) / AllowDarkModeForApp (1809)
        let set_app_mode: Option<unsafe extern "system" fn(i32) -> i32> =
            std::mem::transmute(GetProcAddress(uxtheme, 135 as *const u8));
        if let Some(set_mode) = set_app_mode {
            set_mode(app_mode);
        }
        // Ordinal 133: AllowDarkModeForWindow
        let allow_window: Option<unsafe extern "system" fn(HWND, i32) -> i32> =
            std::mem::transmute(GetProcAddress(uxtheme, 133 as *const u8));
        if let Some(allow) = allow_window {
            allow(hwnd, if is_light { 0 } else { 1 });
        }
        // Ordinal 136: FlushMenuThemes
        let flush_menu_themes: Option<unsafe extern "system" fn()> =
            std::mem::transmute(GetProcAddress(uxtheme, 136 as *const u8));
        if let Some(flush) = flush_menu_themes {
            flush();
        }
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    enable_dark_mode_support(hwnd);
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let (title, playing) = TRAY.with(|cell| {
        let state = cell.borrow();
        (state.status.clone(), state.status.playing)
    });
    let header = if title.title.is_empty() {
        "Music Player".to_string()
    } else if title.artist.is_empty() {
        title.title.clone()
    } else {
        format!("{} — {}", title.title, title.artist)
    };
    let header_wide: Vec<u16> = header.encode_utf16().chain(std::iter::once(0)).collect();
    let open: Vec<u16> = "Open Music Player\0".encode_utf16().collect();
    let previous: Vec<u16> = "Previous\0".encode_utf16().collect();
    let play: Vec<u16> = "Play\0".encode_utf16().collect();
    let pause: Vec<u16> = "Pause\0".encode_utf16().collect();
    let next: Vec<u16> = "Next\0".encode_utf16().collect();
    let quit: Vec<u16> = "Quit Music Player\0".encode_utf16().collect();

    AppendMenuW(menu, 0x0001 | 0x0002, 0, header_wide.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    AppendMenuW(menu, MF_STRING, TM_OPEN, open.as_ptr());
    AppendMenuW(menu, MF_STRING, TM_PREVIOUS, previous.as_ptr());
    AppendMenuW(
        menu,
        MF_STRING,
        TM_PLAY_PAUSE,
        if playing { pause.as_ptr() } else { play.as_ptr() },
    );
    AppendMenuW(menu, MF_STRING, TM_NEXT, next.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    AppendMenuW(menu, MF_STRING, TM_QUIT, quit.as_ptr());

    let mut point = std::mem::zeroed::<POINT>();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    let cmd_id = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RETURNCMD,
        point.x,
        point.y,
        0,
        hwnd,
        ptr::null(),
    );
    PostMessageW(hwnd, 0, 0, 0);
    match cmd_id as usize {
        TM_OPEN => send(MediaCommand::Open),
        TM_PREVIOUS => send(MediaCommand::Previous),
        TM_PLAY_PAUSE => send(MediaCommand::PlayPause),
        TM_NEXT => send(MediaCommand::Next),
        TM_QUIT => send(MediaCommand::Quit),
        _ => {}
    }
    DestroyMenu(menu);
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let key = (*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT)).vkCode;
        let command = match key {
            VK_MEDIA_PLAY_PAUSE => Some(MediaCommand::PlayPause),
            VK_MEDIA_NEXT_TRACK => Some(MediaCommand::Next),
            VK_MEDIA_PREV_TRACK => Some(MediaCommand::Previous),
            _ => None,
        };
        if let Some(command) = command {
            send(command);
            return 1; // swallow the key so other players do not react
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(windows)]
pub fn restore_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_RESTORE};
    if hwnd == 0 {
        return;
    }
    unsafe {
        let h = hwnd as HWND;
        let _ = ShowWindow(h, SW_RESTORE);
        let _ = SetForegroundWindow(h);
    }
}

#[cfg(not(windows))]
pub fn restore_window(_hwnd: isize) {}
