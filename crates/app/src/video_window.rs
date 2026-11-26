use crate::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{CloseRequestResponse, ComponentHandle};
use std::path::PathBuf;

pub(crate) struct VideoHost {
    pub(crate) window: VideoWindow,
    pub(crate) surface_hwnd: isize,
    pub(crate) repeat_current: bool,
    pub(crate) owner_closing: bool,
}

impl VideoHost {
    pub(crate) fn create() -> Result<Self, String> {
        let window = VideoWindow::new().map_err(|e| format!("Failed to create video window: {e}"))?;
        let parent_hwnd = window
            .window()
            .window_handle()
            .window_handle()
            .ok()
            .and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
                _ => None,
            })
            .unwrap_or(0);
        let surface_hwnd = create_child_surface(parent_hwnd);
        Ok(Self {
            window,
            surface_hwnd,
            repeat_current: false,
            owner_closing: false,
        })
    }

    pub(crate) fn sync_state(&self, is_playing: bool, progress: f32, cur: &str, total: &str, vol: u32, speed: f32) {
        self.window.set_is_playing(is_playing);
        self.window.set_progress(progress);
        self.window.set_current_time(ss(cur));
        self.window.set_total_time(ss(total));
        self.window.set_volume(vol as i32);
        self.window.set_speed(ss(format_speed(speed)));
        self.window.set_repeat_current(self.repeat_current);
    }

    pub(crate) fn set_track(&self, track: &Track, speed: f32) {
        let artist = if track.artist.trim().is_empty() {
            "Unknown artist"
        } else {
            track.artist.as_str()
        };
        let title = format!("{} — {}", track.title, artist);
        self.window.set_video_title(ss(title));
        self.window.set_speed(ss(format_speed(speed)));
        self.window.set_repeat_current(self.repeat_current);
    }

    pub(crate) fn resize_surface(&self, logical_w: i32, logical_h: i32) {
        if self.surface_hwnd == 0 {
            return;
        }
        let scale = self.window.window().scale_factor().max(1.0);
        let phys_w = ((logical_w.max(1) as f32) * scale).round() as i32;
        let phys_h = ((logical_h.max(1) as f32) * scale).round() as i32;
        position_child_surface(self.surface_hwnd, phys_w, phys_h);
    }

    pub(crate) fn close_from_owner(&mut self) {
        self.owner_closing = true;
        let _ = self.window.hide();
        destroy_child_surface(self.surface_hwnd);
        self.surface_hwnd = 0;
    }
}

impl Drop for VideoHost {
    fn drop(&mut self) {
        destroy_child_surface(self.surface_hwnd);
        self.surface_hwnd = 0;
    }
}

fn format_speed(rate: f32) -> &'static str {
    if (rate - 0.5).abs() < 0.05 {
        "0.5x"
    } else if (rate - 0.75).abs() < 0.05 {
        "0.75x"
    } else if (rate - 1.25).abs() < 0.05 {
        "1.25x"
    } else if (rate - 1.5).abs() < 0.05 {
        "1.5x"
    } else if (rate - 2.0).abs() < 0.05 {
        "2x"
    } else {
        "1x"
    }
}

fn parse_speed(label: &str) -> f32 {
    label.trim().trim_end_matches('x').parse::<f32>().unwrap_or(1.0)
}

pub(crate) fn screenshots_folder() -> PathBuf {
    if let Some(dirs) = directories::UserDirs::new() {
        if let Some(pics) = dirs.picture_dir() {
            return pics.join("Music Player").join("Screenshots");
        }
        return dirs.home_dir().join("Pictures").join("Music Player").join("Screenshots");
    }
    PathBuf::from("Screenshots")
}

pub(crate) fn sanitized_screenshot_stem(track_path: &str) -> String {
    let raw = Path::new(track_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let mut clean: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    clean = clean.trim().trim_end_matches('.').to_string();
    if clean.len() > 80 {
        clean.truncate(80);
    }
    if clean.is_empty() {
        "video".to_string()
    } else {
        clean
    }
}

#[cfg(windows)]
fn create_child_surface(parent_hwnd: isize) -> isize {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
    };
    if parent_hwnd == 0 {
        return 0;
    }
    const SS_BLACKRECT: u32 = 0x0000_0004;
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let title: Vec<u16> = "\0".encode_utf16().collect();
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | SS_BLACKRECT,
            0,
            0,
            640,
            360,
            parent_hwnd as HWND,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    hwnd as isize
}

