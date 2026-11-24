use crate::*;

impl App {
    pub(crate) fn smart_autoplay(&self, track: &Track, limit: usize) -> Vec<String> {
        // 1:1 with TryAutoplaySimilar: the queue's current entries are the
        // exclusion list, and the store applies the original weighted SQL.
        let exclude = self.coordinator.queue().entries().to_vec();
        self.store
            .smart_autoplay_tracks(&track.path, &track.genre, &track.artist, &exclude, limit)
            .unwrap_or_default()
    }

    // ---- Playback command surface (port of PlaybackService orchestration) --

    pub(crate) fn resolver(&self) -> impl Fn(&str) -> Option<Track> {
        let store = self.store.clone();
        move |path| store.get_track(path).ok().flatten()
    }

    pub(crate) fn track_by_id(&self, id: &str) -> Option<Track> {
        let path = self
            .id_to_path
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
        if let Some(track) = self.track_cache.get(&path) {
            return Some(track.clone());
        }
        if let Ok(Some(track)) = self.store.get_track(&path) {
            return Some(track);
        }
        if let Some(track) = self.track_cache.get(id) {
            return Some(track.clone());
        }
        self.store.get_track(id).ok().flatten()
    }

    pub(crate) fn resolve_track(&mut self, path: &str) -> Option<Track> {
        if let Some(track) = self.track_cache.get(path) {
            return Some(track.clone());
        }
        if let Ok(Some(track)) = self.store.get_track(path) {
            self.track_cache.insert(path.to_string(), track.clone());
            Some(track)
        } else {
            None
        }
    }

    pub(crate) fn prefetch_upcoming_tracks(&mut self) {
        let entries = self.coordinator.queue().entries().to_vec();
        let current_index = self.coordinator.queue().current_index();
        let upcoming_start = current_index.map(|i| i + 1).unwrap_or(0);
        for path in entries.iter().skip(upcoming_start).take(10) {
            if !self.track_cache.contains_key(path) {
                if let Ok(Some(track)) = self.store.get_track(path) {
                    self.track_cache.insert(path.clone(), track);
                }
            }
        }

        // Preload upcoming lyrics (next 3 tracks in queue, 1:1 parity with WebUI preloadUpcomingLyrics)
        use music_player_core::settings::keys as k;
        let auto_load = self.settings.bool_or(k::AUTO_LOAD_LYRICS, true);
        for path in entries.iter().skip(upcoming_start).take(3) {
            if self.lyrics_cache.contains_key(path) {
                continue;
            }
            if let Some(sidecar) = read_sidecar(path) {
                let doc = parse_lrc(&sidecar);
                let timed = !doc.lines.is_empty();
                self.lyrics_cache.insert(
                    path.clone(),
                    (
                        Instant::now(),
                        Some(music_player_core::lyrics::LyricsMatch {
                            raw: sidecar,
                            timed,
                            score: 1.0,
                            result: music_player_core::lyrics::LyricsSearchResult::default(),
                        }),
                    ),
                );
                continue;
            }
            if let Some(embedded) = self
                .metadata_engine
                .read_metadata(Path::new(path), None)
                .ok()
                .and_then(|t| t.lyrics)
            {
                let doc = parse_lrc(&embedded);
                let timed = !doc.lines.is_empty();
                self.lyrics_cache.insert(
                    path.clone(),
                    (
                        Instant::now(),
                        Some(music_player_core::lyrics::LyricsMatch {
                            raw: embedded,
                            timed,
                            score: 1.0,
                            result: music_player_core::lyrics::LyricsSearchResult::default(),
                        }),
                    ),
                );
                continue;
            }
            if auto_load {
                if let Some(track) = self.track_cache.get(path).cloned() {
                    let tx = self.prefetch_lyrics_tx.clone();
                    let t_path = path.clone();
                    let t_title = track.title.clone();
                    let t_artist = track.artist.clone();
                    let t_duration = (track.duration_ms as f64) / 1000.0;
                    std::thread::spawn(move || {
                        let mut client = music_player_core::lyrics::LrclibSearchClient::new();
                        let matched = client
                            .search(&t_title, &t_artist)
                            .ok()
                            .and_then(|results| {
                                music_player_core::lyrics::select_lyrics_match(
                                    &t_title,
                                    &t_artist,
                                    t_duration,
                                    &results,
                                )
                            });
                        let _ = tx.send((t_path, matched));
                    });
                }
            }
        }
    }

