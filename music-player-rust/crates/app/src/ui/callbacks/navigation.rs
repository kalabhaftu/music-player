use crate::*;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(ui: &AppWindow, app: Rc<RefCell<App>>) {
    // Navigation.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_view(move |view, search, group, playlist_id| {
                let mut app = a.borrow_mut();
                if view == "Search" && search.is_empty() {
                    app.ui.global::<State>().set_search_filter(ss("all"));
                }
                app.set_view(&view, &search, &group, &playlist_id);
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_go_back(move || a.borrow_mut().go_back());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_go_forward(move || a.borrow_mut().go_forward());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_window_resized(move |w, h| {
            let app = a.borrow();
            let _ = app.store.set_setting("window-width", &w.to_string());
            let _ = app.store.set_setting("window-height", &h.to_string());
            let hwnd = app
                .ui
                .window()
                .window_handle()
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                    _ => None,
                })
                .unwrap_or(0);
            native_io::save_window_placement(hwnd, &app.store);
            if app.os_attached {
                use slint::winit_030::WinitWindowAccessor;
                app.ui.window().with_winit_window(|win| {
                    app.ui.global::<State>().set_window_maximized(win.is_maximized());
                });
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_begin_search(move || a.borrow_mut().begin_search());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_search(move |query, filter| {
            let mut app = a.borrow_mut();
            let filter = if matches!(filter.as_str(), "song" | "album" | "artist" | "playlist") {
                filter
            } else {
                "all".into()
            };
            app.ui.global::<State>().set_search_filter(ss(&filter));
            app.set_view("Search", &query, "", "");
            app.save_recent_search(&query);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_search_page(move |kind, direction| {
                let mut app = a.borrow_mut();
                let state = app.ui.global::<State>();
                let (offset, total, page_size) = match kind.as_str() {
                    "song" => (
                        state.get_search_song_offset(),
                        state.get_search_song_count(),
                        40,
                    ),
                    "album" => (
                        state.get_search_album_offset(),
                        state.get_search_album_count(),
                        24,
                    ),
                    "artist" => (
                        state.get_search_artist_offset(),
                        state.get_search_artist_count(),
                        24,
                    ),
                    "playlist" => (
                        state.get_search_playlist_offset(),
                        state.get_search_playlist_count(),
                        20,
                    ),
                    _ => return,
                };
                let pages = (total.max(0) + page_size - 1) / page_size;
                let current_page = offset.max(0) / page_size;
                let next_page = (current_page + direction).clamp(0, pages.saturating_sub(1));
                let next_offset = next_page * page_size;
                match kind.as_str() {
                    "song" => state.set_search_song_offset(next_offset),
                    "album" => state.set_search_album_offset(next_offset),
                    "artist" => state.set_search_artist_offset(next_offset),
                    "playlist" => state.set_search_playlist_offset(next_offset),
                    _ => {}
                }
                drop(state);
                app.refresh_view();
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_search_input(move |query| {
            let mut app = a.borrow_mut();
            app.ui.global::<State>().set_view_search(ss(&query));
            let current = app.ui.global::<State>().get_view().to_string();
            if current == "Settings" {
                app.search_debounce = None;
                let words = query.to_lowercase();
                let matches = |terms: &[&str]| terms.iter().any(|term| words.contains(term));
                let section = if matches(&[
                    "update",
                    "release",
                    "version",
                    "license",
                    "privacy",
                    "repository",
                ]) {
                    Some("About")
                } else if matches(&[
                    "sidebar",
                    "navigation",
                    "tray",
                    "logs",
                    "diagnostic",
                    "default app",
                    "export",
                ]) {
                    Some("Advanced")
                } else if matches(&[
                    "folder",
                    "library",
                    "scan",
                    "index",
                    "duplicate",
                    "column",
                    "artwork",
                    "artist",
                    "album size",
                ]) {
                    Some("Library")
                } else if matches(&[
                    "playback",
                    "autoplay",
                    "lyrics",
                    "seek",
                    "queue",
                    "crossfade",
                    "music output",
                    "equalizer",
                    "volume",
                ]) {
                    Some("Playback")
                } else if matches(&[
                    "theme",
                    "appearance",
                    "accent",
                    "color",
                    "colour",
                    "transparen",
                    "motion",
                    "selection",
                ]) {
                    Some("Appearance")
                } else {
                    None
                };
                if let Some(section) = section {
                    app.ui.global::<State>().set_settings_section(ss(section));
                }
                return;
            }
            if current != "Search" {
                let state = app.ui.global::<State>();
                state.set_search_filter(ss("all"));
                state.set_view(ss("Search"));
            }
            app.search_debounce = Some((
                Instant::now() + Duration::from_millis(150),
                query.to_string(),
            ));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_recent_search(move |term| {
            a.borrow_mut().save_recent_search(&term);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_clear_recent_searches(move || a.borrow_mut().clear_recent_searches());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_remove_recent_search(move |term| a.borrow_mut().remove_recent_search(&term));
    }
}
