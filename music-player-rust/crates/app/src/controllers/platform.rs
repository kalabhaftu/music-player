use crate::*;

impl App {
    // ---- Startup (port of MainWindow.LibraryScan.StartStartupScanAsync) ---

    pub(crate) fn startup(&mut self) {
        LocalAppLog::shared().info("startup", "Music Player starting");
        self.publish_settings();
        self.refresh_playlists();
        self.load_roots();
        self.restore_session();
        self.clear_stale_artwork();
        self.restore_view_context();
        self.refresh_view();
        self.publish_playback();
        self.publish_queue(true);
        self.publish_scan();
        self.publish_about();
    }

    /// Drops artwork references whose cache file disappeared, then restores
    /// artwork for every indexed track that has no reference yet. Older builds
    /// could persist a whole library without artwork while the files still
    /// contain embedded covers, and a missing cache file made those albums fall
    /// back to gradients forever (the scan's unchanged check treats `NULL`
    /// artwork as fresh, mirroring the C# indexer).
    pub(crate) fn clear_stale_artwork(&mut self) {
        let refs = match self.store.artwork_refs() {
            Ok(refs) => refs,
            Err(_) => return,
        };
        let missing: Vec<String> = refs
            .into_iter()
            .filter(|path| !Path::new(path).is_file())
            .collect();
        let mut repair_paths = Vec::new();
        if !missing.is_empty() {
            let affected = self
                .store
                .tracks_with_artwork_refs(&missing)
                .unwrap_or_default();
            match self.store.clear_missing_artwork_refs(&missing) {
                Ok(cleared) => LocalAppLog::shared().info(
                    "artwork-cache",
                    &format!(
                        "Cleared {} missing artwork reference{} across {} track{}",
                        cleared,
                        if cleared == 1 { "" } else { "s" },
                        affected.len(),
                        if affected.len() == 1 { "" } else { "s" }
                    ),
                ),
                Err(error) => {
                    LocalAppLog::shared().warning(
                        "artwork-cache",
                        &format!("Could not clear missing artwork references: {error}"),
                    );
                    return;
                }
            }
            repair_paths.extend(affected);
        }
        if let Ok(paths) = self.store.tracks_missing_artwork() {
            repair_paths.extend(paths);
        }
        if repair_paths.is_empty() {
            return;
        }

        let mut seen = HashSet::new();
        repair_paths.retain(|path| seen.insert(path.to_lowercase()));
        let store = self.store.clone();
        let cache = artwork_cache_dir();
        let (tx, rx) = mpsc::channel();
        self.artwork_repair_rx = Some(rx);
        std::thread::spawn(move || {
            let mut repaired = 0usize;
            let mut batch: Vec<(String, String)> = Vec::with_capacity(64);
            for path in repair_paths {
                if !Path::new(&path).is_file() {
                    continue;
                }
                let tags =
                    music_player_core::metadata::read_tags_in_cache(Path::new(&path), &cache);
                if let Some(artwork) = tags.artwork_path {
                    batch.push((path, artwork));
                    if batch.len() >= 64 {
                        if let Ok(count) = store.set_track_artworks(&batch) {
                            repaired += count;
                        }
                        batch.clear();
                    }
                }
            }
            if !batch.is_empty() {
                if let Ok(count) = store.set_track_artworks(&batch) {
                    repaired += count;
                }
            }
            let _ = tx.send(repaired);
        });
    }

    pub(crate) fn poll_artwork_repair(&mut self) {
        let Some(rx) = &self.artwork_repair_rx else {
            return;
        };
        let Ok(repaired) = rx.try_recv() else {
            return;
        };
        self.artwork_repair_rx = None;
        if repaired > 0 {
            LocalAppLog::shared().info(
                "artwork-cache",
                &format!(
                    "Re-extracted artwork for {} track{}",
                    repaired,
                    if repaired == 1 { "" } else { "s" }
                ),
            );
        }
        if let Some(current) = self.current.clone() {
            if let Ok(Some(track)) = self.store.get_track(&current.path) {
                if track.artwork_path != current.artwork_path {
                    self.update_current_track(&track);
                }
            }
        }
        // Cached Track rows still carry the old (missing) artwork paths; drop
        // them so the queue and every future view resolve the repaired rows.
        self.track_cache.clear();
        self.refresh_view();
        self.publish_queue(false);
    }

