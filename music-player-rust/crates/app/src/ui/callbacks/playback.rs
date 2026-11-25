use crate::*;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(ui: &AppWindow, app: Rc<RefCell<App>>) {
    // Playback.
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_track(move |id| {
            let mut app = a.borrow_mut();
            if let Some(track) = app.track_by_id(&id) {
                let paths = app.context_paths_for_track_play(&track);
                let index = paths
                    .iter()
                    .position(|p| p.eq_ignore_ascii_case(&track.path));
                app.coordinator.queue_mut().replace(paths, index);
                app.play_track(track, false, index);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_view(move |view| {
            let mut app = a.borrow_mut();
            app.set_view(&view, "", "", "");
            let paths = app.current_view_all_paths(Some(&view));
            if let Some(path) = paths.first().cloned() {
                if let Some(track) = app.store.get_track(&path).ok().flatten() {
                    app.coordinator.queue_mut().replace(paths, Some(0));
                    app.play_track(track, false, Some(0));
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_shuffle_view(move |view| {
            let mut app = a.borrow_mut();
            app.set_view(&view, "", "", "");
            let paths = app.current_view_all_paths(Some(&view));
            if !paths.is_empty() {
                app.coordinator.queue_mut().replace(paths, Some(0));
                app.coordinator.queue_mut().set_shuffle(true);
                app.coordinator.queue_mut().shuffle_upcoming(0);
                if let Some(path) = app.coordinator.queue().entries().first().cloned() {
                    if let Some(track) = app.store.get_track(&path).ok().flatten() {
                        app.play_track(track, false, Some(0));
                    }
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_group(move |column, value| {
            let mut app = a.borrow_mut();
            let query = music_player_core::TrackQuery {
                group_column: Some(column.to_string()),
                group_value: Some(value.to_string()),
                sort: if column == "album" {
                    music_player_core::TrackSort::TrackNumber
                } else {
                    music_player_core::TrackSort::Title
                },
                page_size: 10_000,
                ..Default::default()
            };
            if let Ok((tracks, _)) = app.store.query_track_page(&query) {
                if !tracks.is_empty() {
                    let paths: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
                    app.coordinator.queue_mut().replace(paths, Some(0));
                    let first = tracks[0].clone();
                    app.play_track(first, false, Some(0));
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_group(move |column, value| {
            let view = match column.as_str() {
                "album" => "Album",
                "artist" => "Artist",
                "genre" => "Genre",
                _ => "Folder",
            };
            a.borrow_mut().set_view(view, "", &value, "");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_play_group_shuffle(move |column, value| {
                let mut app = a.borrow_mut();
                let query = music_player_core::TrackQuery {
                    group_column: Some(column.to_string()),
                    group_value: Some(value.to_string()),
                    sort: music_player_core::TrackSort::Title,
                    page_size: 10_000,
                    ..Default::default()
                };
                if let Ok((tracks, _)) = app.store.query_track_page(&query) {
                    if !tracks.is_empty() {
                        let paths: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
                        app.coordinator.queue_mut().replace(paths, Some(0));
                        app.coordinator.queue_mut().set_shuffle(true);
                        app.coordinator.queue_mut().shuffle_upcoming(0);
                        if let Some(path) = app.coordinator.queue().entries().first().cloned() {
                            if let Some(track) = app.store.get_track(&path).ok().flatten() {
                                app.play_track(track, false, Some(0));
                            }
                        }
                    }
                }
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_sort_view(move |column| {
            let mut app = a.borrow_mut();
            let state = app.ui.global::<State>();
            let current = state.get_view_sort().to_string();
            let desc = if column == current {
                !state.get_view_descending()
            } else {
                false
            };
            state.set_view_sort(ss(&column));
            state.set_view_descending(desc);
            drop(state);
            app.tracks_offset = 0;
            app.refresh_view();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_play_playlist(move |id| a.borrow_mut().play_playlist(&id, false));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_play_playlist_shuffle(move |id| a.borrow_mut().play_playlist(&id, true));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_queue_entry(move |index| {
            let mut app = a.borrow_mut();
            let path = app
                .coordinator
                .queue()
                .entries()
                .get(index as usize)
                .cloned();
            if let Some(path) = path {
                if let Some(_track) = app.store.get_track(&path).ok().flatten() {
                    let resolver = app.resolver();
                    let result = app.coordinator.play_queue_entry(index as usize, &resolver);
                    app.apply_result(result);
                } else {
                    let notice = Notice::warning("That queued file is no longer available.");
                    app.apply_result(PlaybackCommandResult {
                        kind: PlaybackCommandKind::TrackUnavailable,
                        notice: Some(notice),
                        ..PlaybackCommandResult::no_change()
                    });
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_toggle_play(move || a.borrow_mut().toggle_play_pause());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_pause(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            let current = app.current.clone();
            let result = app.coordinator.pause(current.as_ref());
            app.apply_result(result);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_resume(move || {
            let mut app = a.borrow_mut();
            let current = app.current.clone();
            let result = app.coordinator.resume(current.as_ref());
            app.apply_result(result);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_stop(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            app.close_video_window(false);
            let _ = app.engine.stop();
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_next(move || a.borrow_mut().advance_queue(false));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_previous(move || a.borrow_mut().previous_track());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_seek_fraction(move |fraction| {
            let mut app = a.borrow_mut();
            app.seek_wheel_timer.stop();
            app.seek_preview_ms = None;
            app.cancel_crossfade();
            let duration = app.engine.duration_ms();
            let target = (duration as f64 * fraction as f64).max(0.0) as u64;
            let _ = app.engine.seek(target);
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_preview_seek_fraction(move |fraction| {
                let mut app = a.borrow_mut();
                app.seek_wheel_timer.stop();
                let duration = app.engine.duration_ms();
                if duration == 0 || app.current.is_none() {
                    return;
                }
                let target = (duration as f64 * fraction.clamp(0.0, 1.0) as f64) as u64;
                app.apply_seek_preview(target, duration);
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_seek_wheel(move |delta_seconds| {
            let mut app = a.borrow_mut();
            let duration = app.engine.duration_ms();
            if duration == 0 || app.current.is_none() {
                return;
            }
            let current = app
                .seek_preview_ms
                .unwrap_or_else(|| app.engine.position_ms());
            let target = ((current as i64) + i64::from(delta_seconds) * 1000)
                .clamp(0, duration as i64) as u64;
            app.apply_seek_preview(target, duration);

            let callback_app = a.clone();
            app.seek_wheel_timer.start(
                TimerMode::SingleShot,
                Duration::from_millis(100),
                move || {
                    let mut app = callback_app.borrow_mut();
                    let Some(target) = app.seek_preview_ms.take() else {
                        return;
                    };
                    app.cancel_crossfade();
                    let _ = app.engine.seek(target);
                    app.publish_playback();
                },
            );
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_seek_relative(move |delta_seconds| {
                let mut app = a.borrow_mut();
                app.seek_wheel_timer.stop();
                app.seek_preview_ms = None;
                app.cancel_crossfade();
                let duration = app.engine.duration_ms();
                let current = app.engine.position_ms();
                let target = ((current as i64) + (delta_seconds as i64 * 1000))
                    .clamp(0, duration as i64) as u64;
                let _ = app.engine.seek(target);
                app.publish_playback();
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_volume_changed(move |value| {
            a.borrow_mut().set_volume(value.clamp(0, 100) as u32);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_volume_muted(move || {
            let mut app = a.borrow_mut();
            if app.volume > 0 {
                app.unmuted_volume = Some(app.volume);
                app.volume = 0;
            } else {
                app.volume = app.unmuted_volume.take().unwrap_or(75).clamp(1, 100);
            }
            let vol = app.volume;
            let _ = app.engine.set_volume(vol);
            app.pending_volume = Some(app.volume);
            app.last_vol_debounce = Instant::now();
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_shuffle(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            let paths = app.view_paths.clone();
            let current = app.current.as_ref().map(|t| t.path.clone());
            app.coordinator
                .queue_mut()
                .toggle_shuffle(&paths, current.as_deref());
            app.publish_queue(true);
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_cycle_repeat(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            app.coordinator.queue_mut().cycle_repeat_mode();
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_remaining_time(move || {
            let mut app = a.borrow_mut();
            let current = app
                .settings
                .bool_or(music_player_core::settings::keys::SHOW_REMAINING_TIME, false);
            let next = !current;
            app.settings.set_raw(
                music_player_core::settings::keys::SHOW_REMAINING_TIME,
                if next { "true" } else { "false" },
            );
            app.ui.global::<State>().set_show_remaining_time(next);
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_ab(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            let position = app.engine.position_ms();
            let queue = app.coordinator.queue_mut();
            queue.toggle_ab_repeat_point(position);
            let (a_ms, b_ms) = (queue.repeat_a_ms(), queue.repeat_b_ms());

            let state = app.ui.global::<State>();
            state.set_repeat_a(ss(a_ms.map(fmt_time).unwrap_or_default()));
            state.set_repeat_b(ss(b_ms.map(fmt_time).unwrap_or_default()));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_toggle_favorite(move |id| a.borrow_mut().toggle_favorite(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_rating(move |id, rating| {
            let mut app = a.borrow_mut();
            app.context_target = Some(id.to_string());
            app.rate_track(rating);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_autoplay_toggle(move |enabled| {
            a.borrow_mut().settings.set_raw(
                music_player_core::settings::keys::AUTOPLAY_SIMILAR,
                &enabled.to_string(),
            );
        });
    }

    // Queue.
    ui.global::<Actions>().on_queue_drag_data(|index| {
        slint::DataTransfer::from(crate::ui::mapper::ss(index.to_string()))
    });
    ui.global::<Actions>().on_queue_drag_index(|data| {
        data.plain_text()
            .ok()
            .and_then(|value| value.to_string().parse::<i32>().ok())
            .unwrap_or(-1)
    });
    {
        let a = app.clone();
        ui.global::<Actions>().on_queue_move(move |index, target| {
            let mut app = a.borrow_mut();
            if app
                .coordinator
                .queue_mut()
                .move_entry_to(index as usize, target as usize)
            {
                app.publish_queue(true);
                app.save_session();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_queue_remove(move |index| {
            let mut app = a.borrow_mut();
            if app.coordinator.queue().current_index() == Some(index as usize) {
                return; // C# removeQueue refuses the current entry.
            }
            if app.coordinator.queue_mut().remove_entry(index as usize) {
                app.publish_queue(true);
                app.save_session();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_clear_queue(move || a.borrow_mut().clear_queue());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more_queue(move || {
            let mut app = a.borrow_mut();
            app.queue_display_limit = app.queue_display_limit.saturating_add(200);
            app.publish_queue(true);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_add_to_queue(move |id| {
            let mut app = a.borrow_mut();
            if let Some(track) = app.track_by_id(&id) {
                let current = app.current.as_ref().map(|t| t.path.clone());
                app.coordinator
                    .queue_mut()
                    .append_one(track.path, current.as_deref());
                app.publish_queue(true);
                app.save_session();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_next_in_queue(move |id| {
            let mut app = a.borrow_mut();
            if let Some(track) = app.track_by_id(&id) {
                let current = app.current.as_ref().map(|t| t.path.clone());
                app.coordinator
                    .queue_mut()
                    .insert_next(track.path, current.as_deref());
                app.publish_queue(true);
                app.save_session();
            }
        });
    }

    // Audio / EQ.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_eq_band(move |index, value| a.borrow_mut().set_eq_band(index as usize, value));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_eq_preamp(move |value| a.borrow_mut().set_eq_preamp(value));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_eq_preset(move |name| a.borrow_mut().set_eq_preset(&name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_eq_preset(move |name| {
            let mut app = a.borrow_mut();
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return;
            }
            app.settings
                .save_eq_preset(trimmed, app.eq_bands, app.eq_preamp);
            app.settings
                .set_eq_state(trimmed, app.eq_bands, app.eq_preamp);
            app.eq_preset = trimmed.to_string();
            app.eq_dirty = false;
            app.close_modal();
            app.publish_eq();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_output_device(move |name| {
            let mut app = a.borrow_mut();
            let mut id = app
                .audio_devices
                .iter()
                .find(|device| device.name == name.as_str())
                .map(|device| device.id.clone())
                .unwrap_or_default();
            let mut selected_name = if name == "System default" {
                "System default".to_string()
            } else {
                name.to_string()
            };
            app.cancel_crossfade();
            let direct_sound = if id.is_empty() {
                String::new()
            } else {
                os_shell::direct_sound_device_id(&id).unwrap_or_default()
            };
            if !id.is_empty() && direct_sound.is_empty() {
                // The endpoint could not be mapped; keep the system default,
                // matching SelectAudioOutputDevice's fallback.
                id.clear();
                selected_name = "System default".into();
            }
            if let Err(error) = app.engine.set_audio_output_device(&direct_sound) {
                music_player_core::LocalAppLog::shared().warning(
                    "audio-devices",
                    &format!("Could not change the audio output device ({error})."),
                );
            }
            app.settings
                .set_raw(music_player_core::settings::keys::AUDIO_OUTPUT_DEVICE, &id);
            app.settings.set_raw(
                music_player_core::settings::keys::AUDIO_OUTPUT_NAME,
                &selected_name,
            );
            app.ui
                .global::<State>()
                .set_output_device(ss(&selected_name));
            app.show_notice(
                format!("Audio output: {}.", selected_name),
                NoticeSeverity::Informational,
            );
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_refresh_audio_devices(move || {
            a.borrow_mut().publish_eq();
        });
    }
}
