//! Native Win32 dialogs, clipboard, and shell operations.
//! Eliminates fragile PowerShell subprocesses and provides instant response.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use windows::core::w;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    Common::COMDLG_FILTERSPEC, FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog,
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, ShellExecuteW, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_sys::Win32::Foundation::HGLOBAL;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

struct ComApartment;

impl ComApartment {
    fn enter() -> Option<Self> {
        // S_OK and S_FALSE both require one matching CoUninitialize call. If
        // the UI thread is already initialized with another apartment model,
        // do not invoke shell COM interfaces from the wrong apartment.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok()? };
        Some(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        hKey: isize,
        lpSubKey: *const u16,
        ulOptions: u32,
        samDesired: u32,
        phkResult: *mut isize,
    ) -> i32;

    fn RegQueryValueExW(
        hKey: isize,
        lpValueName: *const u16,
        lpReserved: *const u32,
        lpType: *mut u32,
        lpData: *mut u8,
        lpcbData: *mut u32,
    ) -> i32;

    fn RegCloseKey(hKey: isize) -> i32;
}

const HKEY_CURRENT_USER: isize = -2147483647isize; // 0x80000001 as i32 as isize
const KEY_QUERY_VALUE: u32 = 0x0001;

pub fn prefers_light_theme() -> bool {
    unsafe {
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let value_name = w!("AppsUseLightTheme");
        let mut hkey: isize = 0;
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        ) != 0
        {
            return false;
        }

        let mut data: u32 = 0;
        let mut data_len = std::mem::size_of::<u32>() as u32;
        let res = RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut u8,
            &mut data_len,
        );
        RegCloseKey(hkey);

        res == 0 && data == 1
    }
}

fn display_path(item: windows::Win32::UI::Shell::IShellItem) -> Option<String> {
    unsafe {
        let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let value = path.to_string().ok();
        CoTaskMemFree(Some(path.0.cast()));
        value
    }
}

pub fn pick_folder() -> Option<String> {
    unsafe {
        let _com = ComApartment::enter()?;
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;
        let _ = dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
        let _ = dialog.SetTitle(w!("Select Music Folder"));
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        display_path(item)
    }
}

pub fn pick_file(filter: &str) -> Option<String> {
    unsafe {
        let _com = ComApartment::enter()?;
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;
        let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM);

        let (desc, spec) = if let Some((d, s)) = filter.split_once('|') {
            (d, s)
        } else {
            ("All Files (*.*)", "*.*")
        };

        let desc_u16: Vec<u16> = desc.encode_utf16().chain(std::iter::once(0)).collect();
        let spec_u16: Vec<u16> = spec.encode_utf16().chain(std::iter::once(0)).collect();
        let filter_spec = [COMDLG_FILTERSPEC {
            pszName: windows::core::PCWSTR(desc_u16.as_ptr()),
            pszSpec: windows::core::PCWSTR(spec_u16.as_ptr()),
        }];
        let _ = dialog.SetFileTypes(&filter_spec);
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        display_path(item)
    }
}

pub fn save_file(default_name: &str, filter: &str) -> Option<String> {
    unsafe {
        let _com = ComApartment::enter()?;
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL).ok()?;
        let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM);
        let name_u16: Vec<u16> = default_name.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = dialog.SetFileName(windows::core::PCWSTR(name_u16.as_ptr()));

        let (desc, spec) = if let Some((d, s)) = filter.split_once('|') {
            (d, s)
        } else {
            ("All Files (*.*)", "*.*")
        };

        let desc_u16: Vec<u16> = desc.encode_utf16().chain(std::iter::once(0)).collect();
        let spec_u16: Vec<u16> = spec.encode_utf16().chain(std::iter::once(0)).collect();
        let filter_spec = [COMDLG_FILTERSPEC {
            pszName: windows::core::PCWSTR(desc_u16.as_ptr()),
            pszSpec: windows::core::PCWSTR(spec_u16.as_ptr()),
        }];
        let _ = dialog.SetFileTypes(&filter_spec);
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        display_path(item)
    }
}

pub fn set_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        EmptyClipboard();
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes_len = wide.len() * std::mem::size_of::<u16>();
        let h_global: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes_len);
        if !h_global.is_null() {
            let ptr = GlobalLock(h_global) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                GlobalUnlock(h_global);
                // CF_UNICODETEXT = 13
                SetClipboardData(13, h_global);
            }
        }
        CloseClipboard();
    }
}

