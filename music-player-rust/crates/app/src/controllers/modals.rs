use crate::*;

impl App {
    // ---- Playlists ----------------------------------------------------------

    pub(crate) fn new_playlist_id(&self) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{:x}-{:x}", nanos, std::process::id())
    }

    pub(crate) fn create_playlist(&mut self, name: &str) {
        if name.trim().is_empty() {
            self.show_notice("Playlist name cannot be empty.", NoticeSeverity::Warning);
            return;
        }
        let id = self.new_playlist_id();
        if self.store.create_playlist(&id, name.trim()).is_ok() {
            self.invalidate_view_caches();
            self.refresh_playlists();
            self.set_view("Playlist", "", "", &id);
            self.close_modal();
        } else {
            self.show_notice("Could not create the playlist.", NoticeSeverity::Error);
        }
    }

    pub(crate) fn rename_playlist(&mut self, id: &str, name: &str) {
        if name.trim().is_empty() {
            return;
        }
        if self.store.rename_playlist(id, name.trim()).is_ok() {
            self.invalidate_view_caches();
            self.refresh_playlists();
            self.close_modal();
        }
    }

    pub(crate) fn delete_playlist(&mut self, id: &str) {
        if self.store.delete_playlist(id).is_ok() {
            self.invalidate_view_caches();
            self.refresh_playlists();
            self.set_view("Playlists", "", "", "");
            self.close_modal();
        }
    }

    pub(crate) fn play_playlist(&mut self, id: &str, shuffle: bool) {
        let Some(playlist) = self.store.get_playlist(id).ok().flatten() else {
            self.show_notice("Playlist not found.", NoticeSeverity::Warning);
            return;
        };
        let paths: Vec<String> = playlist
            .paths
            .into_iter()
            .filter(|p| Path::new(p).exists())
            .collect();
        if paths.is_empty() {
            self.show_notice(
                "That playlist has no available tracks.",
                NoticeSeverity::Warning,
            );
            return;
        }
        let Some(track) = self.store.get_track(&paths[0]).ok().flatten() else {
            return;
        };
        self.view_paths = paths;
        self.coordinator
            .queue_mut()
            .replace(self.view_paths.clone(), Some(0));
        if shuffle {
            self.coordinator.queue_mut().shuffle_upcoming(1);
        }
        self.play_track(track, false, Some(0));
    }

    // ---- Context menu -------------------------------------------------------

    pub(crate) fn open_context_menu(&mut self, id: &str, x: i32, y: i32) {
        self.context_target = Some(id.to_string());

        let menu_defs = vec![
            ("Play", false, false, true),
            ("Play next", false, false, true),
            ("Add to queue", false, false, true),
            ("Add to playlist", false, false, true),
            ("Create playlist with this song…", false, false, true),
            ("Toggle favorite", false, false, true),
            ("Set rating…", false, false, true),
            ("—", false, true, false),
            ("Lyrics", false, false, true),
            ("Find lyrics…", false, false, true),
            ("Edit lyrics…", false, false, true),
            ("Edit tags", false, false, true),
            ("Restore previous tags", false, false, true),
            ("Track details", false, false, true),
            ("Show in File Explorer", false, false, true),
        ];

        let items: Vec<ContextMenuItem> = menu_defs
            .into_iter()
            .map(|(label, danger, separator, enabled)| ContextMenuItem {
                label: ss(label),
                danger,
                separator,
                enabled,
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_context_items(ui_model(items).into());
        state.set_context_x(if x > 0 { x as f32 } else { 240.0 });
        state.set_context_y(if y > 0 { y as f32 } else { 140.0 });
        state.set_context_visible(true);
    }

    pub(crate) fn context_action(&mut self, index: usize) {
        let Some(id) = self.context_target.clone() else {
            return;
        };
        self.ui.global::<State>().set_context_visible(false);
        match index {
            0 => {
                if let Some(t) = self.track_by_id(&id) {
                    self.play_track(t, true, None);
                }
            }
            1 => {
                if let Some(t) = self.track_by_id(&id) {
                    let current = self.current.as_ref().map(|c| c.path.as_str());
                    self.coordinator.queue_mut().insert_next(t.path, current);
                    self.publish_queue(true);
                    self.save_session();
                }
            }
            2 => {
                if let Some(t) = self.track_by_id(&id) {
                    let current = self.current.as_ref().map(|c| c.path.as_str());
                    self.coordinator.queue_mut().append_one(t.path, current);
                    self.publish_queue(true);
                    self.save_session();
                }
            }
            3 => self.open_add_to_playlist(&id),
            4 => {
                if let Some(t) = self.track_by_id(&id) {
                    let name = t.title.clone();
                    let new_id = self.new_playlist_id();
                    if self
                        .store
                        .create_playlist(&new_id, &format!("{} Mix", name))
                        .is_ok()
                    {
                        let _ = self
                            .store
                            .add_tracks_to_playlist(&new_id, &[t.path.clone()]);
                        self.invalidate_view_caches();
                        self.refresh_playlists();
                        self.set_view("Playlist", "", "", &new_id);
                        self.show_notice(
                            "Created playlist with this song.",
                            NoticeSeverity::Success,
                        );
                    }
                }
            }
            5 => self.toggle_favorite(&id),
            6 => {
                self.ui.global::<State>().set_modal(ss("rating"));
                self.ui.global::<State>().set_rate_value(0);
            }
            8 => self.set_view("Lyrics", "", "", ""),
            9 => self.open_find_lyrics(&id),
            10 => {
                self.edit_target = self.track_by_id(&id).map(|t| t.path.clone());
                self.open_edit_lyrics();
            }
            11 => self.open_tags(&id),
            12 => {
                if let Some(t) = self.track_by_id(&id) {
                    self.confirm_action = Some(ConfirmAction::RestoreBackup(String::new()));
                    let backups = TagEditor::new(tag_backups_dir()).list_backups(&t.path);
                    let choices: Vec<SharedString> = backups
                        .iter()
                        .map(|b| ss(file_name(&b.backup_path)))
                        .collect();
                    self.confirm_action = Some(ConfirmAction::RestoreBackup(
                        backups
                            .first()
                            .map(|b| b.backup_path.clone())
                            .unwrap_or_default(),
                    ));
                    let state = self.ui.global::<State>();
                    state.set_confirm_message(ss("Restore the most recent tag backup?"));
                    state.set_confirm_has_choices(false);
                    state.set_confirm_choices(ui_model(choices).into());
                    state.set_modal(ss("confirm"));
                }
            }
            13 => self.open_details(&id),
            14 => {
                if let Some(t) = self.track_by_id(&id) {
                    open_in_explorer(&t.path);
                }
            }
            _ => {}
        }
    }

    // ---- Modals -------------------------------------------------------------

    pub(crate) fn open_add_to_playlist(&mut self, id: &str) {
        self.add_playlist_track = Some(id.to_string());
        self.ui.global::<State>().set_add_to_playlist_id(ss(id));
        self.open_modal("add-to-playlist");
    }

    pub(crate) fn open_modal(&mut self, name: &str) {
        let title = match name {
            "tags" => "Edit tags",
            "details" => "Track details",
            "find-lyrics" => "Find lyrics",
            "edit-lyrics" => "Edit lyrics",
            "folders" => "Library folders",
            "exclusions" => "Scan exclusions",
            "add-to-playlist" => "Add to playlist",
            "new-playlist" => "Create playlist",
            "rename-playlist" => "Rename playlist",
            "save-preset" => "Save equalizer preset",
            "confirm" => "Confirm",
            "rating" => "Set rating",
            "duplicate-locations" => "Duplicate locations",
            "reset" => "Reset settings",
            "update" => "Update available",
            _ => name,
        };
        let state = self.ui.global::<State>();
        if name == "reset" {
            state.set_reset_appearance(true);
            state.set_reset_playback(true);
            state.set_reset_library(true);
        }
        state.set_modal(ss(name));
        state.set_modal_title(ss(title));
    }

    pub(crate) fn close_modal(&mut self) {
        self.ui.global::<State>().set_modal(ss(""));
        self.confirm_action = None;
    }

    pub(crate) fn open_details(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else {
            return;
        };
        self.details_target = Some(track.path.clone());
        let details = self.metadata_engine.inspect_file(Path::new(&track.path)).ok();
        let mut rows = vec![
            DetailRowItem {
                label: ss("File"),
                value: ss(file_name(&track.path)),
            },
            DetailRowItem {
                label: ss("Title"),
                value: ss(&track.title),
            },
            DetailRowItem {
                label: ss("Artist"),
                value: ss(&track.artist),
            },
            DetailRowItem {
                label: ss("Album"),
                value: ss(&track.album),
            },
            DetailRowItem {
                label: ss("Album artist"),
                value: ss(&track.album_artist),
            },
            DetailRowItem {
                label: ss("Genre"),
                value: ss(&track.genre),
            },
            DetailRowItem {
                label: ss("Year"),
                value: ss(track.year.to_string()),
            },
            DetailRowItem {
                label: ss("Duration"),
                value: ss(fmt_time(track.duration_ms)),
            },
            DetailRowItem {
                label: ss("Size"),
                value: ss(format!("{:.1} MB", track.file_size as f64 / 1_048_576.0)),
            },
        ];
        if let Some(d) = details {
            rows.push(DetailRowItem {
                label: ss("Container"),
                value: ss(d.container),
            });
            rows.push(DetailRowItem {
                label: ss("Bitrate"),
                value: ss(format!("{} kbps", d.bitrate_kbps)),
            });
        }
        let additional: Vec<DetailRowItem> =
            read_additional_standard_fields(Path::new(&track.path))
                .into_iter()
                .map(|(k, v)| DetailRowItem {
                    label: ss(k),
                    value: ss(v),
                })
                .collect();
        let custom: Vec<DetailRowItem> = read_custom_fields(Path::new(&track.path))
            .into_iter()
            .map(|(k, v)| DetailRowItem {
                label: ss(k),
                value: ss(v),
            })
            .collect();
        let backups: Vec<BackupItem> = TagEditor::new(tag_backups_dir())
            .list_backups(&track.path)
            .into_iter()
            .map(|b| BackupItem {
                label: ss(file_name(&b.backup_path)),
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_detail_rows(ui_model(rows).into());
        state.set_detail_additional(ui_model(additional).into());
        state.set_detail_custom(ui_model(custom).into());
        state.set_detail_backups(ui_model(backups).into());
        self.open_modal("details");
    }

    pub(crate) fn open_tags(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else {
            return;
        };
        self.tags_target = Some(track.path.clone());
        let tags = self
            .metadata_engine
            .read_metadata(Path::new(&track.path), Some(&artwork_cache_dir()))
            .unwrap_or_default();
        let state = self.ui.global::<State>();
        state.set_tags_title(ss(tags.title.unwrap_or_else(|| track.title.clone())));
        state.set_tags_artist(ss(tags.artist.unwrap_or_else(|| track.artist.clone())));
        state.set_tags_album(ss(tags.album.unwrap_or_else(|| track.album.clone())));
        state.set_tags_album_artist(ss(tags
            .album_artist
            .unwrap_or_else(|| track.album_artist.clone())));
        state.set_tags_genre(ss(tags.genre.unwrap_or_else(|| track.genre.clone())));
        state.set_tags_year(ss(if tags.year > 0 {
            tags.year.to_string()
        } else {
            track.year.to_string()
        }));
        state.set_tags_track(ss(if tags.track_number > 0 {
            tags.track_number.to_string()
        } else {
            String::new()
        }));
        state.set_tags_artwork_name(ss(tags
            .artwork_path
            .as_deref()
            .map(file_name)
            .unwrap_or_default()));
        state.set_tags_has_artwork(tags.artwork_path.is_some());
        state.set_tags_format_note(ss(music_player_core::custom_field_format(Path::new(
            &track.path,
        ))
        .unwrap_or("")));
        self.tags_additional = read_additional_standard_fields(Path::new(&track.path))
            .into_iter()
            .collect();
        self.tags_custom = read_custom_fields(Path::new(&track.path))
            .into_iter()
            .collect();
        self.publish_tag_fields();
        self.open_modal("tags");
    }

    pub(crate) fn publish_tag_fields(&mut self) {
        let additional: Vec<TagFieldItem> = self
            .tags_additional
            .iter()
            .map(|(k, v)| TagFieldItem {
                key: ss(k),
                value: ss(v),
            })
            .collect();
        let custom: Vec<TagFieldItem> = self
            .tags_custom
            .iter()
            .map(|(k, v)| TagFieldItem {
                key: ss(k),
                value: ss(v),
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_tags_additional(ui_model(additional).into());
        state.set_tags_custom(ui_model(custom).into());
    }

    pub(crate) fn save_tags(&mut self) {
        let Some(path) = self.tags_target.clone() else {
            return;
        };
        let state = self.ui.global::<State>();
        let title = state.get_tags_title().to_string();
        let artist = state.get_tags_artist().to_string();
        let album = state.get_tags_album().to_string();
        let album_artist = state.get_tags_album_artist().to_string();
        let genre = state.get_tags_genre().to_string();
        let year = state.get_tags_year().to_string().parse().ok();
        let track_number = state.get_tags_track().to_string().parse().ok();
        let mut custom = std::collections::HashMap::new();
        for (k, v) in &self.tags_custom {
            if !k.trim().is_empty() {
                custom.insert(k.clone(), v.clone());
            }
        }
        let mut additional = std::collections::HashMap::new();
        for (k, v) in &self.tags_additional {
            additional.insert(k.clone(), v.clone());
        }
        let edit = TagEdit {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            album_artist: Some(album_artist),
            genre: Some(genre),
            year,
            track_number,
            custom_fields: if custom.is_empty() {
                None
            } else {
                Some(custom)
            },
            additional_fields: if additional.is_empty() {
                None
            } else {
                Some(additional)
            },
            ..TagEdit::default()
        };
        let editor = TagEditor::new(tag_backups_dir());
        let target_path = path.clone();
        let edit_clone = edit.clone();
        std::thread::Builder::new()
            .name("metadata-save".to_string())
            .spawn(move || {
                let outcome = editor
                    .save(&target_path, &edit_clone)
                    .map_err(|e| e.to_string());
                let _ = slint::invoke_from_event_loop(move || {
                    crate::with_app(|app| {
                        app.finish_save_tags(&target_path, &edit_clone, outcome);
                    });
                });
            })
            .ok();
    }

    pub(crate) fn finish_save_tags(
        &mut self,
        path: &str,
        edit: &TagEdit,
        result: Result<TagBackup, String>,
    ) {
        match result {
            Ok(backup) => {
                let snapshot = serde_json::to_string(edit).unwrap_or_default();
                let _ = self.store.record_metadata_edit(
                    path,
                    "tags",
                    None,
                    None,
                    &snapshot,
                    Some(&backup.backup_path),
                );
                self.invalidate_view_caches();
                self.show_notice(
                    "Tags saved. A backup of the original was kept.",
                    NoticeSeverity::Success,
                );
                if let Ok(Some(track)) = self.store.get_track(path) {
                    self.track_cache.insert(path.to_string(), track.clone());
                    self.update_current_track(&track);
                }
                self.close_modal();
                self.refresh_view();
            }
            Err(err) => {
                LocalAppLog::shared().error("tags", "save failed", Some(&err));
                self.show_notice(
                    "Tags could not be saved. See Settings → Open log folder for details.",
                    NoticeSeverity::Error,
                );
            }
        }
    }

    pub(crate) fn restore_backup(&mut self, path: &str) {
        let Some(target) = self.tags_target.clone().or_else(|| {
            self.context_target
                .as_ref()
                .and_then(|id| self.id_to_path.get(id).cloned())
        }) else {
            return;
        };
        let Some(backups) = TagEditor::new(tag_backups_dir())
            .list_backups(&target)
            .into_iter()
            .find(|b| b.backup_path == path || file_name(&b.backup_path) == file_name(path))
        else {
            return;
        };
        match TagEditor::new(tag_backups_dir()).restore(&backups) {
            Ok(restored) => {
                let _ = self.store.record_metadata_edit(
                    &target,
                    "restore",
                    None,
                    None,
                    "{}",
                    Some(&restored.backup_path),
                );
                self.invalidate_view_caches();
                self.show_notice("Tag backup restored.", NoticeSeverity::Success);
                self.close_modal();
                self.refresh_view();
            }
            Err(err) => {
                LocalAppLog::shared().error("tags", "restore failed", Some(&err));
                self.show_notice("Could not restore the tag backup.", NoticeSeverity::Error);
            }
        }
    }

    pub(crate) fn open_find_lyrics(&mut self, id: &str) {
        let track = if !id.is_empty() {
            self.track_by_id(id)
        } else if let Some(path) = &self.lyrics_target {
            self.track_by_id(path)
        } else if let Some(current) = &self.current {
            Some(current.clone())
        } else {
            None
        };
        let Some(track) = track else {
            return;
        };
        self.lyrics_target = Some(track.path.clone());
        let state = self.ui.global::<State>();
        state.set_find_lyrics_title(ss(&track.title));
        state.set_find_lyrics_artist(ss(&track.artist));
        state.set_lyrics_results(ui_model(Vec::<LyricResultItem>::new()).into());
        state.set_lyrics_search_status(ss(""));
        self.open_modal("find-lyrics");
        self.find_lyrics();
    }

    pub(crate) fn find_lyrics(&mut self) {
        let state = self.ui.global::<State>();
        let title = state.get_find_lyrics_title().to_string();
        let artist = state.get_find_lyrics_artist().to_string();
        state.set_lyrics_searching(true);
        state.set_lyrics_search_status(ss("Searching LRCLIB…"));
        let (tx, rx) = mpsc::channel();
        self.lyrics_rx = Some(rx);
        self.lyrics_results.clear();
        std::thread::spawn(move || {
            let mut client = LrclibSearchClient::new();
            let _ = tx.send(client.search(&title, &artist));
            let _ = slint::invoke_from_event_loop(|| {
                crate::with_app(|app| {
                    app.poll_lyrics_search();
                });
            });
        });
    }

    pub(crate) fn use_lyric_result(&mut self, id: &str, timed: bool) {
        let Some(result) = self
            .lyrics_results
            .iter()
            .find(|r| r.id.to_string() == id)
            .cloned()
        else {
            return;
        };
        let text = if timed {
            result.synced_lyrics.clone().unwrap_or_default()
        } else {
            result.plain_lyrics.clone().unwrap_or_default()
        };
        if text.trim().is_empty() {
            return;
        }
        let Some(path) = self.lyrics_target.clone() else {
            return;
        };
        // Default: embed in audio file; fallback to sidecar if embedding fails
        let stored = TagEditor::new(tag_backups_dir())
            .save(
                &path,
                &TagEdit {
                    lyrics: Some(text.clone()),
                    ..TagEdit::default()
                },
            )
            .is_ok();
        if !stored {
            let _ = write_sidecar(&path, &text);
        }
        let _ = self.store.update_track_lyrics_status(&path, true);
        if let Ok(Some(track)) = self.store.get_track(&path) {
            self.load_lyrics(&track);
        }
        self.close_modal();
        self.show_notice("Lyrics saved.", NoticeSeverity::Success);
    }

    pub(crate) fn open_edit_lyrics(&mut self) {
        let Some(path) = self
            .edit_target
            .clone()
            .or_else(|| self.lyrics_target.clone())
            .or_else(|| self.current.as_ref().map(|t| t.path.clone()))
        else {
            return;
        };
        self.edit_target = Some(path.clone());
        let (text, source) = if let Some(sidecar) = read_sidecar(&path) {
            (sidecar, "sidecar file")
        } else if let Some(embedded) = self
            .metadata_engine
            .read_metadata(Path::new(&path), None)
            .ok()
            .and_then(|t| t.lyrics)
            .filter(|l| !l.trim().is_empty())
        {
            (embedded, "embedded tags")
        } else if let Some(doc) = &self.lyrics_doc {
            (
                doc.lines
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                "currently loaded lyrics",
            )
        } else {
            (String::new(), "no existing lyrics")
        };
        let state = self.ui.global::<State>();
        state.set_edit_lyrics_text(ss(text));
        state.set_edit_lyrics_source_label(ss(format!("Source: {}", source)));
        state.set_edit_lyrics_sidecar(false);
        state.set_edit_lyrics_offset(ss(self
            .lyrics_doc
            .as_ref()
            .map(|d| d.offset_ms.to_string())
            .unwrap_or_else(|| "0".into())));
        self.open_modal("edit-lyrics");
    }

    pub(crate) fn save_lyrics_modal(&mut self) {
        let Some(path) = self.edit_target.clone() else {
            return;
        };
        let state = self.ui.global::<State>();
        let mut text = state.get_edit_lyrics_text().to_string();
        let offset: i64 = state
            .get_edit_lyrics_offset()
            .to_string()
            .parse()
            .unwrap_or(0);
        if offset != 0 {
            text = format!("[offset:{}]\n{}", offset, text);
        }
        let sidecar = state.get_edit_lyrics_sidecar();
        let mut stored = if sidecar {
            write_sidecar(&path, &text).is_ok()
        } else {
            TagEditor::new(tag_backups_dir())
                .save(
                    &path,
                    &TagEdit {
                        lyrics: Some(text.clone()),
                        ..TagEdit::default()
                    },
                )
                .is_ok()
        };
        if !stored && !sidecar {
            // Embed fallback to sidecar
            stored = write_sidecar(&path, &text).is_ok();
        }
        if !stored {
            self.show_notice(
                "Lyrics could not be saved. Check the file location.",
                NoticeSeverity::Error,
            );
            return;
        }
        let has_lyrics = !text.trim().is_empty();
        let _ = self.store.update_track_lyrics_status(&path, has_lyrics);
        if let Ok(Some(track)) = self.store.get_track(&path) {
            self.load_lyrics(&track);
        }
        self.close_modal();
        self.show_notice("Lyrics saved.", NoticeSeverity::Success);
    }

    pub(crate) fn remove_lyrics(&mut self) {
        let Some(path) = self
            .edit_target
            .clone()
            .or_else(|| self.lyrics_target.clone())
        else {
            return;
        };
        let _ = std::fs::remove_file(music_player_core::lyrics::sidecar_path(&path));
        let _ = TagEditor::new(tag_backups_dir()).save(
            &path,
            &TagEdit {
                lyrics: Some(String::new()),
                ..TagEdit::default()
            },
        );
        let _ = self.store.update_track_lyrics_status(&path, false);
        if let Ok(Some(track)) = self.store.get_track(&path) {
            self.load_lyrics(&track);
        }
        self.close_modal();
    }

    pub(crate) fn stamp_lyric_position(&mut self) {
        let position = fmt_time(self.engine.position_ms());
        let current = self.ui.global::<State>().get_edit_lyrics_text().to_string();
        self.ui
            .global::<State>()
            .set_edit_lyrics_text(ss(format!("[{}]{}\n{}", position, " ", current)));
    }

    pub(crate) fn seek_to_lyric(&mut self, index: usize) {
        if let Some(doc) = &self.lyrics_doc {
            if let Some(line) = doc.lines.get(index) {
                let mut time = line.time_ms as i64 + doc.offset_ms;
                if time < 0 {
                    time = 0;
                }
                let _ = self.engine.seek(time as u64);
            }
        }
    }

    pub(crate) fn toggle_favorite(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else {
            return;
        };
        let new_value = !track.favorite;
        if let Some(cached) = self.track_cache.get_mut(&track.path) {
            cached.favorite = new_value;
        }
        if let Some(curr) = &mut self.current {
            if curr.path == track.path {
                curr.favorite = new_value;
                self.ui.global::<State>().set_current_favorite(new_value);
            }
        }
        if self.store.set_favorite(&track.path, new_value).is_ok() {
            self.invalidate_view_caches();
            self.refresh_view();
            if self.current.as_ref().map(|t| t.path.as_str()) == Some(track.path.as_str()) {
                if let Ok(Some(updated)) = self.store.get_track(&track.path) {
                    self.update_current_track(&updated);
                }
            }
        }
    }

    pub(crate) fn rate_track(&mut self, rating: i32) {
        let Some(path) = self
            .context_target
            .as_ref()
            .and_then(|id| self.id_to_path.get(id))
            .cloned()
            .or_else(|| self.current.as_ref().map(|t| t.path.clone()))
        else {
            return;
        };
        if self
            .store
            .set_rating(&path, rating.clamp(0, 5) as u8)
            .is_ok()
        {
            self.invalidate_view_caches();
            self.close_modal();
            self.refresh_view();
        }
    }

    pub(crate) fn confirm_action(&mut self) {
        let Some(action) = self.confirm_action.clone() else {
            return;
        };
        match action {
            ConfirmAction::RemoveRoot(path) => {
                let mut roots: Vec<String> = self
                    .settings
                    .library_roots()
                    .into_iter()
                    .filter(|r| !r.eq_ignore_ascii_case(&path))
                    .collect();
                let _ = self.store.purge_tracks_under_directory(&path);
                self.invalidate_view_caches();
                self.settings.set_library_roots(&roots);
                roots.retain(|r| Path::new(r).exists());
                self.load_roots();
                self.refresh_view();
            }
            ConfirmAction::DeletePlaylist(id) => self.delete_playlist(&id),
            ConfirmAction::RestoreBackup(path) => self.restore_backup(&path),
            ConfirmAction::RemoveLyrics => self.remove_lyrics(),
        }
        self.close_modal();
    }
}
