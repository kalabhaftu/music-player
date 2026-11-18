//! Native Win32 dialogs, clipboard, and shell operations.
//! Eliminates fragile PowerShell subprocesses and provides instant response.

use std::path::Path;
use windows::core::w;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::Graphics::Gdi::{GetSysColor, COLOR_WINDOW};
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

pub fn prefers_light_theme() -> bool {
    let color = unsafe { GetSysColor(COLOR_WINDOW) };
    let red = (color & 0xff) as f32;
    let green = ((color >> 8) & 0xff) as f32;
    let blue = ((color >> 16) & 0xff) as f32;
    (0.2126 * red + 0.7152 * green + 0.0722 * blue) >= 128.0
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
        .arg(format!("/select,\"{}\"", path))
        .spawn();
}

pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// Safely decodes any image format (JPEG, PNG, JFIF, BMP, WebP) directly from
/// memory bytes without relying on file extensions, preventing crashes on `.img` files.
pub fn load_artwork_image(path: &Path) -> Option<slint::Image> {
    let bytes = std::fs::read(path).ok()?;
    let dyn_img = image::load_from_memory(&bytes).ok()?;
    let dyn_img = dyn_img.thumbnail(384, 384);
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
    Some(slint::Image::from_rgba8(buffer))
}

