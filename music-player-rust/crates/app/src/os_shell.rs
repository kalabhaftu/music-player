//! Windows shell surface beyond the tray icon (Wave 5+):
//!
//! * System Media Transport Controls (SMTC) metadata and transport buttons.
//! * Taskbar thumbnail toolbar (Previous / Play-Pause / Next).
//! * `WM_APPCOMMAND` media keys on the main window.
//! * Render-endpoint enumeration for the audio settings page.
//!
//! All state lives on the Slint UI thread. Commands are pushed into the same
//! channel the tray icon uses and drained by the 250 ms clock.
#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::sync::mpsc::Sender;

use windows::core::{GUID, HSTRING};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Storage::StorageFile;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, PROPERTYKEY, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Media::Audio::{
    eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    PKEY_AudioEndpoint_GUID,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToStringAlloc,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::WinRT::{
    ISystemMediaTransportControlsInterop, RoGetActivationFactory, RoInitialize,
    RO_INIT_SINGLETHREADED,
};
use windows::Win32::UI::Shell::{
    ITaskbarList3, THUMBBUTTON, THUMBBUTTONMASK, THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_DISABLED,
    THBF_ENABLED, THBN_CLICKED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateIconIndirect, DefWindowProcW, DestroyIcon, GetWindowLongPtrW,
    RegisterWindowMessageW, SetWindowLongPtrW, GWLP_WNDPROC, ICONINFO, WM_APPCOMMAND, WM_COMMAND,
    WNDPROC,
};

use crate::os::MediaCommand;

// ---------------------------------------------------------------------------
// Thread state
// ---------------------------------------------------------------------------

thread_local! {
    static SHELL: RefCell<ShellState> = RefCell::new(ShellState::default());
    /// `TaskbarButtonCreated` is registered before the window is subclassed and
    /// is read from the window procedure, which Windows can call re-entrantly
    /// while the shell state is in use. Such values live outside the `RefCell`.
    static TASKBAR_MESSAGE: Cell<u32> = const { Cell::new(0) };
    /// Original window procedure, also needed from re-entrant calls.
    static OLD_WNDPROC: Cell<WNDPROC> = const { Cell::new(None) };
    /// `ThumbBarAddButtons` dispatches window messages synchronously; this flag
    /// keeps the re-entrant call from adding the same buttons twice.
    static ADDING_TASKBAR_BUTTONS: Cell<bool> = const { Cell::new(false) };
    static TASKBAR_ADD_PENDING: Cell<bool> = const { Cell::new(false) };
}

struct ShellState {
    hwnd: HWND,
    commands: Option<Sender<MediaCommand>>,
    smtc: Option<SystemMediaTransportControls>,
    smtc_meta: Option<SmtcMetadata>,
    smtc_playing: Option<bool>,
    taskbar: Option<ITaskbarList3>,
    buttons_added: bool,
    has_track: bool,
    playing: bool,
    icons: ThumbIcons,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            hwnd: HWND(std::ptr::null_mut()),
            commands: None,
            smtc: None,
            smtc_meta: None,
            smtc_playing: None,
            taskbar: None,
            buttons_added: false,
            has_track: false,
            playing: false,
            icons: ThumbIcons::default(),
        }
    }
}

#[derive(Clone)]
struct SmtcMetadata {
    title: String,
    artist: String,
    album: String,
    artwork: String,
}

#[derive(Default)]
struct ThumbIcons {
    previous: Option<windows::Win32::UI::WindowsAndMessaging::HICON>,
    play: Option<windows::Win32::UI::WindowsAndMessaging::HICON>,
    pause: Option<windows::Win32::UI::WindowsAndMessaging::HICON>,
    next: Option<windows::Win32::UI::WindowsAndMessaging::HICON>,
}

fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = RoInitialize(RO_INIT_SINGLETHREADED);
    }
}

fn log_warning(area: &str, message: &str, error: &dyn std::fmt::Display) {
    music_player_core::LocalAppLog::shared().warning(
        area,
        &format!("{message} ({error})"),
    );
}

// ---------------------------------------------------------------------------
// Attach / detach
// ---------------------------------------------------------------------------

