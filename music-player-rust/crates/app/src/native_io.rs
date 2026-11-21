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
    if !path.is_file() {
        return None;
    }

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

pub use music_player_core::ArtworkPalette;

/// Extracts an artwork color palette for the given image path using the core palette extractor.
pub fn extract_artwork_palette(path: &Path) -> Option<ArtworkPalette> {
    if path.is_file() {
        Some(music_player_core::extract_palette(path))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_artwork_palette_nonexistent() {
        assert!(extract_artwork_palette(Path::new("nonexistent_test_art.jpg")).is_none());
    }
}