#[cfg(not(windows))]
fn create_child_surface(_parent_hwnd: isize) -> isize {
    0
}

#[cfg(windows)]
fn position_child_surface(surface_hwnd: isize, width: i32, height: i32) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_SHOWWINDOW,
    };
    if surface_hwnd == 0 {
        return;
    }
    unsafe {
        let _ = SetWindowPos(
            surface_hwnd as HWND,
            HWND_TOP,
            0,
            0,
            width.max(1),
            height.max(1),
            SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

#[cfg(not(windows))]
fn position_child_surface(_surface_hwnd: isize, _width: i32, _height: i32) {}

#[cfg(windows)]
fn destroy_child_surface(surface_hwnd: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;
    if surface_hwnd != 0 {
        unsafe {
            let _ = DestroyWindow(surface_hwnd as HWND);
        }
    }
}

#[cfg(not(windows))]
fn destroy_child_surface(_surface_hwnd: isize) {}

impl App {
    pub(crate) fn ensure_video_window(&mut self, track: &Track) -> isize {
        if let Some(ref host) = self.video_host {
            let _ = host.window.show();
            host.set_track(track, self.engine.video_rate());
            return host.surface_hwnd;
        }
        let Ok(host) = VideoHost::create() else {
            self.show_notice(
                "Music Player could not open the video window. See the local log for details.",
                NoticeSeverity::Error,
            );
            return 0;
        };
        let win = host.window.clone_strong();
        win.on_toggle_play(|| {
            crate::with_app(|app| {
                app.toggle_play_pause();
            });
        });
        win.on_previous(|| {
            crate::with_app(|app| {
                app.previous_track();
            });
        });
        win.on_next(|| {
            crate::with_app(|app| {
                app.advance_queue(false);
            });
        });
        win.on_seek_fraction(|frac| {
            crate::with_app(|app| {
                let dur = app.engine.duration_ms();
                if dur > 0 {
                    app.cancel_crossfade();
                    let _ = app.engine.seek(((dur as f64) * (frac as f64)).round() as u64);
                    app.publish_playback();
                }
            });
        });
        win.on_seek_relative(|secs| {
            crate::with_app(|app| {
                let dur = app.engine.duration_ms();
                let cur = app.engine.position_ms();
                let next = (cur as i64 + (secs as i64) * 1000).clamp(0, dur as i64) as u64;
                let _ = app.engine.seek(next);
                app.publish_playback();
            });
        });
        win.on_volume_changed(|vol| {
            crate::with_app(|app| {
                app.set_volume(vol.clamp(0, 100) as u32);
            });
        });
        win.on_speed_selected(|label| {
            crate::with_app(|app| {
                let rate = parse_speed(&label);
                if app.engine.set_video_rate(rate).is_err() {
                    app.show_notice(
                        "The media engine rejected the requested video playback speed.",
                        NoticeSeverity::Warning,
                    );
                }
            });
        });
        win.on_toggle_repeat(|| {
            crate::with_app(|app| {
                if let Some(ref mut vhost) = app.video_host {
                    vhost.repeat_current = !vhost.repeat_current;
                    vhost.window.set_repeat_current(vhost.repeat_current);
                }
            });
        });
        win.on_open_subtitles(|| {
            crate::with_app(|app| {
                app.refresh_video_subtitles_popup();
            });
        });
        win.on_select_subtitle(|id| {
            crate::with_app(|app| {
                if !app.engine.set_video_subtitle(id) {
                    app.show_notice(
                        "The media engine could not change the subtitle track.",
                        NoticeSeverity::Warning,
                    );
                }
                if let Some(ref vhost) = app.video_host {
                    vhost.window.set_selected_subtitle_id(id);
                }
            });
        });
        win.on_take_snapshot(|| {
            crate::with_app(|app| {
                app.save_video_frame_screenshot();
            });
        });
        win.on_toggle_fullscreen(|| {
            crate::with_app(|app| {
                if let Some(ref vhost) = app.video_host {
                    let next = !vhost.window.get_is_fullscreen();
                    vhost.window.set_is_fullscreen(next);
                }
            });
        });
        win.on_exit_fullscreen(|| {
            crate::with_app(|app| {
                if let Some(ref vhost) = app.video_host {
                    vhost.window.set_is_fullscreen(false);
                }
            });
        });
        win.on_surface_resized(|w, h| {
            crate::with_app(|app| {
                if let Some(ref vhost) = app.video_host {
                    vhost.resize_surface(w, h);
                }
            });
        });
        win.window().on_close_requested(|| {
            crate::with_app(|app| {
                app.handle_video_window_closed_by_user();
            });
            CloseRequestResponse::HideWindow
        });

        let surface_hwnd = host.surface_hwnd;
        host.set_track(track, self.engine.video_rate());
        let _ = host.window.show();
        host.resize_surface(960, 524);
        self.video_host = Some(host);
        self.sync_video_window();
        surface_hwnd
    }

    pub(crate) fn handle_video_window_closed_by_user(&mut self) {
        let Some(mut host) = self.video_host.take() else {
            return;
        };
        let owner_closing = host.owner_closing;
        if !owner_closing && self.engine.is_video_mode() && self.engine.is_playing() {
            let current = self.current.clone();
            let result = self.coordinator.pause(current.as_ref());
            self.apply_result(result);
        }
        self.engine.set_video_surface_handle(0);
        host.close_from_owner();
    }

    pub(crate) fn close_video_window(&mut self, pause_video: bool) {
        let Some(mut host) = self.video_host.take() else {
            return;
        };
        if pause_video && self.engine.is_video_mode() && self.engine.is_playing() {
            let current = self.current.clone();
            let result = self.coordinator.pause(current.as_ref());
            self.apply_result(result);
        }
        self.engine.set_video_surface_handle(0);
        host.close_from_owner();
    }

    pub(crate) fn sync_video_window(&self) {
        let Some(ref host) = self.video_host else {
            return;
        };
        let dur = self.engine.duration_ms();
        let pos = self.engine.position_ms();
        let progress = if dur > 0 {
            (pos as f32 / dur as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let cur_str = fmt_time(pos);
        let tot_str = fmt_time(dur);
        host.sync_state(
            self.engine.is_playing(),
            progress,
            &cur_str,
            &tot_str,
            self.volume,
            self.engine.video_rate(),
        );
    }

    fn refresh_video_subtitles_popup(&mut self) {
        let selected_id = self.engine.video_subtitle_id();
        let subs: Vec<VideoSubtitleItem> = self
            .engine
            .video_subtitles()
            .into_iter()
            .map(|t| VideoSubtitleItem {
                id: t.id,
                name: ss(t.name),
                selected: t.id == selected_id,
            })
            .collect();
        if let Some(ref host) = self.video_host {
            host.window.set_selected_subtitle_id(selected_id);
            host.window.set_has_subtitles(!subs.is_empty());
            host.window.set_subtitles(ui_model(subs).into());
            host.window
                .set_subtitle_menu_open(!host.window.get_subtitle_menu_open());
        }
    }

    fn save_video_frame_screenshot(&mut self) {
        let folder = screenshots_folder();
        if let Err(err) = std::fs::create_dir_all(&folder) {
            LocalAppLog::shared().warning("video-screenshot", &format!("Could not create folder: {err}"));
            self.show_notice(
                "Could not save the video screenshot. Check the Pictures folder permissions.",
                NoticeSeverity::Warning,
            );
            return;
        }
        let stem = self
            .current
            .as_ref()
            .map(|t| sanitized_screenshot_stem(&t.path))
            .unwrap_or_else(|| "video".to_string());
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
        let file_name_str = format!("{stem}-{stamp}.png");
        let dest = folder.join(&file_name_str);
        if self.engine.take_video_snapshot(&dest) {
            self.show_notice(
                format!("Screenshot saved in Pictures\\Music Player\\Screenshots: {file_name_str}"),
                NoticeSeverity::Informational,
            );
        } else {
            self.show_notice(
                "The media engine could not capture the current video frame.",
                NoticeSeverity::Warning,
            );
        }
    }
}