/// Attaches SMTC and the taskbar thumbnail toolbar to the Slint window.
///
/// Must be called on the UI thread after the native window exists (the Slint
/// event loop has processed at least one iteration after `show`).
pub fn attach_main_window(hwnd: isize, commands: Sender<MediaCommand>) -> bool {
    if hwnd == 0 {
        return false;
    }
    ensure_com();
    let window = HWND(hwnd as *mut core::ffi::c_void);

    let smtc = match unsafe { create_smtc(window, commands.clone()) } {
        Ok(controls) => Some(controls),
        Err(error) => {
            log_warning(
                "system-media-controls",
                "Could not initialize Windows media controls.",
                &error,
            );
            None
        }
    };
    let smtc_available = smtc.is_some();

    let taskbar_message = unsafe {
        let wide: Vec<u16> = "TaskbarButtonCreated"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        RegisterWindowMessageW(windows::core::PCWSTR(wide.as_ptr()))
    };
    let icons = ThumbIcons {
        previous: create_thumb_icon(IconGlyph::Previous),
        play: create_thumb_icon(IconGlyph::Play),
        pause: create_thumb_icon(IconGlyph::Pause),
        next: create_thumb_icon(IconGlyph::Next),
    };

    // Publish the state before the subclass is installed so the first message
    // already sees a fully initialised shell.
    SHELL.with(|cell| {
        let mut state = cell.borrow_mut();
        state.hwnd = window;
        state.commands = Some(commands);
        state.smtc = smtc;
        state.smtc_meta = None;
        state.smtc_playing = None;
        state.icons = icons;
    });
    TASKBAR_MESSAGE.set(taskbar_message);

    unsafe {
        let old = GetWindowLongPtrW(window, GWLP_WNDPROC);
        if old != 0 {
            OLD_WNDPROC.set(std::mem::transmute::<isize, WNDPROC>(old));
        }
        SetWindowLongPtrW(
            window,
            GWLP_WNDPROC,
            shell_wnd_proc as *const () as usize as isize,
        );
    }

    smtc_available
}

/// Restores the original window procedure and releases shell resources.
pub fn detach() {
    let hwnd = SHELL.with(|cell| cell.borrow().hwnd);
    let old = OLD_WNDPROC.take();
    unsafe {
        if let Some(old) = old {
            if !hwnd.0.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, old as usize as isize);
            }
        }
    }
    TASKBAR_MESSAGE.set(0);
    SHELL.with(|cell| {
        let mut state = cell.borrow_mut();
        unsafe {
            for icon in [
                state.icons.previous.take(),
                state.icons.play.take(),
                state.icons.pause.take(),
                state.icons.next.take(),
            ]
            .into_iter()
            .flatten()
            {
                let _ = DestroyIcon(icon);
            }
        }
        state.icons = ThumbIcons::default();
        state.taskbar = None;
        state.smtc = None;
        state.smtc_meta = None;
        state.commands = None;
        state.buttons_added = false;
        state.hwnd = HWND(std::ptr::null_mut());
    });
}

// ---------------------------------------------------------------------------
// SMTC
// ---------------------------------------------------------------------------

unsafe fn create_smtc(
    hwnd: HWND,
    commands: Sender<MediaCommand>,
) -> windows::core::Result<SystemMediaTransportControls> {
    // `ISystemMediaTransportControlsInterop` is implemented by the activation
    // factory of the runtime class itself; there is no separate "…Interop"
    // class (RoGetActivationFactory then fails with REGDB_E_CLASSNOTREG).
    let factory: ISystemMediaTransportControlsInterop = RoGetActivationFactory(
        &HSTRING::from("Windows.Media.SystemMediaTransportControls"),
    )?;
    let controls: SystemMediaTransportControls = factory.GetForWindow(hwnd)?;
    controls.SetIsEnabled(true)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsNextEnabled(true)?;
    controls.SetIsPreviousEnabled(true)?;
    controls.ButtonPressed(&TypedEventHandler::new(
        move |_, args: windows::core::Ref<
            windows::Media::SystemMediaTransportControlsButtonPressedEventArgs,
        >| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let command = match args.Button()? {
                SystemMediaTransportControlsButton::Play => Some(MediaCommand::Play),
                SystemMediaTransportControlsButton::Pause => Some(MediaCommand::Pause),
                SystemMediaTransportControlsButton::Next => Some(MediaCommand::Next),
                SystemMediaTransportControlsButton::Previous => Some(MediaCommand::Previous),
                _ => None,
            };
            if let Some(command) = command {
                let _ = commands.send(command);
            }
            Ok(())
        },
    ))?;
    Ok(controls)
}

