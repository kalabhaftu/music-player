use crate::*;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(ui: &AppWindow, app: Rc<RefCell<App>>) {
    // Metadata / tags / files.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_details(move |id| a.borrow_mut().open_details(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_tags(move |id| a.borrow_mut().open_tags(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_save_tags(move || a.borrow_mut().save_tags());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_pick_artwork(move || {
            if let Some(path) = pick_file("Images (*.png;*.jpg;*.jpeg)|*.png;*.jpg;*.jpeg") {
                {
                    let app = a.borrow();
                    let state = app.ui.global::<State>();
                    state.set_tags_artwork_name(ss(file_name(&path)));
                    state.set_tags_has_artwork(true);
                }
                let app = a.borrow_mut();
                if let Some(target) = app.tags_target.clone() {
                    let _ = TagEditor::new(tag_backups_dir()).save(
                        &target,
                        &TagEdit {
                            artwork_path: Some(path),
                            ..TagEdit::default()
                        },
                    );
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_get_tag_backups(move |_id| {
            let app = a.borrow_mut();
            if let Some(path) = app.tags_target.clone() {
                let backups: Vec<BackupItem> = TagEditor::new(tag_backups_dir())
                    .list_backups(&path)
                    .into_iter()
                    .map(|b| BackupItem {
                        label: ss(file_name(&b.backup_path)),
                    })
                    .collect();
                app.ui
                    .global::<State>()
                    .set_detail_backups(ui_model(backups).into());
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_log_folder(move || {
            let folder = a.borrow().log_folder.clone();
            open_folder(&folder);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_file_location(move |id| {
            if let Some(track) = a.borrow().track_by_id(&id) {
                open_in_explorer(&track.path);
            }
        });
    }
    {
        let _a = app.clone();
        ui.global::<Actions>()
            .on_open_file_location_at(move |path| open_in_explorer(&path));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_copy_track_path(move |id| {
            if let Some(track) = a.borrow().track_by_id(&id) {
                set_clipboard(&track.path);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_confirm_restore_backup(move |label| {
                let mut app = a.borrow_mut();
                app.confirm_action = Some(ConfirmAction::RestoreBackup(label.to_string()));
                app.ui
                    .global::<State>()
                    .set_confirm_message(ss(format!("Restore tag backup {}?", file_name(&label))));
                app.open_modal("confirm");
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_restore_tags(move |id, path| {
            let mut app = a.borrow_mut();
            app.context_target = Some(id.to_string());
            app.restore_backup(&path);
        });
    }

    // Lyrics.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_find_lyrics(move |id| a.borrow_mut().open_find_lyrics(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_find_lyrics(move || a.borrow_mut().find_lyrics());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_use_lyric_result(move |id, timed| a.borrow_mut().use_lyric_result(&id, timed));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_edit_lyrics(move || a.borrow_mut().open_edit_lyrics());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_save_lyrics_modal(move || a.borrow_mut().save_lyrics_modal());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_save_lyrics(move |text, sidecar, offset| {
                let mut app = a.borrow_mut();
                let state = app.ui.global::<State>();
                state.set_edit_lyrics_text(ss(text));
                state.set_edit_lyrics_sidecar(sidecar);
                state.set_edit_lyrics_offset(ss(offset));
                drop(state);
                app.save_lyrics_modal();
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_remove_lyrics(move || {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::RemoveLyrics);
            app.ui
                .global::<State>()
                .set_confirm_message(ss("Remove the current lyrics?"));
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_stamp_lyric_position(move || a.borrow_mut().stamp_lyric_position());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_seek_to_lyric(move |index| a.borrow_mut().seek_to_lyric(index as usize));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_lyrics_scroll_changed(move |paused| {
                let app = a.borrow_mut();
                let state = app.ui.global::<State>();
                state.set_lyrics_scroll_paused(paused);
                if !paused {
                    state.set_lyrics_scroll_nonce(state.get_lyrics_scroll_nonce() + 1);
                    state.set_lyrics_active_line(app.lyrics_active);
                }
            });
    }

    // Playlists.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_create_playlist(move |name| a.borrow_mut().create_playlist(&name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_rename_playlist(move |id, name| a.borrow_mut().rename_playlist(&id, &name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_delete_playlist(move |id| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::DeletePlaylist(id.to_string()));
            app.ui
                .global::<State>()
                .set_confirm_message(ss("Delete this playlist? The tracks stay in your library."));
            app.ui.global::<State>().set_confirm_danger(true);
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_add_track_to_playlist(move |track_id, playlist_name| {
                let mut app = a.borrow_mut();
                let Some(id) = app
                    .playlists
                    .iter()
                    .find(|p| playlist_name == p.name)
                    .map(|p| p.id.clone())
                else {
                    return;
                };
                if let Some(track) = app.track_by_id(&track_id) {
                    if app.store.add_tracks_to_playlist(&id, &[track.path]).is_ok() {
                        app.invalidate_view_caches();
                        app.refresh_playlists();
                        app.close_modal();
                        app.show_notice("Added to playlist.", NoticeSeverity::Success);
                    }
                }
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_remove_from_playlist(move |position| {
                let mut app = a.borrow_mut();
                let id = app.ui.global::<State>().get_view_playlist_id().to_string();
                if app
                    .store
                    .remove_from_playlist(&id, position as u32)
                    .unwrap_or(false)
                {
                    app.invalidate_view_caches();
                    app.refresh_view();
                }
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_playlist_move(move |from, to| {
            let mut app = a.borrow_mut();
            let id = app.ui.global::<State>().get_view_playlist_id().to_string();
            if app
                .store
                .move_playlist_track(&id, from as u32, to as u32)
                .unwrap_or(false)
            {
                app.invalidate_view_caches();
                app.refresh_view();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_import_m3u(move || {
            if let Some(path) = pick_file("M3U playlists (*.m3u;*.m3u8)|*.m3u;*.m3u8") {
                let name = file_stem(&path);
                if let Ok(paths) = music_player_core::import_m3u8(Path::new(&path), |p| {
                    music_player_core::is_supported_audio(Path::new(p))
                }) {
                    let mut app = a.borrow_mut();
                    let id = app.new_playlist_id();
                    if app.store.create_playlist(&id, &name).is_ok() {
                        let _ = app.store.add_tracks_to_playlist(&id, &paths);
                        app.invalidate_view_caches();
                        app.refresh_playlists();
                        app.show_notice("Playlist imported.", NoticeSeverity::Success);
                    }
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_export_m3u(move |id| {
            let Some(playlist) = a.borrow().store.get_playlist(&id).ok().flatten() else {
                return;
            };
            let tracks: Vec<Track> = playlist
                .paths
                .iter()
                .filter_map(|p| a.borrow().store.get_track(p).ok().flatten())
                .collect();
            if let Some(path) = save_file(&playlist.name, "M3U8 playlist (*.m3u8)|*.m3u8") {
                let _ = music_player_core::export_m3u8(&playlist, &tracks, Path::new(&path));
                a.borrow_mut()
                    .show_notice("Playlist exported.", NoticeSeverity::Success);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_add_to_playlist(move |id| {
            a.borrow_mut().open_add_to_playlist(&id);
        });
    }

    // Context menu.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_context_menu(move |id, x, y| a.borrow_mut().open_context_menu(&id, x, y));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_context_action(move |index| a.borrow_mut().context_action(index as usize));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_context_menu(move || {
            a.borrow().ui.global::<State>().set_context_visible(false);
        });
    }

    // Modals.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_modal(move |name| a.borrow_mut().open_modal(&name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_close_modal(move || a.borrow_mut().close_modal());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_confirm_action(move || a.borrow_mut().confirm_action());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_confirm_remove_root(move |path| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::RemoveRoot(path.to_string()));
            app.ui
                .global::<State>()
                .set_confirm_message(ss(format!("Remove {} from the library?", path)));
            app.ui.global::<State>().set_confirm_danger(true);
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_rate_track(move |rating| a.borrow_mut().rate_track(rating));
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_tags_field_changed(move |index, value| {
                let mut app = a.borrow_mut();
                if let Some(field) = app.tags_additional.get_mut(index as usize) {
                    field.1 = value.to_string();
                }
                app.publish_tag_fields();
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_tags_custom_key_changed(move |index, value| {
                let mut app = a.borrow_mut();
                if let Some(field) = app.tags_custom.get_mut(index as usize) {
                    field.0 = value.to_string();
                }
                app.publish_tag_fields();
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_tags_custom_value_changed(move |index, value| {
                let mut app = a.borrow_mut();
                if let Some(field) = app.tags_custom.get_mut(index as usize) {
                    field.1 = value.to_string();
                }
                app.publish_tag_fields();
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_tags_remove_custom(move |index| {
            let mut app = a.borrow_mut();
            if (index as usize) < app.tags_custom.len() {
                app.tags_custom.remove(index as usize);
            }
            app.publish_tag_fields();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_tags_add_custom_field(move |key, value| {
                let mut app = a.borrow_mut();
                app.tags_custom.push((key.to_string(), value.to_string()));
                app.publish_tag_fields();
            });
    }
}