    pub(crate) fn play_track(&mut self, track: Track, reset_queue: bool, index: Option<usize>) {
        let resolver = self.resolver();
        let result =
            self.coordinator
                .start_track(&track, reset_queue, index, &self.view_paths, &resolver);
        self.apply_result(result);
    }

    pub(crate) fn apply_result(&mut self, result: PlaybackCommandResult) {
        use music_player_core::audio::coordinator::PlaybackCommandKind as K;
        match result.kind {
            K::NoChange => {}
            K::TrackStarted => {
                let Some(path) = result.track_path.clone() else {
                    return;
                };
                if self.availability.mark_available(&path) {
                    self.refresh_view();
                }
                if let Err(err) = self.engine.play_track(&path) {
                    self.show_notice(
                        format!(
                            "Could not play {}. See Settings → Open log folder for details.",
                            file_name(&path)
                        ),
                        NoticeSeverity::Error,
                    );
                    LocalAppLog::shared().error("playback", "play_track failed", Some(&err));
                    return;
                }
                self.listening.reset_for_track();
                self.crossfade_in_progress = false;
                self.crossfade_failure_source = None;
                self.crossfade_source_index = None;
                if let Some(track) = self.resolve_track(&path) {
                    self.update_current_track(&track);
                }
                self.prefetch_upcoming_tracks();
                if result.persist_session {
                    self.save_session();
                }
                if result.queue_changed {
                    self.publish_queue(true);
                } else {
                    self.publish_queue(false);
                }
                self.publish_playback();
            }
            K::Paused => {
                let _ = self.engine.pause();
                self.publish_playback();
            }
            K::Resumed => {
                let _ = self.engine.resume();
                self.listening
                    .mark_resumed(self.engine.position_ms() as i64);
                self.publish_playback();
            }
            K::PositionReset => {
                let _ = self.engine.seek(0);
                self.publish_playback();
            }
            K::CrossfadeRequested => {
                if let Some(request) = result.crossfade {
                    self.crossfade_source_index = Some(request.source_index);
                    self.crossfade_in_progress = true;
                    let path = result.track_path.clone().unwrap_or_default();
                    let title = file_stem(&path);
                    match self.engine.begin_crossfade(&path, request.duration_ms) {
                        CrossfadeOutcome::Committed => {
                            if let Some(track) = self.store.get_track(&path).ok().flatten() {
                                self.update_current_track(&track);
                            }
                            self.listening.reset_for_track();
                        }
                        CrossfadeOutcome::Canceled => {
                            self.cancel_crossfade();
                        }
                        CrossfadeOutcome::Rejected => {
                            // Plain start when the engine cannot fade.
                            self.crossfade_in_progress = false;
                            if let Err(err) = self.engine.play_track(&path) {
                                self.show_notice(
                                    format!("Could not play {}. See Settings → Open log folder for details.", title),
                                    NoticeSeverity::Error,
                                );
                                LocalAppLog::shared().error(
                                    "playback",
                                    "crossfade fallback failed",
                                    Some(&err),
                                );
                            } else if let Some(track) = self.store.get_track(&path).ok().flatten() {
                                self.update_current_track(&track);
                                self.listening.reset_for_track();
                            }
                        }
                        CrossfadeOutcome::Failed(message) => {
                            self.crossfade_in_progress = false;
                            self.crossfade_failure_source =
                                self.current.as_ref().map(|t| t.path.clone());
                            self.restore_crossfade_queue_index();
                            self.show_notice(
                                format!("Could not start {} during crossfade. Playback will continue; see the local log for details.", title),
                                NoticeSeverity::Error,
                            );
                            LocalAppLog::shared().error("playback", &message, None);
                        }
                    }
                    self.publish_queue(true);
                    self.publish_playback();
                }
            }
            K::StopPlayback => {
                self.cancel_crossfade();
                let _ = self.engine.stop();
                if result.queue_changed {
                    self.publish_queue(true);
                }
                self.publish_playback();
            }
            K::TrackUnavailable => {
                let missing = result.track_path.clone();
                if let Some(path) = missing {
                    if !Path::new(&path).exists() {
                        let removed = self.reconcile_missing(&path);
                        let name = file_name(&path);
                        let notice = if removed {
                            format!("File not found: {}. Its library, playlist and queue entries were removed.", name)
                        } else {
                            format!("The file location is unavailable: {}. Its library and playlist references were kept; reconnect the drive and rescan when it is available.", name)
                        };
                        self.show_notice(notice, NoticeSeverity::Warning);
                    } else if let Some(notice) = result.notice {
                        self.show_notice(notice.message, NoticeSeverity::Warning);
                    }
                } else if let Some(notice) = result.notice {
                    self.show_notice(notice.message, NoticeSeverity::Warning);
                }
                self.publish_queue(true);
            }
            K::AdvanceFallback => self.advance_queue(false),
        }
    }