/// Pushes the current track/state into the shell surfaces every clock tick.
#[allow(clippy::too_many_arguments)]
pub fn update_playback(
    title: &str,
    artist: &str,
    album: &str,
    artwork: Option<&str>,
    has_track: bool,
    playing: bool,
    position_ms: u64,
    duration_ms: u64,
) {
    // Snapshot everything Win32/COM needs, then release the `RefCell` before
    // calling into the shell. ThumbBarUpdateButtons can dispatch messages
    // synchronously back into `shell_wnd_proc`.
    let (hwnd, buttons_added, taskbar, buttons) = SHELL.with(|cell| {
        let mut state = cell.borrow_mut();
        state.has_track = has_track;
        state.playing = playing;
        let buttons = thumb_buttons(&state);
        (
            state.hwnd,
            state.buttons_added,
            state.taskbar.clone(),
            buttons,
        )
    });
    if buttons_added {
        if let Some(taskbar) = taskbar {
            let _ = unsafe { taskbar.ThumbBarUpdateButtons(hwnd, &buttons) };
        }
    }

    let track_present = has_track && !title.is_empty();
    let artwork = artwork.unwrap_or("");
    let (smtc, smtc_playing, metadata_changed) = SHELL.with(|cell| {
        let state = cell.borrow();
        let changed = state
            .smtc_meta
            .as_ref()
            .map(|current| {
                current.title != title
                    || current.artist != artist
                    || current.album != album
                    || current.artwork != artwork
            })
            .unwrap_or(true);
        (state.smtc.clone(), state.smtc_playing, changed)
    });
    let Some(controls) = smtc else {
        return;
    };

    if smtc_playing != Some(playing) {
        let status = if playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        };
        let _ = controls.SetPlaybackStatus(status);
        SHELL.with(|cell| cell.borrow_mut().smtc_playing = Some(playing));
    }
    if !track_present {
        return;
    }

    if metadata_changed {
        if let Err(error) =
            unsafe { update_smtc_metadata(&controls, title, artist, album, artwork) }
        {
            log_warning(
                "system-media-controls",
                "Could not update Windows media controls.",
                &error,
            );
        }
        SHELL.with(|cell| {
            cell.borrow_mut().smtc_meta = Some(SmtcMetadata {
                title: title.to_string(),
                artist: artist.to_string(),
                album: album.to_string(),
                artwork: artwork.to_string(),
            });
        });
    }

    if duration_ms > 0 {
        if let Ok(timeline) = SystemMediaTransportControlsTimelineProperties::new() {
            let _ = timeline.SetStartTime(TimeSpan { Duration: 0 });
            let _ = timeline.SetEndTime(TimeSpan {
                Duration: duration_ms.min(i64::MAX as u64) as i64 * 10_000,
            });
            let _ = timeline.SetPosition(TimeSpan {
                Duration: position_ms.min(duration_ms) as i64 * 10_000,
            });
            let _ = controls.UpdateTimelineProperties(&timeline);
        }
    }
}

unsafe fn update_smtc_metadata(
    controls: &SystemMediaTransportControls,
    title: &str,
    artist: &str,
    album: &str,
    artwork: &str,
) -> windows::core::Result<()> {
    let updater = controls.DisplayUpdater()?;
    updater.SetType(MediaPlaybackType::Music)?;
    let music = updater.MusicProperties()?;
    music.SetTitle(&HSTRING::from(title))?;
    music.SetArtist(&HSTRING::from(artist))?;
    music.SetAlbumTitle(&HSTRING::from(album))?;

    if !artwork.is_empty() && std::path::Path::new(artwork).is_file() {
        if let Ok(operation) = StorageFile::GetFileFromPathAsync(&HSTRING::from(artwork)) {
            if let Ok(file) = operation.join() {
                if let Ok(reference) = RandomAccessStreamReference::CreateFromFile(&file) {
                    let _ = updater.SetThumbnail(&reference);
                }
            }
        }
    } else {
        let _ = updater.SetThumbnail(None::<&RandomAccessStreamReference>);
    }
    updater.Update()
}

