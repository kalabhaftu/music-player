//! Startup artwork window — 1:1 port of `StartupSplash.cs`.
//!
//! A per-pixel-alpha native window: only the app artwork and the status text
//! are visible. It does not create a second Slint tree, taskbar button, border,
//! or background.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    DIB_RGB_COLORS, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateBitmapFromScan0, GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreateSolidFill,
    GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteFont, GdipDeleteFontFamily,
    GdipDeleteGraphics, GdipDeleteStringFormat, GdipDisposeImage, GdipDrawImageRectI, GdipDrawString,
    GdipFlush, GdipGetImageGraphicsContext, GdipGraphicsClear, GdipSetInterpolationMode,
    GdipSetStringFormatAlign, GdipSetTextRenderingHint, GdiplusShutdown, GdiplusStartup,
    GdiplusStartupInput, GpBitmap, GpBrush, GpFont, GpFontFamily, GpGraphics, GpImage,
    GpSolidFill, GpStringFormat, FlushIntentionSync, FontStyleBold, InterpolationModeHighQualityBicubic,
    RectF, Status, StringAlignmentCenter, TextRenderingHintAntiAlias, UnitPixel,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, RegisterClassExW,
    ShowWindow, UpdateLayeredWindow, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use music_player_core::LocalAppLog;

/// The dark artwork shipped with the C# app. Embedded so the splash works
/// before (and independently of) any asset deployment step.
const ARTWORK: &[u8] = include_bytes!("../assets/MusicPlayer-dark.png");

const CLASS_NAME: PCWSTR = w!("MusicPlayer.StartupArtwork");
const WINDOW_TITLE: PCWSTR = w!("Music Player");

pub struct Splash {
    window: HWND,
}

impl Splash {
    /// Port of `StartupSplash.TryShow`: failures are logged and the app keeps
    /// starting without a splash.
    pub fn try_show() -> Option<Self> {
        match Self::create() {
            Ok(splash) => Some(splash),
            Err(error) => {
                LocalAppLog::shared().warning(
                    "startup",
                    &format!("Could not show startup artwork. ({error})"),
                );
                None
            }
        }
    }

