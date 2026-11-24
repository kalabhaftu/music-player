use crate::*;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(ui: &AppWindow, app: Rc<RefCell<App>>) {
    // Settings.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_setting(move |name, value| a.borrow_mut().set_setting(&name, &value));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_move_navigation(move |view, delta| a.borrow_mut().move_navigation(&view, delta));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_toggle_navigation(move |view| a.borrow_mut().toggle_navigation(&view));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_export_logs(move || {
            let Some(path) = save_file("music-player-logs.zip", "ZIP log archive (*.zip)|*.zip")
            else {
                return;
            };
            let result = LocalAppLog::shared().export_to(Path::new(&path));
            let mut app = a.borrow_mut();
            match result {
                Ok(()) => app.show_notice(
                    "Logs exported. Review the ZIP for local paths before sharing.",
                    NoticeSeverity::Informational,
                ),
                Err(error) => app.show_notice(
                    format!("Could not export logs: {error}"),
                    NoticeSeverity::Error,
                ),
            }
        });
    }
    {
        ui.global::<Actions>().on_open_default_apps(move || {
            let _ = crate::os_associations::register_associations();
            open_url("ms-settings:defaultapps");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_check_updates(move || a.borrow_mut().check_updates_manual());
    }
    {
        ui.global::<Actions>().on_open_url(move |url| {
            let allowed = [
                "https://github.com/kalabhaftu/music-player",
                "https://github.com/kalabhaftu/music-player/releases",
                "https://github.com/kalabhaftu/music-player/blob/main/LICENSE",
                "https://github.com/kalabhaftu/music-player/blob/main/ThirdPartyNotices.md",
                "https://github.com/kalabhaftu/music-player/blob/main/LGPL-2.1.txt",
            ];
            if allowed.contains(&url.as_str()) {
                open_url(&url);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_accent_preset(move |name| {
            let color = match name.as_str() {
                "Sky" => "#59d7ff",
                "Violet" => "#a878ff",
                "Rose" => "#ff6e91",
                "Amber" => "#ffb04a",
                _ => "#b7ff2d",
            };
            let mut app = a.borrow_mut();
            app.settings
                .set_raw(music_player_core::settings::keys::ACCENT_COLOR, color);
            app.settings
                .set_raw(music_player_core::settings::keys::ACCENT_MANUAL, "true");
            app.publish_settings();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_crossfade_label(move |label| a.borrow_mut().set_crossfade_label(&label));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_reset_settings(move |groups| {
            a.borrow_mut()
                .settings
                .reset_groups(&groups.split(',').map(str::to_string).collect::<Vec<_>>());
            let mut app = a.borrow_mut();
            app.publish_settings();
            app.close_modal();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_settings(move || {
            let mut app = a.borrow_mut();
            app.set_view("Settings", "", "", "");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_pick_library_root(move || {
            if let Some(path) = pick_folder() {
                a.borrow_mut().add_root(&path);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_pick_ignored_directory(move || {
            if let Some(path) = pick_folder() {
                let mut app = a.borrow_mut();
                let mut dirs = app.settings.ignored_directories();
                if !dirs.contains(&path) {
                    dirs.push(path);
                }
                app.settings.set_ignored_directories(&dirs);
                app.publish_exclusions();
                let roots = app.settings.library_roots();
                app.reconfigure_watcher(&roots);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_extension_enabled(move |ext, enabled| {
                let mut app = a.borrow_mut();
                let mut map = app.settings.extension_enabled();
                map.insert(ext.to_lowercase(), enabled);
                app.settings.set_extension_enabled(&map);
                app.publish_exclusions();
                let roots = app.settings.library_roots();
                app.reconfigure_watcher(&roots);
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_add_extension(move || {
            let mut app = a.borrow_mut();
            let state = app.ui.global::<State>();
            let ext = state
                .get_exclusion_new_extension()
                .to_string()
                .trim()
                .to_lowercase();
            let kind = state.get_exclusion_new_kind().to_string();
            drop(state);
            let valid = ext.starts_with('.')
                && ext.len() >= 2
                && ext.len() <= 13
                && ext[1..]
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
            if !valid {
                app.show_notice(
                    "Use an extension like .mkv (letters and digits only).",
                    NoticeSeverity::Warning,
                );
                return;
            }
            let mut enabled = app.settings.extension_enabled();
            let mut kinds = app.settings.extension_types();
            enabled.insert(ext.clone(), true);
            kinds.insert(ext, kind);
            app.settings.set_extension_enabled(&enabled);
            app.settings.set_extension_types(&kinds);
            app.ui.global::<State>().set_exclusion_new_extension(ss(""));
            app.publish_exclusions();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_remove_ignored_directory(move |dir| {
                let mut app = a.borrow_mut();
                let dirs: Vec<String> = app
                    .settings
                    .ignored_directories()
                    .into_iter()
                    .filter(|d| d.as_str() != dir.as_str())
                    .collect();
                app.settings.set_ignored_directories(&dirs);
                app.publish_exclusions();
            });
    }

    // Window / overlays / misc.
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_window(move || {
            let mut app = a.borrow_mut();
            app.save_session();
            let _ = app.store.checkpoint_wal();
            let _ = app.ui.hide();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_minimize_window(move || {
            use slint::winit_030::WinitWindowAccessor;
            let app = a.borrow();
            if app.ui.window().with_winit_window(|win| win.set_minimized(true)).is_none() {
                app.ui.window().set_minimized(true);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_maximize_window(move || {
            use slint::winit_030::WinitWindowAccessor;
            let app = a.borrow();
            app.ui.window().with_winit_window(|win| {
                let next = !win.is_maximized();
                win.set_maximized(next);
                app.ui.global::<State>().set_window_maximized(next);
            });
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_drag_window(move || {
            use slint::winit_030::WinitWindowAccessor;
            let app = a.borrow();
            app.ui.window().with_winit_window(|win| {
                let _ = win.drag_window();
            });
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_drag_resize_window(move |dir| {
            use slint::winit_030::winit::window::ResizeDirection;
            use slint::winit_030::WinitWindowAccessor;
            let direction = match dir.as_str() {
                "n" => ResizeDirection::North,
                "s" => ResizeDirection::South,
                "e" => ResizeDirection::East,
                "w" => ResizeDirection::West,
                "nw" => ResizeDirection::NorthWest,
                "ne" => ResizeDirection::NorthEast,
                "sw" => ResizeDirection::SouthWest,
                "se" => ResizeDirection::SouthEast,
                _ => return,
            };
            let app = a.borrow();
            app.ui.window().with_winit_window(|win| {
                let _ = win.drag_resize_window(direction);
            });
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_immersive(move |kind| {
            let mut app = a.borrow_mut();
            let state = app.ui.global::<State>();
            let open = state.get_immersive_open();
            let same_view = state.get_immersive_kind() == kind;
            state.set_immersive_open(!(open && same_view));
            state.set_immersive_kind(ss(&kind));
            drop(state);
            app.publish_queue(true);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_immersive(move || {
            let mut app = a.borrow_mut();
            app.ui.global::<State>().set_immersive_open(false);
            app.publish_queue(true);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_right_panel(move || {
            let app = a.borrow_mut();
            let state = app.ui.global::<State>();
            let open = !state.get_right_panel_open();
            state.set_right_panel_open(open);
            drop(state);
            app.settings.set_raw(
                music_player_core::settings::keys::RIGHT_PANEL_OPEN,
                &open.to_string(),
            );
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_tools_menu(move || {
            let app = a.borrow();
            let state = app.ui.global::<State>();
            state.set_tools_menu_open(!state.get_tools_menu_open());
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_scan_details(move || {
            let app = a.borrow();
            let state = app.ui.global::<State>();
            state.set_scan_details_open(!state.get_scan_details_open());
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_sidebar(move || {
            let app = a.borrow();
            let state = app.ui.global::<State>();
            let collapsed = !state.get_navigation_collapsed();
            state.set_navigation_collapsed(collapsed);
            app.settings.set_raw(
                music_player_core::settings::keys::SIDEBAR_COLLAPSED,
                &collapsed.to_string(),
            );
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_immersive_player(move || {
            let app = a.borrow();
            let state = app.ui.global::<State>();
            state.set_immersive_player_open(true);
            state.set_immersive_lyrics_open(false);
            drop(state);
            app.sync_window_mode();
            app.publish_appearance();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_immersive_player(move || {
            use slint::winit_030::WinitWindowAccessor;
            let app = a.borrow();
            if app.ui.window().is_fullscreen() {
                app.ui.window().set_fullscreen(false);
                app.ui.window().with_winit_window(|win| win.set_fullscreen(None));
            }
            let state = app.ui.global::<State>();
            state.set_immersive_fullscreen(false);
            state.set_immersive_player_open(false);
            state.set_immersive_lyrics_open(false);
            drop(state);
            app.sync_window_mode();
            app.publish_appearance();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_toggle_immersive_fullscreen(move || {
                use slint::winit_030::WinitWindowAccessor;
                let app = a.borrow();
                let fullscreen = !app.ui.window().is_fullscreen();
                app.ui.window().set_fullscreen(fullscreen);
                if !fullscreen {
                    app.ui.window().with_winit_window(|win| win.set_fullscreen(None));
                }
                app.sync_window_mode();
                if !fullscreen {
                    app.publish_appearance();
                }
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_immersive_lyrics(move || {
            let mut app = a.borrow_mut();
            let state = app.ui.global::<State>();
            let open = !state.get_immersive_lyrics_open();
            state.set_immersive_lyrics_open(open);
            drop(state);
            if open {
                if let Some(track) = app.current.clone() {
                    app.load_lyrics(&track);
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_right_tab(move |tab| {
            a.borrow()
                .ui
                .global::<State>()
                .set_right_panel_tab(ss(&tab));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_ui_ready(move || a.borrow().ui.global::<State>().set_page_loading(true));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_notice(move || {
            a.borrow().ui.global::<State>().set_notice_visible(false);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_release(move || {
            let app = a.borrow();
            // The About row is the release history; the update dialog targets
            // the exact cached release. Both reject anything outside GitHub.
            let url = if app.ui.global::<State>().get_modal() == "update" {
                app.settings
                    .raw(music_player_core::settings::keys::UPDATE_RELEASE_URL)
                    .filter(|url| {
                        updates::is_release_page(url) || updates::is_repository_info_page(url)
                    })
                    .unwrap_or_else(|| updates::RELEASES_URL.to_string())
            } else {
                updates::RELEASES_URL.to_string()
            };
            drop(app);
            open_url(&url);
        });
    }
}