// ---------------------------------------------------------------------------
// Taskbar thumbnail toolbar
// ---------------------------------------------------------------------------

fn thumb_buttons(state: &ShellState) -> Vec<THUMBBUTTON> {
    let flags = if state.has_track {
        THBF_ENABLED
    } else {
        THBF_DISABLED
    };
    vec![
        thumb_button(1, state.icons.previous, "Previous track", flags),
        thumb_button(
            2,
            if state.playing {
                state.icons.pause
            } else {
                state.icons.play
            },
            if state.playing { "Pause" } else { "Play" },
            flags,
        ),
        thumb_button(3, state.icons.next, "Next track", flags),
    ]
}

fn thumb_button(
    id: u32,
    icon: Option<windows::Win32::UI::WindowsAndMessaging::HICON>,
    tooltip: &str,
    flags: windows::Win32::UI::Shell::THUMBBUTTONFLAGS,
) -> THUMBBUTTON {
    let mut button = THUMBBUTTON {
        dwMask: THUMBBUTTONMASK(THB_ICON.0 | THB_TOOLTIP.0 | THB_FLAGS.0),
        iId: id,
        hIcon: icon.unwrap_or_default(),
        dwFlags: flags,
        ..Default::default()
    };
    for (slot, ch) in button.szTip.iter_mut().zip(tooltip.encode_utf16()) {
        *slot = ch;
    }
    button
}

fn send_media_command(command: MediaCommand) -> bool {
    SHELL.with(|cell| {
        cell.borrow()
            .commands
            .as_ref()
            .map(|sender| sender.send(command).is_ok())
            .unwrap_or(false)
    })
}

fn is_playing() -> bool {
    SHELL.with(|cell| cell.borrow().playing)
}

fn add_taskbar_buttons() {
    if ADDING_TASKBAR_BUTTONS.get() {
        // Re-entered by a message dispatched from ThumbBarAddButtons itself;
        // the outer call finishes the job.
        TASKBAR_ADD_PENDING.set(true);
        return;
    }
    let (hwnd, already_added, buttons) = SHELL.with(|cell| {
        let state = cell.borrow();
        (state.hwnd, state.buttons_added, thumb_buttons(&state))
    });
    if already_added || hwnd.0.is_null() {
        return;
    }

    ADDING_TASKBAR_BUTTONS.set(true);
    let result = unsafe {
        let taskbar: windows::core::Result<ITaskbarList3> = CoCreateInstance(
            &GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090),
            None::<&windows::core::IUnknown>,
            CLSCTX_ALL,
        );
        taskbar.and_then(|taskbar| {
            taskbar.HrInit()?;
            taskbar.ThumbBarAddButtons(hwnd, &buttons)?;
            Ok(taskbar)
        })
    };
    ADDING_TASKBAR_BUTTONS.set(false);

    match result {
        Ok(taskbar) => {
            SHELL.with(|cell| {
                let mut state = cell.borrow_mut();
                state.buttons_added = true;
                state.taskbar = Some(taskbar);
            });
            TASKBAR_ADD_PENDING.set(false);
        }
        Err(error) => {
            log_warning(
                "taskbar-controls",
                "Could not add playback controls to the taskbar preview.",
                &error,
            );
            // A TaskbarButtonCreated message that arrived while the first
            // attempt was running may succeed on a second try.
            if TASKBAR_ADD_PENDING.replace(false) {
                let added = SHELL.with(|cell| cell.borrow().buttons_added);
                if !added {
                    add_taskbar_buttons();
                }
            }
        }
    }
}

