use crate::*;

impl App {
    pub(crate) fn load_roots(&mut self) {
        let mut roots = self.settings.library_roots();
        let configured = self.settings.roots_configured();
        if !configured {
            roots = default_roots()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            self.settings.set_library_roots(&roots);
        }
        // Normalize: drop empty and non-existent roots, keep the rest.
        let before = roots.len();
        roots.retain(|r| !r.trim().is_empty() && Path::new(r).is_dir());
        if roots.len() != before {
            self.settings.set_library_roots(&roots);
        }
        self.ui.global::<State>().set_has_roots(!roots.is_empty());
        self.reconfigure_watcher(&roots);
        self.publish_folder_roots(&roots);
    }

    pub(crate) fn reconfigure_watcher(&mut self, roots: &[String]) {
        let paths: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
        let ignored: Vec<PathBuf> = self
            .settings
            .ignored_directories()
            .iter()
            .map(PathBuf::from)
            .collect();
        let extensions = self.extension_set();
        self.watcher.set_roots(&paths, &ignored, Some(&extensions));
    }

    pub(crate) fn extension_set(&self) -> HashSet<String> {
        let mut set: HashSet<String> = music_player_core::AUDIO_EXTENSIONS
            .iter()
            .map(|e| e.to_string())
            .collect();
        for (ext, enabled) in self.settings.extension_enabled() {
            if enabled {
                set.insert(ext.to_lowercase());
            } else {
                set.remove(&ext.to_lowercase());
            }
        }
        for extension in [".mp4", ".m4v", ".mkv", ".avi", ".mov", ".wmv", ".webm"] {
            if self.settings.is_extension_enabled(extension) {
                set.insert(extension.to_string());
            } else {
                set.remove(extension);
            }
        }
        set
    }

    pub(crate) fn video_extensions(&self) -> Vec<String> {
        let defaults = [".mp4", ".m4v", ".mkv", ".avi", ".mov", ".wmv", ".webm"];
        let enabled = self.settings.extension_enabled();
        let kinds = self.settings.extension_types();
        let mut extensions: Vec<String> = defaults.iter().map(|ext| (*ext).to_string()).collect();
        extensions.extend(kinds.iter().filter_map(|(extension, kind)| {
            kind.eq_ignore_ascii_case("Video")
                .then(|| extension.to_ascii_lowercase())
        }));
        extensions.sort();
        extensions.dedup();
        extensions.retain(|extension| enabled.get(extension).copied().unwrap_or(true));
        extensions
    }

    pub(crate) fn startup_scan(&mut self) {
        let roots = self.settings.library_roots();
        if roots.is_empty() {
            // Availability-only reconcile path when no roots are configured.
            // Run in background thread so application startup and UI painting are never blocked.
            let availability = self.availability.clone();
            std::thread::Builder::new()
                .name("startup-reconcile".to_string())
                .spawn(move || {
                    let reconciliation =
                        availability.reconcile_indexed_tracks(&[], &[], None);
                    let _ = availability.clear_unavailable_tracks_that_exist();
                    if reconciliation.unavailable_count > 0 || reconciliation.recovered_count > 0 {
                        LocalAppLog::shared().info(
                            "library-scan",
                            &format!(
                                "Startup reconcile: {} unavailable, {} recovered",
                                reconciliation.unavailable_count, reconciliation.recovered_count
                            ),
                        );
                    }
                })
                .ok();
            return;
        }

        // Always initiate background directory discovery on every startup (force: false).
        // Uses fast file fingerprinting (size + mtime + artwork check) to skip unchanged files
        // without re-reading metadata. Newly added, modified, deleted, and renamed files
        // are detected in the background without blocking the UI. The 12-hour threshold
        // never suppresses discovery of new files.
        self.start_scan(&roots, false);
    }

    pub(crate) fn start_scan(&mut self, roots: &[String], force: bool) {
        let paths: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
        let ignored: Vec<PathBuf> = self
            .settings
            .ignored_directories()
            .iter()
            .map(PathBuf::from)
            .collect();
        let (tx, rx) = mpsc::channel();
        let extensions = self.extension_set();
        let started = self
            .scan
            .start(&paths, &ignored, force, Some(&extensions), Some(tx));
        if started {
            self.scan_progress_rx = Some(rx);
            self.settings.set_raw(
                music_player_core::settings::keys::LIBRARY_LAST_SCAN_UTC,
                &chrono::Utc::now().to_rfc3339(),
            );
            self.ui.global::<State>().set_scan_outcome(ss(""));
        } else {
            self.show_notice("Scan is already running.", NoticeSeverity::Informational);
        }
        self.publish_scan();
    }

    pub(crate) fn poll_scan(&mut self) {
        let progress: Vec<ScanProgress> = match &self.scan_progress_rx {
            Some(rx) => {
                let mut items = Vec::new();
                while let Ok(item) = rx.try_recv() {
                    items.push(item);
                }
                items
            }
            None => Vec::new(),
        };
        for item in &progress {
            self.apply_scan_progress(item);
        }
        if let Some(result) = self.scan.poll_result() {
            self.scan_progress_rx = None;
            match result {
                Ok(index) => self.finish_scan(&index),
                Err(message) => {
                    self.ui
                        .global::<State>()
                        .set_scan_outcome(ss(format!("Scan failed: {}", message)));
                    self.show_notice(message, NoticeSeverity::Error);
                }
            }
        }
        self.publish_scan();
    }

    pub(crate) fn apply_scan_progress(&mut self, progress: &ScanProgress) {
        let state = self.ui.global::<State>();
        state.set_scan_scanned(progress.files_found as i32);
        state.set_library_tracks(progress.indexed_tracks.max(0) as i32);
        if !progress.current_path.is_empty() {
            state.set_scan_path(ss(&progress.current_path));
        }
    }

    pub(crate) fn finish_scan(&mut self, index: &IndexResult) {
        let message = if index.removed > 0 {
            format!(
                "Library scan complete: {} added, {} removed",
                index.indexed, index.removed
            )
        } else {
            format!("Library scan complete: {} tracks indexed", index.indexed)
        };
        self.ui.global::<State>().set_scan_outcome(ss(message));
        self.scan_outcome_time = Some(Instant::now());
        self.settings.set_raw(
            music_player_core::settings::keys::LIBRARY_LAST_SCAN_UTC,
            &chrono::Utc::now().to_rfc3339(),
        );
        if index.indexed > 0 || index.removed > 0 {
            self.track_cache.clear();
            self.invalidate_view_caches();
            self.load_roots();
            self.refresh_playlists();
            self.refresh_view();
        }
    }

    pub(crate) fn on_paths_removed(&mut self, paths: &[PathBuf]) {
        let strings: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let _ = self.store.purge_deleted_tracks(&strings);
        self.invalidate_view_caches();
        self.coordinator.queue_mut().remove_paths(&strings);
        self.save_session();
        self.publish_queue(true);
        self.refresh_view();
    }

    pub(crate) fn on_paths_rescan(&mut self, paths: &[PathBuf], _full: bool) {
        let roots: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        self.start_scan(&roots, false);
    }

    // ---- Helpers for native commands ---------------------------------------

    pub(crate) fn add_root(&mut self, path: &str) {
        if path.trim().is_empty() || !Path::new(path).is_dir() {
            return;
        }
        let mut roots = self.settings.library_roots();
        if !roots.iter().any(|r| r.eq_ignore_ascii_case(path)) {
            roots.push(path.to_string());
            self.settings.set_library_roots(&roots);
        }
        self.load_roots();
        self.start_scan(&roots, false);
    }
}