pub fn open_url(url: &str) {
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let op = w!("open");
    unsafe {
        ShellExecuteW(
            None,
            op,
            windows::core::PCWSTR(wide.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_in_explorer(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path))
        .spawn();
}

pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

struct ArtworkCache(std::sync::Mutex<HashMap<PathBuf, slint::Image>>);
unsafe impl Send for ArtworkCache {}
unsafe impl Sync for ArtworkCache {}

impl ArtworkCache {
    fn lock(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, HashMap<PathBuf, slint::Image>>,
        std::sync::PoisonError<std::sync::MutexGuard<'_, HashMap<PathBuf, slint::Image>>>,
    > {
        self.0.lock()
    }
}

static ARTWORK_CACHE: std::sync::OnceLock<ArtworkCache> = std::sync::OnceLock::new();

/// Safely decodes any image format (JPEG, PNG, JFIF, BMP, WebP) directly from
/// memory bytes or file paths, caching in memory to avoid UI thread lag.
pub fn load_artwork_image(path: &Path) -> Option<slint::Image> {
    let cache = ARTWORK_CACHE.get_or_init(|| ArtworkCache(std::sync::Mutex::new(HashMap::new())));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(path) {
            return Some(cached.clone());
        }
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    let is_standard_ext = matches!(ext.as_deref(), Some("jpg") | Some("jpeg") | Some("png"));

    let loaded = if is_standard_ext {
        slint::Image::load_from_path(path).ok()
    } else {
        None
    };

    let image = match loaded {
        Some(img) => img,
        None => {
            let bytes = std::fs::read(path).ok()?;
            let dyn_img = image::load_from_memory(&bytes).ok()?;
            let dyn_img = dyn_img.resize(384, 384, image::imageops::FilterType::Triangle);
            let rgba = dyn_img.to_rgba8();
            let (width, height) = rgba.dimensions();
            if width == 0 || height == 0 {
                return None;
            }
            let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
            for (dest, src) in buffer.make_mut_slice().iter_mut().zip(rgba.chunks_exact(4)) {
                dest.r = src[0];
                dest.g = src[1];
                dest.b = src[2];
                dest.a = src[3];
            }
            slint::Image::from_rgba8(buffer)
        }
    };

    if let Ok(mut guard) = cache.lock() {
        guard.insert(path.to_path_buf(), image.clone());
    }

    Some(image)
}

#[derive(Clone, Copy, Debug)]
pub struct ArtworkPalette {
    pub background: (u8, u8, u8),
    pub sidebar: (u8, u8, u8),
    pub main: (u8, u8, u8),
    pub accent: (u8, u8, u8),
}

/// 1:1 port of C# `ArtworkColors.FromRgba`.
/// Groups cover hues across brightness levels, ignoring transparent pixels and tiny color specks.
pub fn extract_artwork_palette(path: &Path) -> Option<ArtworkPalette> {
    let bytes = std::fs::read(path).ok()?;
    let dyn_img = image::load_from_memory(&bytes).ok()?;
    let small = dyn_img.resize_exact(32, 32, image::imageops::FilterType::Triangle).to_rgba8();
    let pixels = small.as_raw();

    let mut bins = [(0.0f64, 0.0f64, 0.0f64, 0.0f64); 19];
    let mut visible = 0.0f64;
    for chunk in pixels.chunks_exact(4) {
        let alpha = chunk[3] as f64 / 255.0;
        if alpha < 0.1 { continue; }
        let r = chunk[0] as f64 / 255.0;
        let g = chunk[1] as f64 / 255.0;
        let b = chunk[2] as f64 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = max - min;
        let saturation = if max == 0.0 { 0.0 } else { chroma / max };
        let hue = if chroma == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / chroma + 6.0) % 6.0
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        };
        let bin_index = if saturation >= 0.18 && max >= 0.08 {
            ((hue * 3.0).floor() as usize) % 18
        } else {
            18
        };
        bins[bin_index].0 += alpha;
        bins[bin_index].1 += chunk[0] as f64 * alpha;
        bins[bin_index].2 += chunk[1] as f64 * alpha;
        bins[bin_index].3 += chunk[2] as f64 * alpha;
        visible += alpha;
    }
    let mut selected = 18;
    for i in 0..18 {
        if bins[i].0 >= visible * 0.05 && (selected == 18 || bins[i].0 > bins[selected].0) {
            selected = i;
        }
    }
    let chosen = bins[selected];
    let clamp_byte = |v: f64| -> u8 { v.round().clamp(0.0, 255.0) as u8 };
    let mut seed = if chosen.0 > 0.0 {
        (clamp_byte(chosen.1 / chosen.0), clamp_byte(chosen.2 / chosen.0), clamp_byte(chosen.3 / chosen.0))
    } else {
        (128, 128, 128)
    };
    if selected == 18 {
        let gray = clamp_byte(0.2126 * seed.0 as f64 + 0.7152 * seed.1 as f64 + 0.0722 * seed.2 as f64);
        seed = (gray, gray, gray);
    }
    let maximum = 1.0f64.max((seed.0 as f64).max(seed.1 as f64).max(seed.2 as f64));
    let tone = |shade: f64| -> (u8, u8, u8) {
        (
            clamp_byte(10.0 + seed.0 as f64 / maximum * shade),
            clamp_byte(10.0 + seed.1 as f64 / maximum * shade),
            clamp_byte(10.0 + seed.2 as f64 / maximum * shade),
        )
    };
    let background = tone(38.0);
    let sidebar = tone(34.0);
    let main = tone(42.0);
    Some(ArtworkPalette {
        background,
        sidebar,
        main,
        accent: seed,
    })
}