    pub(crate) fn reconcile_missing(&mut self, path: &str) -> bool {
        let confirmed_missing = self.availability.mark_unavailable(path);
        if confirmed_missing {
            self.coordinator
                .queue_mut()
                .remove_paths(&[path.to_string()]);
            self.refresh_view();
        }
        confirmed_missing
    }

    pub(crate) fn handle_playback_failure(&mut self, track: &Track) {
        self.crossfade_in_progress = false;
        if !Path::new(&track.path).exists() {
            let _ = self.engine.stop();
            let removed = self.reconcile_missing(&track.path);
            let name = file_name(&track.path);
            self.show_notice(
                if removed {
                    format!("File not found: {}. Its library, playlist and queue entries were removed.", name)
                } else {
                    format!("The file location is unavailable: {}. Its library and playlist references were kept; reconnect the drive and rescan when it is available.", name)
                },
                NoticeSeverity::Warning,
            );
        } else {
            self.show_notice(
                format!(
                    "Could not play {}. See Settings → Open log folder for details.",
                    track.title
                ),
                NoticeSeverity::Error,
            );
        }
    }

    pub(crate) fn cancel_crossfade(&mut self) {
        if !self.crossfade_in_progress {
            return;
        }
        if self.engine.has_committed_crossfade() {
            self.crossfade_in_progress = false;
            self.crossfade_source_index = None;
            self.crossfade_failure_source = None;
            self.listening.reset_for_track();
            if let Some(path) = self.current.as_ref().map(|t| t.path.clone()) {
                if let Some(track) = self.resolve_track(&path) {
                    self.update_current_track(&track);
                }
                self.prefetch_upcoming_tracks();
            }
            return;
        }
        self.engine.cancel_crossfade();
        self.crossfade_in_progress = false;
        self.restore_crossfade_queue_index();
    }

    pub(crate) fn restore_crossfade_queue_index(&mut self) {
        let index = self
            .crossfade_source_index
            .filter(|i| *i < self.coordinator.queue().entries().len())
            .or_else(|| {
                self.current
                    .as_ref()
                    .and_then(|t| self.coordinator.queue().find_index(Some(&t.path)))
            });
        self.coordinator.queue_mut().set_current_index(index);
        self.crossfade_source_index = None;
    }

    pub(crate) fn start_crossfade(&mut self, target_index: usize, duration_ms: u64) {
        let resolver = self.resolver();
        let source = if self.crossfade_in_progress {
            self.crossfade_source_index
        } else {
            None
        };
        let result =
            self.coordinator
                .prepare_crossfade(target_index, duration_ms, source, &resolver);
        self.apply_result(result);
    }