unsafe extern "system" fn shell_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_message = TASKBAR_MESSAGE.get();
    if taskbar_message != 0 && message == taskbar_message {
        add_taskbar_buttons();
        return LRESULT(0);
    }

    if message == WM_COMMAND {
        let command = ((wparam.0 as u64) >> 16) & 0xffff;
        if command == THBN_CLICKED as u64 {
            let id = (wparam.0 as u64) & 0xffff;
            let command = match id {
                1 => Some(MediaCommand::Previous),
                2 => Some(MediaCommand::PlayPause),
                3 => Some(MediaCommand::Next),
                _ => None,
            };
            if let Some(command) = command {
                if send_media_command(command) {
                    return LRESULT(0);
                }
            }
        }
    }

    if message == WM_APPCOMMAND {
        // GET_APPCOMMAND_LPARAM: drop the input-device bits in HIWORD.
        let command = ((lparam.0 as u64) >> 16) & 0x0fff;
        let playing = is_playing();
        let command = match command {
            11 => Some(MediaCommand::Next),
            12 => Some(MediaCommand::Previous),
            14 => Some(MediaCommand::PlayPause),
            46 if !playing => Some(MediaCommand::Play),
            47 if playing => Some(MediaCommand::Pause),
            _ => None,
        };
        if let Some(command) = command {
            if send_media_command(command) {
                // Returning TRUE keeps the shell's global controller from also
                // handling the key and toggling playback twice.
                return LRESULT(1);
            }
        }
    }

    let old = OLD_WNDPROC.get();
    match old {
        Some(old) => CallWindowProcW(Some(old), hwnd, message, wparam, lparam),
        None => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// Hand-drawn 32 px transport icons (port of TaskbarPeekControls.CreateIcon)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum IconGlyph {
    Previous,
    Play,
    Pause,
    Next,
}

fn create_thumb_icon(
    glyph: IconGlyph,
) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    const SIZE: usize = 32;
    const SAMPLES: usize = 4;
    let mut pixels = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut covered = 0usize;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f64 + (sx as f64 + 0.5) / SAMPLES as f64;
                    let py = y as f64 + (sy as f64 + 0.5) / SAMPLES as f64;
                    if glyph_contains(glyph, px, py) {
                        covered += 1;
                    }
                }
            }
            if covered == 0 {
                continue;
            }
            let offset = (y * SIZE + x) * 4;
            pixels[offset] = 255;
            pixels[offset + 1] = 255;
            pixels[offset + 2] = 255;
            pixels[offset + 3] = (covered * 255 / (SAMPLES * SAMPLES)) as u8;
        }
    }

    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE as i32,
                biHeight: -(SIZE as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return None;
        };
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(color.0));
            return None;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
        let mask_bytes = vec![0u8; SIZE * SIZE / 8];
        let mask = CreateBitmap(
            SIZE as i32,
            SIZE as i32,
            1,
            1,
            Some(mask_bytes.as_ptr() as *const core::ffi::c_void),
        );
        let icon_info = ICONINFO {
            fIcon: windows::core::BOOL(1),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&icon_info).ok();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}

fn glyph_contains(glyph: IconGlyph, x: f64, y: f64) -> bool {
    fn rect(x: f64, y: f64, left: f64, top: f64, right: f64, bottom: f64) -> bool {
        x >= left && x <= right && y >= top && y <= bottom
    }
    fn triangle(
        x: f64,
        y: f64,
        ax: f64,
        ay: f64,
        bx: f64,
        by: f64,
        cx: f64,
        cy: f64,
    ) -> bool {
        fn side(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
            (px - bx) * (ay - by) - (ax - bx) * (py - by)
        }
        let a = side(x, y, ax, ay, bx, by);
        let b = side(x, y, bx, by, cx, cy);
        let c = side(x, y, cx, cy, ax, ay);
        (a >= 0.0 && b >= 0.0 && c >= 0.0) || (a <= 0.0 && b <= 0.0 && c <= 0.0)
    }
    match glyph {
        IconGlyph::Play => triangle(x, y, 8.0, 5.0, 8.0, 27.0, 27.0, 16.0),
        IconGlyph::Pause => {
            rect(x, y, 7.0, 5.0, 13.0, 27.0) || rect(x, y, 19.0, 5.0, 25.0, 27.0)
        }
        IconGlyph::Previous => {
            rect(x, y, 3.0, 7.0, 6.0, 25.0)
                || triangle(x, y, 7.0, 16.0, 18.0, 7.0, 18.0, 25.0)
                || triangle(x, y, 15.0, 16.0, 27.0, 7.0, 27.0, 25.0)
        }
        IconGlyph::Next => {
            rect(x, y, 26.0, 7.0, 29.0, 25.0)
                || triangle(x, y, 17.0, 7.0, 17.0, 25.0, 28.0, 16.0)
                || triangle(x, y, 7.0, 7.0, 7.0, 25.0, 19.0, 16.0)
        }
    }
}