    fn create() -> Result<Self, String> {
        unsafe {
            // The splash is first on screen; match the C# manifest so the
            // layered window is sized for the real monitor DPI.
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

            let instance = GetModuleHandleW(None)
                .map_err(|error| format!("GetModuleHandle failed ({error})"))?;
            let window_class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: HINSTANCE(instance.0),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassExW(&window_class) == 0 {
                return Err("RegisterClassEx failed".into());
            }

            let window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS_NAME,
                WINDOW_TITLE,
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(HINSTANCE(instance.0)),
                None,
            )
            .map_err(|error| format!("CreateWindowEx failed ({error})"))?;

            let splash = Self { window };
            if let Err(error) = splash.render("Opening") {
                drop(splash);
                return Err(error);
            }
            Ok(splash)
        }
    }

    /// Port of `StartupSplash.SetStatus`: rendering failures are logged and the
    /// previous artwork stays visible.
    pub fn set_status(&self, status: &str) {
        if let Err(error) = self.render(status) {
            LocalAppLog::shared().warning(
                "startup",
                &format!("Could not update startup status. ({error})"),
            );
        }
    }

    fn render(&self, status: &str) -> Result<(), String> {
        unsafe {
            let scale = GetDpiForWindow(self.window) as f32 / 96.0;
            let width = (240.0 * scale) as i32;
            let height = (224.0 * scale) as i32;

            let screen = GetDC(None);
            if screen.0.is_null() {
                return Err("GetDC failed".into());
            }
            let dc = CreateCompatibleDC(Some(screen));
            if dc.0.is_null() {
                ReleaseDC(None, screen);
                return Err("CreateCompatibleDC failed".into());
            }

            let mut info = BITMAPINFO::default();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            };
            let mut pixels: *mut c_void = ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut pixels, None, 0)
            {
                Ok(bitmap) if !bitmap.0.is_null() => bitmap,
                _ => {
                    let _ = DeleteDC(dc);
                    ReleaseDC(None, screen);
                    return Err("CreateDIBSection failed".into());
                }
            };
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));

            let mut token = 0usize;
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                ..Default::default()
            };
            let startup_status = GdiplusStartup(&mut token, &input, ptr::null_mut());

            let mut image: *mut GpBitmap = ptr::null_mut();
            let mut graphics: *mut GpGraphics = ptr::null_mut();
            let mut family: *mut GpFontFamily = ptr::null_mut();
            let mut font: *mut GpFont = ptr::null_mut();
            let mut brush: *mut GpSolidFill = ptr::null_mut();
            let mut shadow: *mut GpSolidFill = ptr::null_mut();
            let mut format: *mut GpStringFormat = ptr::null_mut();
            let mut icon: *mut GpBitmap = ptr::null_mut();
            // `GdipCreateBitmapFromScan0` references this buffer; it must stay
            // alive until after the artwork has been drawn.
            let mut artwork_buffer: Option<image::RgbaImage> = None;

            let result = (|| -> Result<(), String> {
                check(startup_status)?;
                check(GdipCreateBitmapFromScan0(
                    width,
                    height,
                    width * 4,
                    0x000E200B, // PixelFormat32bppPARGB
                    Some(pixels as *const u8),
                    &mut image,
                ))?;
                check(GdipGetImageGraphicsContext(
                    image as *mut GpImage,
                    &mut graphics,
                ))?;
                check(GdipGraphicsClear(graphics, 0))?;

                let artwork = image::load_from_memory_with_format(ARTWORK, image::ImageFormat::Png)
                    .map_err(|error| format!("Could not load startup artwork: {error}"))?
                    .to_rgba8();
                let (artwork_width, artwork_height) = artwork.dimensions();
                artwork_buffer = Some(artwork);
                let artwork = artwork_buffer.as_ref().unwrap();
                check(GdipCreateBitmapFromScan0(
                    artwork_width as i32,
                    artwork_height as i32,
                    artwork_width as i32 * 4,
                    0x0026200A, // PixelFormat32bppARGB
                    Some(artwork.as_raw().as_ptr()),
                    &mut icon,
                ))?;
                check(GdipSetInterpolationMode(
                    graphics,
                    InterpolationModeHighQualityBicubic,
                ))?;
                let size = (160.0 * scale) as i32;
                check(GdipDrawImageRectI(
                    graphics,
                    icon as *mut GpImage,
                    (width - size) / 2,
                    4,
                    size,
                    size,
                ))?;

                check(GdipCreateFontFamilyFromName(
                    w!("Segoe UI"),
                    ptr::null_mut(),
                    &mut family,
                ))?;
                check(GdipCreateFont(
                    family,
                    17.0 * scale,
                    FontStyleBold.0,
                    UnitPixel,
                    &mut font,
                ))?;
                check(GdipCreateSolidFill(0xFFF5F7F4, &mut brush))?;
                check(GdipCreateSolidFill(0xD0000000, &mut shadow))?;
                check(GdipCreateStringFormat(0, 0, &mut format))?;
                check(GdipSetStringFormatAlign(format, StringAlignmentCenter))?;
                check(GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAlias))?;

                let status_wide: Vec<u16> = status.encode_utf16().chain(std::iter::once(0)).collect();
                let status = PCWSTR(status_wide.as_ptr());
                let mut text = RectF {
                    X: 1.0,
                    Y: 177.0 * scale + 1.0,
                    Width: width as f32,
                    Height: 34.0 * scale,
                };
                check(GdipDrawString(
                    graphics,
                    status,
                    -1,
                    font,
                    &text,
                    format,
                    shadow as *const GpBrush,
                ))?;
                text.X = 0.0;
                text.Y -= 1.0;
                check(GdipDrawString(
                    graphics,
                    status,
                    -1,
                    font,
                    &text,
                    format,
                    brush as *const GpBrush,
                ))?;
                check(GdipFlush(graphics, FlushIntentionSync))?;

                let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
                let mut monitor_info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
                    return Err("GetMonitorInfo failed".into());
                }
                let position = POINT {
                    x: (monitor_info.rcWork.left + monitor_info.rcWork.right - width) / 2,
                    y: (monitor_info.rcWork.top + monitor_info.rcWork.bottom - height) / 2,
                };
                let dimensions = SIZE {
                    cx: width,
                    cy: height,
                };
                let origin = POINT { x: 0, y: 0 };
                let blend = BLENDFUNCTION {
                    BlendOp: 0,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: 1, // AC_SRC_ALPHA
                };
                UpdateLayeredWindow(
                    self.window,
                    Some(screen),
                    Some(&position),
                    Some(&dimensions),
                    Some(dc),
                    Some(&origin),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )
                .map_err(|error| format!("UpdateLayeredWindow failed ({error})"))?;
                let _ = ShowWindow(self.window, SW_SHOWNOACTIVATE);
                Ok(())
            })();

            if !icon.is_null() {
                GdipDisposeImage(icon as *mut GpImage);
            }
            if !format.is_null() {
                GdipDeleteStringFormat(format);
            }
            if !shadow.is_null() {
                GdipDeleteBrush(shadow as *mut GpBrush);
            }
            if !brush.is_null() {
                GdipDeleteBrush(brush as *mut GpBrush);
            }
            if !font.is_null() {
                GdipDeleteFont(font);
            }
            if !family.is_null() {
                GdipDeleteFontFamily(family);
            }
            if !graphics.is_null() {
                GdipDeleteGraphics(graphics);
            }
            if !image.is_null() {
                GdipDisposeImage(image as *mut GpImage);
            }
            drop(artwork_buffer);
            if token != 0 {
                GdiplusShutdown(token);
            }
            let _ = SelectObject(dc, previous);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);

            result
        }
    }
}

impl Drop for Splash {
    fn drop(&mut self) {
        if !self.window.0.is_null() {
            let _ = unsafe { DestroyWindow(self.window) };
            self.window = HWND(ptr::null_mut());
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(window, message, wparam, lparam)
}

fn check(status: Status) -> Result<(), String> {
    if status.0 != 0 {
        Err(format!("Startup artwork rendering failed ({}).", status.0))
    } else {
        Ok(())
    }
}