    pub(crate) fn advance_queue(&mut self, automatic: bool) {
        let blocked = self
            .crossfade_failure_source
            .as_deref()
            .map(|p| {
                self.current
                    .as_ref()
                    .map(|t| t.path.eq_ignore_ascii_case(p))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let source = if self.crossfade_in_progress {
            self.crossfade_source_index
        } else {
            None
        };
        let autoplay = self
            .settings
            .bool_or(music_player_core::settings::keys::AUTOPLAY_SIMILAR, true);
        let fallback = if autoplay {
            self.current
                .as_ref()
                .and_then(|t| self.smart_autoplay(t, 10).into_iter().next())
                .map(|first| vec![first])
        } else {
            None
        };
        let resolver = self.resolver();
        let result = self
            .coordinator
            .advance_with_options(automatic, blocked, source, &resolver, fallback);
        if matches!(result.kind, PlaybackCommandKind::StopPlayback)
            && (automatic || self.coordinator.queue().current_index().is_none())
            && autoplay
        {
            if let Some(current) = self.current.clone() {
                let recommendations = self.smart_autoplay(&current, 10);
                if !recommendations.is_empty() {
                    self.coordinator
                        .queue_mut()
                        .append(&recommendations, Some(&current.path));
                    self.publish_queue(true);
                    let resolver = self.resolver();
                    let result = self
                        .coordinator
                        .advance_with_options(true, false, None, &resolver, None);
                    self.apply_result(result);
                    return;
                }
            }
        }
        self.apply_result(result);
    }

    pub(crate) fn save_session(&mut self) {
        let scale = self.ui.window().scale_factor().max(1.0);
        let phys = self.ui.window().size();
        let log_w = (phys.width as f32 / scale).round() as u32;
        let log_h = (phys.height as f32 / scale).round() as u32;
        if (720..=1400).contains(&log_w) && (500..=900).contains(&log_h) {
            let _ = self.store.set_setting("window-width", &log_w.to_string());
            let _ = self.store.set_setting("window-height", &log_h.to_string());
        }
        let session = PlaybackSession {
            track_path: self.current.as_ref().map(|t| t.path.clone()),
            position_ms: self.engine.position_ms(),
            queue: self.coordinator.queue().entries().to_vec(),
            shuffle: self.coordinator.queue().shuffle,
            repeat_mode: self.coordinator.queue().repeat_mode.as_str().to_string(),
            repeat_a_ms: self.coordinator.queue().repeat_a_ms(),
            repeat_b_ms: self.coordinator.queue().repeat_b_ms(),
            queue_index: self
                .coordinator
                .queue()
                .current_index()
                .map(|i| i as i32)
                .unwrap_or(-1),
        };
        let _ = self.store.save_session(&session);
    }

    pub(crate) fn restore_session(&mut self) {
        let Some(session) = self.store.load_session().ok().flatten() else {
            return;
        };

        let saved_queue = session.queue.clone();
        let saved_index = if session.queue_index >= 0 {
            Some(session.queue_index as usize)
        } else {
            None
        };
        let saved_current_path = session.track_path.clone();

        // Preserve the occurrence of the current track at the saved index so
        // duplicate queue entries restore to the intended position.
        let current_occurrence = saved_index
            .and_then(|idx| saved_queue.get(idx))
            .filter(|path| {
                saved_current_path
                    .as_deref()
                    .map(|c| c.eq_ignore_ascii_case(path))
                    .unwrap_or(false)
            })
            .map(|_| {
                let idx = saved_index.unwrap_or(0).min(saved_queue.len());
                saved_queue[..=idx]
                    .iter()
                    .filter(|p| {
                        saved_current_path
                            .as_deref()
                            .map(|c| c.eq_ignore_ascii_case(p))
                            .unwrap_or(false)
                    })
                    .count()
                    .saturating_sub(1)
            })
            .unwrap_or(0usize);

        let mut filtered_queue = Vec::with_capacity(saved_queue.len());
        for path in saved_queue {
            if Path::new(&path).exists() {
                filtered_queue.push(path);
                continue;
            }
            let parent_exists = Path::new(&path)
                .parent()
                .map(|d| d.exists())
                .unwrap_or(false);
            if parent_exists {
                // File is missing from a reachable folder: treat as deleted.
                let _ = self.store.purge_deleted_tracks(std::slice::from_ref(&path));
            } else {
                // Parent is unreachable (e.g. removed drive/folder): keep the
                // entry so it can become available again.
                filtered_queue.push(path);
            }
        }

        let resolved_index = if let Some(ref current_path) = saved_current_path {
            if let Some(mut occ) = Some(current_occurrence) {
                let mut found = None;
                for (i, p) in filtered_queue.iter().enumerate() {
                    if p.eq_ignore_ascii_case(current_path) {
                        if occ == 0 {
                            found = Some(i);
                            break;
                        }
                        occ -= 1;
                    }
                }
                found.or_else(|| {
                    filtered_queue
                        .iter()
                        .position(|p| p.eq_ignore_ascii_case(current_path))
                })
            } else {
                filtered_queue
                    .iter()
                    .position(|p| p.eq_ignore_ascii_case(current_path))
            }
        } else {
            saved_index.filter(|i| *i < filtered_queue.len())
        };

        self.coordinator
            .queue_mut()
            .replace(filtered_queue, resolved_index);
        self.coordinator.queue_mut().set_shuffle(session.shuffle);
        self.coordinator
            .queue_mut()
            .set_repeat_mode(RepeatMode::from_str_lenient(&session.repeat_mode));
        self.coordinator
            .queue_mut()
            .set_ab_repeat_points(session.repeat_a_ms, session.repeat_b_ms);

        let current = saved_current_path
            .as_ref()
            .filter(|p| Path::new(p).exists())
            .and_then(|p| self.store.get_track(p).ok().flatten());
        if let Some(track) = current {
            self.update_current_track(&track);
            if self.engine.play_track(&track.path).is_ok() {
                let _ = self.engine.pause();
                let _ = self.engine.seek(session.position_ms);
                self.listening.restore_paused(session.position_ms as i64);
            }
        } else if let Some(ref path) = saved_current_path {
            // If the saved track is unavailable but still in the queue, keep
            // the coordinator/app current path aligned with the session.
            self.coordinator.set_current_track_path(Some(path.clone()));
        }
    }

    pub(crate) fn set_crossfade_label(&mut self, label: &str) {
        let seconds = match label {
            "2 s" | "2 seconds" => 2,
            "3 s" | "3 seconds" => 3,
            "5 s" | "5 seconds" => 5,
            "8 s" | "8 seconds" => 8,
            "10 s" | "10 seconds" => 10,
            _ => 0,
        };
        self.settings.set_raw(
            music_player_core::settings::keys::CROSSFADE_SECONDS,
            &seconds.to_string(),
        );
        self.coordinator.set_crossfade_ms(seconds * 1000);
        self.ui.global::<State>().set_crossfade_label(ss(label));
    }

    pub(crate) fn apply_equalizer(&mut self) {
        let preset = self.coordinator_eq_name();
        let name = if preset == "Off" {
            None
        } else {
            Some(preset.as_str())
        };
        self.engine
            .apply_equalizer(name, Some(self.eq_bands), self.eq_preamp);
    }

    pub(crate) fn set_eq_preset(&mut self, name: &str) {
        if name == "Off" {
            self.eq_bands = [0.0; 10];
            self.eq_preamp = NEUTRAL_EQUALIZER_PREAMP;
        } else if let Some((bands, preamp)) = self.settings.saved_eq_preset(name) {
            self.eq_bands = bands;
            self.eq_preamp = preamp;
        } else if let Some(gains) = music_player_core::canonical_preset_by_name(name) {
            self.eq_bands = gains;
        }
        self.settings
            .set_eq_state(name, self.eq_bands, self.eq_preamp);
        self.ui.global::<State>().set_eq_preset(ss(name));
        self.apply_equalizer();
        self.publish_eq();
    }

    pub(crate) fn clear_queue(&mut self) {
        self.coordinator.queue_mut().clear_upcoming();
        self.publish_queue(true);
        self.save_session();
    }

    pub(crate) fn read_external_tracks(&mut self, paths: &[String]) -> Vec<Track> {
        let mut tracks = Vec::new();
        for path in paths {
            let p = Path::new(path);
            if !music_player_core::is_supported_audio(p) {
                continue;
            }
            if let Ok(Some(existing)) = self.store.get_track(path) {
                if let Ok(meta) = std::fs::metadata(p) {
                    if existing.file_size == meta.len() {
                        tracks.push(existing);
                        continue;
                    }
                }
            }
            let extracted = self
                .metadata_engine
                .read_metadata(p, Some(&artwork_cache_dir()))
                .unwrap_or_default();
            let meta = std::fs::metadata(p).ok();
            let file_size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .map(chrono::DateTime::from)
                .unwrap_or_else(chrono::Utc::now);
            let title = extracted
                .title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| file_stem(path));
            let track = Track {
                path: path.clone(),
                title,
                artist: extracted.artist.unwrap_or_default(),
                album: extracted.album.unwrap_or_default(),
                album_artist: extracted.album_artist.unwrap_or_default(),
                genre: extracted.genre.unwrap_or_default(),
                year: extracted.year,
                track_number: extracted.track_number,
                duration_ms: extracted.duration_ms,
                file_size,
                modified_utc: modified,
                added_utc: chrono::Utc::now(),
                artwork_path: extracted.artwork_path,
                has_lyrics: extracted.has_lyrics || read_sidecar(path).is_some(),
                rating: 0,
                favorite: false,
                play_count: 0,
                last_played_utc: None,
                unavailable: false,
            };
            let _ = self.store.upsert_tracks(&[track.clone()]);
            tracks.push(track);
        }
        tracks
    }

    pub(crate) fn play_external_files(&mut self, paths: &[String]) {
        let tracks = self.read_external_tracks(paths);
        if tracks.is_empty() {
            self.show_notice(
                "No supported audio files could be opened.",
                NoticeSeverity::Warning,
            );
            return;
        }
        let opened_paths: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
        let current_path = self.current.as_ref().map(|t| t.path.as_str());
        let existing_queue: Vec<String> = self.coordinator.queue().entries().to_vec();
        let current_idx = self
            .coordinator
            .queue()
            .current_index()
            .map(|i| i as i64)
            .unwrap_or(0);
        let new_queue = music_player_core::audio::queue::build_external_open_queue(
            &opened_paths,
            current_path,
            &existing_queue,
            current_idx,
        );
        self.coordinator.queue_mut().replace(new_queue, Some(0));
        self.play_track(tracks[0].clone(), false, Some(0));
        self.publish_queue(true);
        if tracks.len() > 1 {
            self.show_notice(
                format!("Playing {}; {} more selected tracks follow, then the previous track and queue.", tracks[0].title, tracks.len() - 1),
                NoticeSeverity::Informational,
            );
        }
    }

    pub(crate) fn add_external_files_to_queue(&mut self, paths: &[String]) {
        let tracks = self.read_external_tracks(paths);
        if tracks.is_empty() {
            self.show_notice(
                "No supported audio files could be added.",
                NoticeSeverity::Warning,
            );
            return;
        }
        let track_paths: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
        let retained = self.current.as_ref().map(|t| t.path.as_str());
        self.coordinator
            .queue_mut()
            .insert_next_range(&track_paths, retained);
        self.publish_queue(true);
        self.save_session();
        self.show_notice(
            format!(
                "Added {} {} to the queue.",
                tracks.len(),
                if tracks.len() == 1 { "track" } else { "tracks" }
            ),
            NoticeSeverity::Informational,
        );
    }

    pub(crate) fn create_playlist_with_external_files(&mut self, paths: &[String]) {
        let tracks = self.read_external_tracks(paths);
        if tracks.is_empty() {
            self.show_notice(
                "No supported audio files could be added.",
                NoticeSeverity::Warning,
            );
            return;
        }
        let track_paths: Vec<String> = tracks.iter().map(|t| t.path.clone()).collect();
        let name = Path::new(&tracks[0].path)
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "New playlist".to_string());
        let id = self.new_playlist_id();
        let _ = self.store.create_playlist(&id, &name);
        let _ = self.store.add_tracks_to_playlist(&id, &track_paths);
        self.invalidate_view_caches();
        self.refresh_playlists();
        self.show_notice(
            format!("Created playlist “{}”.", name),
            NoticeSeverity::Success,
        );
        self.set_view("Playlist", "", "", &id);
    }
}