// ---------------------------------------------------------------------------
// Audio render endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AudioOutputDevice {
    pub id: String,
    pub name: String,
}

/// Enumerates active render endpoints in the same order the original settings
/// page used (friendly name, ordinal-ignore-case), with duplicate IDs removed.
pub fn audio_output_devices() -> Vec<AudioOutputDevice> {
    ensure_com();
    let mut devices = Vec::new();
    unsafe {
        let enumerator: IMMDeviceEnumerator = match CoCreateInstance(
            &MMDeviceEnumerator,
            None::<&windows::core::IUnknown>,
            CLSCTX_ALL,
        ) {
            Ok(enumerator) => enumerator,
            Err(error) => {
                log_warning(
                    "audio-devices",
                    "Could not list Windows audio devices.",
                    &error,
                );
                return devices;
            }
        };
        let collection = match enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
            Ok(collection) => collection,
            Err(error) => {
                log_warning(
                    "audio-devices",
                    "Could not list Windows audio devices.",
                    &error,
                );
                return devices;
            }
        };
        let count = collection.GetCount().unwrap_or(0);
        for index in 0..count {
            let Ok(device) = collection.Item(index) else {
                continue;
            };
            let Some(id) = device_id(&device) else {
                continue;
            };
            if id.trim().is_empty() {
                continue;
            }
            let name = device_property(&device, &PKEY_Device_FriendlyName).unwrap_or_default();
            if name.trim().is_empty() {
                continue;
            }
            if devices
                .iter()
                .any(|existing: &AudioOutputDevice| existing.id.eq_ignore_ascii_case(&id))
            {
                continue;
            }
            devices.push(AudioOutputDevice { id, name });
        }
    }
    devices.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
    });
    devices
}

/// Maps a WinRT/MMDevice endpoint ID to the DirectSound GUID LibVLC expects.
pub fn direct_sound_device_id(endpoint_id: &str) -> Option<String> {
    let trimmed = endpoint_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(guid) = canonical_guid(trimmed) {
        return Some(guid);
    }
    ensure_com();
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None::<&windows::core::IUnknown>,
            CLSCTX_ALL,
        )
        .ok()?;
        let device = enumerator
            .GetDevice(&HSTRING::from(endpoint_id_from_winrt(trimmed)))
            .ok()?;
        let guid = device_property(&device, &PKEY_AudioEndpoint_GUID)?;
        canonical_guid(&guid)
    }
}

/// WinRT device-interface IDs wrap the MMDevice endpoint ID in `#` sections.
fn endpoint_id_from_winrt(id: &str) -> &str {
    let lower = id.to_lowercase();
    let Some(start) = lower.find("{0.0.0.") else {
        return id;
    };
    match id[start..].find('#') {
        Some(end) => &id[start..start + end],
        None => &id[start..],
    }
}

fn canonical_guid(value: &str) -> Option<String> {
    let bare = value
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}');
    let guid = GUID::try_from(bare).ok()?;
    Some(format!("{{{:?}}}", guid).to_lowercase())
}

unsafe fn device_id(device: &IMMDevice) -> Option<String> {
    let id = device.GetId().ok()?;
    let text = id.to_string().ok();
    CoTaskMemFree(Some(id.0 as *const core::ffi::c_void));
    text
}

unsafe fn device_property(device: &IMMDevice, key: &PROPERTYKEY) -> Option<String> {
    let store = device.OpenPropertyStore(STGM_READ).ok()?;
    let mut value = store.GetValue(key).ok()?;
    let text = PropVariantToStringAlloc(&value)
        .ok()
        .and_then(|wide| {
            let text = wide.to_string().ok();
            CoTaskMemFree(Some(wide.0 as *const core::ffi::c_void));
            text
        });
    let _ = PropVariantClear(&mut value);
    text
}