    /// Applies background-decoded artwork. Only the currently visible view is
    /// rebuilt, and only when at least one new image landed.
    pub(crate) fn poll_artwork_images(&mut self) {
        let available = native_io::poll_artwork_images();
        if available.is_empty() {
            return;
        }
        self.publish_queue(false);
        self.refresh_view();
    }

    pub(crate) fn sync_titlebar_theme(
        &self,
        hwnd: isize,
        light: bool,
        custom_color: Option<(u8, u8, u8)>,
    ) {
        let dark_mode: u32 = if custom_color.is_some() || !light {
            1
        } else {
            0
        };
        let caption_color: u32 = if let Some((r, g, b)) = custom_color {
            ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
        } else if light {
            0x00E6EAE7
        } else {
            0x00090A09
        };
        const DWMWA_TEXT_COLOR: u32 = 36;
        let text_color: u32 = if dark_mode == 1 {
            0x00F6F8F7
        } else {
            0x00191D19
        };
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const u32 as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &caption_color as *const u32 as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                &text_color as *const u32 as *const std::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    // ---- The 250 ms clock (port of MainWindow.Playback.Clock_Tick) ---------

    pub(crate) fn tick(&mut self) {
        // Startup runs from the first clock tick, after the window has painted.
        // Doing it before `ui.run()` blocked the first frame behind artwork
        // decoding and library queries.
        if !self.started {
            self.started = true;
            self.startup();
        }
        self.attach_os();

        // Search debouncing (150 ms).
        if let Some((when, query)) = self.search_debounce.clone() {
            if Instant::now() >= when {
                self.search_debounce = None;
                self.set_view("Search", &query, "", "");
            }
        }

        // Scan outcome dismissal after 8s (matching C# MainWindow.WebUi.Events.cs).
        if let Some(t) = self.scan_outcome_time {
            if t.elapsed() >= Duration::from_secs(8) {
                self.scan_outcome_time = None;
                self.ui.global::<State>().set_scan_outcome(ss(""));
                self.ui.global::<State>().set_scan_details_open(false);
            }
        }

        // Secondary-instance activation files.
        for payload in os::poll_activations() {
            if payload.verb == "addtoqueue" {
                self.add_external_files_to_queue(&payload.paths);
            } else if payload.verb == "createplaylist" {
                self.create_playlist_with_external_files(&payload.paths);
            } else {
                self.play_external_files(&payload.paths);
            }
        }
        let mut position = self.engine.position_ms().max(0);
        let duration = self.engine.duration_ms().max(0);

        // A–B repeat: seek back to A once position reaches B.
        if self.engine.is_playing() {
            if let (Some(a), Some(b)) = (
                self.coordinator.queue().repeat_a_ms(),
                self.coordinator.queue().repeat_b_ms(),
            ) {
                if position >= b {
                    let _ = self.engine.seek(a);
                    position = self.engine.position_ms().max(0);
                }
            }
        }

        // A track counts as a play once half of it has been heard (silent background update).
        if let Some(track) = self.current.clone() {
            if self.listening.observe(
                true,
                self.engine.is_playing(),
                position as i64,
                duration as i64,
            ) {
                let path = track.path.clone();
                let _ = self.store.record_play(&path);
            }
        }

        // Native end-of-track event (port of Playback_TrackEnded).
        if self.engine.take_end_reached() {
            self.advance_queue(true);
        }
        if self.engine.take_encountered_error() {
            if let Some(track) = self.current.clone() {
                self.handle_playback_failure(&track);
            }
        }

        let automatic_next = self.coordinator.queue().next_index(true);
        let crossfade_ms = self.coordinator.crossfade_ms();
        let autoplay = self
            .settings
            .bool_or(music_player_core::settings::keys::AUTOPLAY_SIMILAR, true);

        // Autoplay-similar: keep listening when the queue ends.
        if automatic_next.is_none()
            && self.engine.is_playing()
            && self.coordinator.queue().repeat_mode != RepeatMode::Track
            && self.coordinator.queue().repeat_a_ms().is_none()
            && autoplay
            && duration > 0
            && duration.saturating_sub(position) <= 15000.max(crossfade_ms + 3000)
        {
            if let Some(current) = self.current.clone() {
                let recommendations = self.smart_autoplay(&current, 10);
                if !recommendations.is_empty() {
                    self.coordinator
                        .queue_mut()
                        .append(&recommendations, Some(&current.path));
                    self.publish_queue(true);
                }
            }
        }

        // Crossfade window: duration - position <= fadeMs + 1500.
        if self.engine.is_playing()
            && self.coordinator.queue().repeat_mode != RepeatMode::Track
            && self.coordinator.queue().repeat_a_ms().is_none()
            && !self.crossfade_in_progress
            && !self
                .crossfade_failure_source
                .as_deref()
                .map(|p| {
                    self.current
                        .as_ref()
                        .map(|t| t.path.eq_ignore_ascii_case(p))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            && self.coordinator.queue().next_index(true).is_some()
            && crossfade_ms > 0
            && duration > 0
            && duration.saturating_sub(position) <= crossfade_ms + 1500
        {
            if let Some(next) = self.coordinator.queue().next_index(true) {
                self.start_crossfade(next, crossfade_ms);
            }
        }

        // Persist the queue/session every 5 s.
        if self.last_session_save.elapsed() >= Duration::from_secs(5) {
            self.save_session();
            self.last_session_save = Instant::now();
        }

        // Debounced volume persist (250 ms).
        if let Some(volume) = self.pending_volume {
            if self.last_vol_debounce.elapsed() >= Duration::from_millis(250) {
                self.settings.set_raw(
                    music_player_core::settings::keys::VOLUME,
                    &volume.to_string(),
                );
                self.persisted_volume = volume as i64;
                self.pending_volume = None;
            }
        }
        // Debounced equalizer persist (700 ms).
        if self.eq_dirty && self.last_vol_debounce.elapsed() >= Duration::from_millis(700) {
            self.settings
                .set_eq_state(&self.coordinator_eq_name(), self.eq_bands, self.eq_preamp);
            self.eq_dirty = false;
        }

        self.publish_playback();
        self.update_lyrics_position(position);

        // WAL checkpoint when idle > 1 min (port of the same C# branch).
        if !self.engine.is_playing()
            && !self.scan.is_running()
            && self.last_wal.elapsed() >= Duration::from_secs(60)
        {
            self.last_wal = Instant::now();
            let _ = self.store.checkpoint_wal();
        }

        // File watcher + scan status poll. Collect first, then apply without
        // holding two overlapping &mut self borrows in the callbacks.
        let mut removed_paths: Vec<PathBuf> = Vec::new();
        let mut rescan_paths: Vec<(PathBuf, bool)> = Vec::new();
        {
            let mut on_removed = |paths: &[PathBuf]| removed_paths.extend_from_slice(paths);
            let mut on_rescan = |paths: &[PathBuf], full: bool| {
                for path in paths {
                    rescan_paths.push((path.clone(), full));
                }
            };
            self.watcher
                .pump(Instant::now(), Some(&mut on_removed), Some(&mut on_rescan));
        }
        for path in removed_paths {
            self.on_paths_removed(&[path]);
        }
        for (path, full) in rescan_paths {
            self.on_paths_rescan(&[path], full);
        }
        self.poll_scan();
        self.poll_lyrics_search();
        self.poll_update_check();
        self.poll_artwork_repair();
        self.poll_artwork_images();
        self.poll_duplicate_load();
        self.poll_os_commands();
        if self.startup_scan_deferred {
            self.startup_scan_deferred = false;
            self.startup_scan();
            self.check_updates_if_enabled();
        }
    }

    /// Attaches SMTC/taskbar once the Slint window has a native handle. The
    /// handle only exists after the event loop has started.
    pub(crate) fn attach_os(&mut self) {
        if self.os_attached {
            return;
        }
        let hwnd = self
            .ui
            .window()
            .window_handle()
            .window_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                _ => None,
            });
        let Some(hwnd) = hwnd else {
            return;
        };

        self.publish_appearance();

        let Some(os) = &self.os else {
            self.os_attached = true;
            return;
        };
        let smtc = os_shell::attach_main_window(hwnd, os.command_sender());
        if smtc {
            os.disable_media_key_hook();
        }
        self.os_attached = true;
    }

    /// Commands from the system tray / global media keys (Wave 5).
    pub(crate) fn poll_os_commands(&mut self) {
        let Some(os) = &self.os else { return };
        let mut commands = Vec::new();
        while let Some(command) = os.try_command() {
            commands.push(command);
        }
        for command in commands {
            match command {
                os::MediaCommand::Open => {
                    let _ = self.ui.show();
                }
                os::MediaCommand::Previous => {
                    self.cancel_crossfade();
                    let position = self.engine.position_ms();
                    let current = self.current.clone();
                    let resolver = self.resolver();
                    let result = self
                        .coordinator
                        .previous(current.as_ref(), position, &resolver);
                    self.apply_result(result);
                }
                os::MediaCommand::PlayPause => {
                    if self.current.is_none() {
                        let paths = self.view_paths.clone();
                        if let Some(path) = paths.first() {
                            if let Some(track) = self.store.get_track(path).ok().flatten() {
                                self.coordinator.queue_mut().replace(paths, Some(0));
                                self.play_track(track, false, Some(0));
                            }
                        }
                    } else {
                        let current = self.current.clone();
                        let result = if self.engine.is_playing() {
                            self.cancel_crossfade();
                            self.coordinator.pause(current.as_ref())
                        } else {
                            self.coordinator.resume(current.as_ref())
                        };
                        self.apply_result(result);
                    }
                }
                os::MediaCommand::Play => {
                    if self.current.is_none() {
                        let paths = self.view_paths.clone();
                        if let Some(path) = paths.first() {
                            if let Some(track) = self.store.get_track(path).ok().flatten() {
                                self.coordinator.queue_mut().replace(paths, Some(0));
                                self.play_track(track, false, Some(0));
                            }
                        }
                    } else if !self.engine.is_playing() {
                        let current = self.current.clone();
                        let result = self.coordinator.resume(current.as_ref());
                        self.apply_result(result);
                    }
                }
                os::MediaCommand::Pause => {
                    if self.engine.is_playing() {
                        let current = self.current.clone();
                        self.cancel_crossfade();
                        let result = self.coordinator.pause(current.as_ref());
                        self.apply_result(result);
                    }
                }
                os::MediaCommand::Next => self.advance_queue(false),
                os::MediaCommand::Quit => {
                    self.save_session();
                    let _ = self.store.checkpoint_wal();
                    let _ = self.ui.hide();
                }
            }
        }
    }

    // ---- Settings / equalizer ----------------------------------------------

    pub(crate) fn set_setting(&mut self, name: &str, value: &str) {
        let web_name = match name {
            "theme" => "theme",
            "accent-mode" => "accentMode",
            "accent-manual" => "accentManual",
            "accent-color" => "accentColor",
            "selection-color-mode" => "selectionColorMode",
            "selection-color" => "selectionColor",
            "window-transparency" => "windowTransparency",
            "navigation-width" => "navigationWidth",
            "browse-width" => "browseWidth",
            "motion-style" => "motionStyle",
            "transparent-window" => "transparentWindow",
            "hide-exact-duplicates" => "hideDuplicates",
            "autoplay-similar" => "autoplaySimilar",
            "auto-open-side-panel" => "autoOpenPanel",
            "auto-load-lyrics" => "autoLoadLyrics",
            "seek-from-lyrics" => "seekFromLyrics",
            "show-list-artwork" => "showArtwork",
            "show-list-artist" => "showArtist",
            "show-list-album" => "showAlbum",
            "show-list-added" => "showAdded",
            "show-list-year" => "showYear",
            "show-list-duration" => "showDuration",
            "show-list-favorite" => "showFavorite",
            "check-updates" => "checkUpdates",
            "minimize-to-tray" => "minimizeToTray",
            "right-panel-open" => "rightPanelOpen",
            _ => name,
        };
        let json_value = if matches!(value, "true" | "false") {
            json!(value == "true")
        } else if let Ok(n) = value.parse::<i64>() {
            json!(n)
        } else if let Ok(f) = value.parse::<f64>() {
            json!(f.round() as i64)
        } else {
            json!(value)
        };
        match web_name {
            "theme" | "accentMode" | "accentManual" | "accentColor" | "windowTransparency"
            | "selectionColorMode" | "selectionColor" | "navigationWidth" | "browseWidth"
            | "motionStyle" | "transparentWindow" | "hideDuplicates" | "autoplaySimilar"
            | "autoOpenPanel" | "autoLoadLyrics" | "seekFromLyrics" | "showArtwork"
            | "showArtist" | "showAlbum" | "showAdded" | "showYear" | "showDuration"
            | "showFavorite" | "checkUpdates" | "minimizeToTray" | "rightPanelOpen" => {
                if let Err(message) = self.settings.update_web_setting(web_name, &json_value) {
                    self.show_notice(message, NoticeSeverity::Warning);
                }
            }
            _ => self.settings.set_raw(name, value),
        }
        // Keep the live UI in sync with settings without waiting for restart.
        self.publish_settings();
        if name == "check-updates" {
            self.check_updates_if_enabled();
        }
        self.refresh_view();
    }

    /// Port of `MainWindow.WindowsShell.CheckForUpdatesAsync(force: false)`.
    /// The GitHub request runs on a worker thread and is applied by `tick`.
    pub(crate) fn check_updates_if_enabled(&mut self) {
        self.check_updates(false);
    }

    pub(crate) fn check_updates_manual(&mut self) {
        self.check_updates(true);
    }

    pub(crate) fn check_updates(&mut self, force: bool) {
        use music_player_core::settings::keys as k;
        if !force && !self.settings.bool_or(k::CHECK_UPDATES, true) {
            return;
        }
        if self.update_rx.is_some() || (!force && !self.update_check_due()) {
            return;
        }
        let Some(version) = updates::ReleaseVersion::parse(updates::current_version()) else {
            LocalAppLog::shared()
                .warning("updates", "The installed version could not be determined.");
            return;
        };
        let channel = if version.is_prerelease() {
            "preview"
        } else {
            "stable"
        };
        let same_channel = self.settings.raw(k::UPDATE_RELEASE_CHANNEL).as_deref() == Some(channel);
        // Only reuse the cache when it belongs to this release channel.
        let cached = if same_channel && !force {
            updates::read_cached_release(
                self.settings.raw(k::UPDATE_RELEASE_TAG).as_deref(),
                self.settings.raw(k::UPDATE_RELEASE_URL).as_deref(),
            )
        } else {
            None
        };
        let etag = if same_channel && !force {
            self.settings.raw(k::UPDATE_ETAG)
        } else {
            None
        };
        let cached_for_request = cached.clone();
        let (sender, receiver) = mpsc::channel();
        self.update_rx = Some(receiver);
        self.ui.global::<State>().set_update_checking(true);
        std::thread::spawn(move || {
            let result = updates::get_latest(
                version.is_prerelease(),
                etag.as_deref(),
                cached_for_request.as_ref(),
                same_channel,
            );
            let _ = sender.send(UpdateOutcome {
                channel: channel.to_string(),
                result,
            });
        });
    }

    pub(crate) fn update_check_due(&self) -> bool {
        use music_player_core::settings::keys as k;
        let Some(raw) = self.settings.raw(k::LAST_UPDATE_CHECK) else {
            return true;
        };
        let Ok(last) = chrono::DateTime::parse_from_rfc3339(raw.trim()) else {
            return true;
        };
        let elapsed = chrono::Utc::now().signed_duration_since(last.with_timezone(&chrono::Utc));
        elapsed >= chrono::Duration::hours(6)
    }

    pub(crate) fn poll_update_check(&mut self) {
        let Some(receiver) = self.update_rx.take() else {
            return;
        };
        let outcome = match receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.update_rx = Some(receiver);
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ui.global::<State>().set_update_checking(false);
                return;
            }
        };
        self.ui.global::<State>().set_update_checking(false);
        use music_player_core::settings::keys as k;
        match outcome.result {
            Ok(check) => {
                // Only a completed response is recorded. An offline start must
                // not suppress the next attempt for six hours.
                let timestamp =
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                self.settings.set_raw(k::LAST_UPDATE_CHECK, &timestamp);
                self.settings
                    .set_raw(k::UPDATE_ETAG, check.etag.as_deref().unwrap_or(""));
                let tag = check
                    .release
                    .as_ref()
                    .map(|release| release.tag.clone())
                    .unwrap_or_default();
                let url = check
                    .release
                    .as_ref()
                    .map(|release| release.url.clone())
                    .unwrap_or_default();
                self.settings.set_raw(k::UPDATE_RELEASE_TAG, &tag);
                self.settings.set_raw(k::UPDATE_RELEASE_URL, &url);
                self.settings
                    .set_raw(k::UPDATE_RELEASE_CHANNEL, &outcome.channel);
                self.ui.global::<State>().set_update_tag(ss(tag));
            }
            Err(error) => {
                let reason = match error.kind {
                    updates::UpdateErrorKind::RateLimited => {
                        "GitHub is limiting update checks right now."
                    }
                    updates::UpdateErrorKind::Timeout => "The GitHub update check timed out.",
                    updates::UpdateErrorKind::Network => "Could not reach GitHub.",
                    updates::UpdateErrorKind::InvalidData => {
                        "GitHub returned an update response Music Player could not read."
                    }
                };
                LocalAppLog::shared().warning("updates", &format!("{reason} {}", error.message));
            }
        }
    }
}
