//! Music Player — Slint desktop host.
//!
//! Port of the C# `MainWindow.*` orchestration layer. The engine room runs on
//! the UI thread behind a 250 ms timer (`Clock_Tick`), exactly like the
//! original DispatcherTimer loop. The `core` crate remains headless.
#![allow(deprecated)]

use chrono::Timelike;
use music_player_core::audio::coordinator::{
    Notice, NoticeSeverity, PlaybackCommandKind, PlaybackCommandResult,
};
use music_player_core::audio::engine::CrossfadeOutcome;
use music_player_core::audio::equalizer::NEUTRAL_EQUALIZER_PREAMP;
use music_player_core::audio::listening::PlaybackListeningTracker;
use music_player_core::audio::PlaybackCoordinator;
use music_player_core::lyrics::lrclib::{LrclibSearchClient, LyricsSearchResult};
use music_player_core::metadata::tag_reader::{read_additional_standard_fields, read_custom_fields};
use music_player_core::scanner::scan::default_roots;
use music_player_core::metadata::tag_backups_dir;
use music_player_core::{
    artwork_cache_dir, format_lrc, inspect_file, parse_lrc,
    read_sidecar, write_sidecar, ExactDuplicateGroup,
    IndexResult, LibraryFileWatcher, LibraryScanCoordinator, LibraryStats, LibraryStore,
    LocalAppLog,
    PlaybackSession, PlayerSettings, PlaylistSummary, QueueCoordinator,
    RepeatMode, ScanProgress, TagEdit, TagEditor, Track, TrackAvailabilityService, TrackQuery,
    TrackSort, EQUALIZER_BANDS,
};
use serde_json::json;
use slint::{ComponentHandle, Image, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

slint::include_modules!();

// ---------------------------------------------------------------------------
// Helpers (WebUI DTO formatting, faithful to the original JS/C# display code)
// ---------------------------------------------------------------------------

fn fmt_time(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

fn fmt_added(epoch_secs: u64) -> String {
    if epoch_secs == 0 {
        return String::new();
    }
    // Compact ISO date — the WebUI shows a short relative date; the exact
    // wording is supplied by the toast layer and is not a behavioral surface.
    let days = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(epoch_secs)
        .saturating_sub(epoch_secs))
        / 86400;
    match days {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        d if d < 7 => format!("{} days ago", d),
        _ => {
            let secs = epoch_secs as i64;
            let date = chrono_fallback_date(secs);
            date
        }
    }
}

fn chrono_fallback_date(secs: i64) -> String {
    // chrono is already a transitive core dependency and is available here.
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%b %d, %Y").to_string())
        .unwrap_or_default()
}

fn initials(text: &str) -> String {
    let mut out = String::new();
    for word in text.split_whitespace().take(2) {
        if let Some(ch) = word.chars().find(|c| c.is_alphanumeric()) {
            out.extend(ch.to_uppercase());
        }
    }
    if out.is_empty() {
        "?".to_string()
    } else {
        out
    }
}

/// FNV-1a slot 1..=10, mirroring the WebUI `gradientClass()` helper.
fn gradient_slot(name: &str) -> i32 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in name.to_ascii_lowercase().bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash % 10) as i32 + 1
}

fn badge_color(slot: i32) -> slint::Color {
    let (r, g, b) = match slot {
        2 => (0x21, 0x33, 0x37),
        3 => (0x38, 0x27, 0x2b),
        4 => (0x24, 0x33, 0x28),
        5 => (0x35, 0x30, 0x25),
        6 => (0x2a, 0x2a, 0x32),
        7 => (0x2e, 0x2f, 0x36),
        8 => (0x3e, 0x21, 0x40),
        9 => (0x1b, 0x39, 0x3b),
        10 => (0x40, 0x36, 0x23),
        _ => (0x2a, 0x2f, 0x3f),
    };
    slint::Color::from_rgb_u8(r, g, b)
}

fn parse_hex_color(s: &str) -> slint::Color {
    let s = s.trim().trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(183);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(45);
        slint::Color::from_rgb_u8(r, g, b)
    } else {
        slint::Color::from_rgb_u8(183, 255, 45)
    }
}

fn ss(s: impl Into<String>) -> SharedString {
    SharedString::from(s.into())
}

fn ui_model<T: Clone + 'static>(items: Vec<T>) -> Rc<VecModel<T>> {
    Rc::new(VecModel::from(items))
}

fn song_sort(name: &str, default: TrackSort) -> TrackSort {
    match name {
        "Title" => TrackSort::Title,
        "Artist" => TrackSort::Artist,
        "Album" => TrackSort::Album,
        "Genre" => TrackSort::Genre,
        "Year" => TrackSort::Year,
        "Added" => TrackSort::Added,
        "Duration" => TrackSort::Duration,
        "PlayCount" | "Plays" => TrackSort::PlayCount,
        "LastPlayed" => TrackSort::LastPlayed,
        "TrackNumber" | "Track" => TrackSort::TrackNumber,
        "Rating" => TrackSort::Rating,
        "Path" => TrackSort::Path,
        _ => default,
    }
}

fn default_sort_for_view(view: &str) -> (String, bool) {
    match view {
        "Most Played" => ("PlayCount".into(), true),
        "Recently Played" => ("LastPlayed".into(), true),
        "Recently Added" => ("Added".into(), true),
        _ => ("Title".into(), false),
    }
}

fn detail_sort_for_column(column: &str) -> (String, bool) {
    match column {
        "album" => ("TrackNumber".into(), false),
        "artist" => ("PlayCount".into(), true),
        _ => ("Title".into(), false),
    }
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name(path))
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Native dialogs without new crates (the original used WinRT pickers). The
/// project already shells out to PowerShell for LRCLIB, so this keeps the
/// offline dependency set fixed while preserving native picker behavior.
fn powershell(script: &str) -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn pick_folder() -> Option<String> {
    powershell(
        "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description='Select music folder'; if($d.ShowDialog() -eq 'OK'){ Write-Output $d.SelectedPath }",
    )
}

fn pick_file(filter: &str) -> Option<String> {
    powershell(&format!(
        "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='{}'; if($d.ShowDialog() -eq 'OK'){{ Write-Output $d.FileName }}",
        ps_quote(filter)
    ))
}

fn save_file(default_name: &str, filter: &str) -> Option<String> {
    powershell(&format!(
        "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.SaveFileDialog; $d.FileName='{}'; $d.Filter='{}'; if($d.ShowDialog() -eq 'OK'){{ Write-Output $d.FileName }}",
        ps_quote(default_name),
        ps_quote(filter)
    ))
}

fn open_in_explorer(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,\"{}\"", path))
        .spawn();
}

fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

fn open_url(url: &str) {
    // Allowlist: only the project's GitHub release pages are opened.
    if !url.starts_with("https://github.com/kalabhaftu/music-player") {
        return;
    }
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("Start-Process '{}'", ps_quote(url)),
        ])
        .spawn();
}

fn set_clipboard(text: &str) {
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("Set-Clipboard -Value '{}'", ps_quote(text)),
        ])
        .spawn();
}

#[derive(Clone)]
struct NavContext {
    view: String,
    search: String,
    group_column: String,
    group_value: String,
    playlist_id: String,
}

impl NavContext {
    fn snapshot(state: &State<'_>) -> Self {
        Self {
            view: state.get_view().to_string(),
            search: state.get_view_search().to_string(),
            group_column: state.get_view_group_column().to_string(),
            group_value: state.get_view_group_value().to_string(),
            playlist_id: state.get_view_playlist_id().to_string(),
        }
    }
    fn restore(&self, state: &State<'_>) {
        state.set_view(ss(&self.view));
        state.set_view_search(ss(&self.search));
        state.set_view_group_column(ss(&self.group_column));
        state.set_view_group_value(ss(&self.group_value));
        state.set_view_playlist_id(ss(&self.playlist_id));
    }
}

#[derive(Clone)]
#[allow(dead_code)]
enum ConfirmAction {
    RemoveRoot(String),
    DeletePlaylist(String),
    RestoreBackup(String),
    ClearQueue,
    RemoveLyrics,
    Reset(String),
}

struct App {
    ui: AppWindow,
    store: LibraryStore,
    settings: PlayerSettings,
    engine: music_player_core::AudioEngine,
    coordinator: PlaybackCoordinator,
    listening: PlaybackListeningTracker,
    scan: LibraryScanCoordinator,
    watcher: LibraryFileWatcher,
    availability: TrackAvailabilityService,
    scan_progress_rx: Option<Receiver<ScanProgress>>,
    lyrics_rx: Option<Receiver<std::result::Result<Vec<LyricsSearchResult>, String>>>,
    lyrics_results: Vec<LyricsSearchResult>,
    notice_timer: Timer,
    current: Option<Track>,
    id_to_path: HashMap<String, String>,
    view_paths: Vec<String>,
    history: Vec<NavContext>,
    history_pos: usize,
    crossfade_in_progress: bool,
    crossfade_source_index: Option<usize>,
    crossfade_failure_source: Option<String>,
    volume: u32,
    persisted_volume: i64,
    pending_volume: Option<u32>,
    last_session_save: Instant,
    last_wal: Instant,
    last_vol_debounce: Instant,
    eq_bands: [f32; 10],
    eq_preamp: f32,
    eq_dirty: bool,
    tracks_offset: usize,
    groups_offset: usize,
    group_total: i64,
    dup_sort: String,
    dup_desc: bool,
    dup_page: usize,
    details_target: Option<String>,
    tags_target: Option<String>,
    tags_additional: Vec<(String, String)>,
    tags_custom: Vec<(String, String)>,
    lyrics_target: Option<String>,
    lyrics_doc: Option<music_player_core::LyricsDocument>,
    lyrics_timed: bool,
    lyrics_active: i32,
    edit_target: Option<String>,
    context_target: Option<String>,
    confirm_action: Option<ConfirmAction>,
    add_playlist_track: Option<String>,
    playlists: Vec<PlaylistSummary>,
    log_folder: PathBuf,
}

impl App {
    fn new(ui: AppWindow) -> Self {
        let store = LibraryStore::in_app_data().expect("could not open the library database");
        let settings = PlayerSettings::new(store.clone());
        let scan = LibraryScanCoordinator::new(&store).with_artwork_cache(artwork_cache_dir());
        let log_folder = LocalAppLog::shared().folder().to_path_buf();
        let persisted_volume = settings.int_or(music_player_core::settings::keys::VOLUME, 75);
        let volume = persisted_volume.clamp(0, 100) as u32;
        let crossfade_ms = settings.crossfade_seconds() as u64 * 1000;
        let coordinator = PlaybackCoordinator::with_crossfade_ms(QueueCoordinator::new(), crossfade_ms);
        let eq_bands = settings.eq_bands().unwrap_or([0.0; 10]);
        let eq_preamp = settings.eq_preamp().unwrap_or(NEUTRAL_EQUALIZER_PREAMP);
        let now = Instant::now();
        Self {
            ui,
            availability: TrackAvailabilityService::new(&store).with_artwork_directory(artwork_cache_dir()),
            store,
            settings,
            engine: music_player_core::AudioEngine::new(),
            coordinator,
            listening: PlaybackListeningTracker::new(),
            scan,
            watcher: LibraryFileWatcher::new(),
            scan_progress_rx: None,
            lyrics_rx: None,
            lyrics_results: Vec::new(),
            notice_timer: Timer::default(),
            current: None,
            id_to_path: HashMap::new(),
            view_paths: Vec::new(),
            history: Vec::new(),
            history_pos: 0,
            crossfade_in_progress: false,
            crossfade_source_index: None,
            crossfade_failure_source: None,
            volume,
            persisted_volume,
            pending_volume: None,
            last_session_save: now,
            last_wal: now,
            last_vol_debounce: now,
            eq_bands,
            eq_preamp,
            eq_dirty: false,
            tracks_offset: 0,
            groups_offset: 0,
            group_total: 0,
            dup_sort: "title".into(),
            dup_desc: false,
            dup_page: 0,
            details_target: None,
            tags_target: None,
            tags_additional: Vec::new(),
            tags_custom: Vec::new(),
            lyrics_target: None,
            lyrics_doc: None,
            lyrics_timed: false,
            lyrics_active: -1,
            edit_target: None,
            context_target: None,
            confirm_action: None,
            add_playlist_track: None,
            playlists: Vec::new(),
            log_folder,
        }
    }

    // ---- Startup (port of MainWindow.LibraryScan.StartStartupScanAsync) ---

    fn startup(&mut self) {
        LocalAppLog::shared().info("startup", "Music Player starting");
        self.publish_settings();
        self.refresh_playlists();
        self.load_roots();
        self.restore_session();
        self.startup_scan();
        self.refresh_view();
        self.publish_playback();
        self.publish_queue(true);
        self.publish_scan();
        self.publish_about();
        self.check_updates_if_enabled();
    }

    fn load_roots(&mut self) {
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
        self.reconfigure_watcher(&roots);
        self.publish_folder_roots(&roots);
    }

    fn reconfigure_watcher(&mut self, roots: &[String]) {
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

    fn extension_set(&self) -> HashSet<String> {
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
        set
    }

    fn startup_scan(&mut self) {
        let roots = self.settings.library_roots();
        let recent = self
            .settings
            .raw(music_player_core::settings::keys::LIBRARY_LAST_SCAN_UTC)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|t| {
                chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc))
                    < chrono::Duration::hours(12)
            })
            .unwrap_or(false);
        if recent {
            // Availability-only reconcile path (< 12 h since the last scan).
            let root_refs = roots.clone();
            let reconciliation = self
                .availability
                .reconcile_indexed_tracks(&[], &[], Some(&root_refs));
            let _ = self
                .availability
                .clear_unavailable_tracks_that_exist();
            if reconciliation.unavailable_count > 0 || reconciliation.recovered_count > 0 {
                LocalAppLog::shared().info(
                    "library-scan",
                    &format!(
                        "Startup reconcile: {} unavailable, {} recovered",
                        reconciliation.unavailable_count, reconciliation.recovered_count
                    ),
                );
            }
        } else {
            self.start_scan(&roots, false);
        }
    }

    fn start_scan(&mut self, roots: &[String], force: bool) {
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

    // ---- The 250 ms clock (port of MainWindow.Playback.Clock_Tick) ---------

    fn tick(&mut self) {
        let position = self.engine.position_ms().max(0);
        let duration = self.engine.duration_ms().max(0);

        // A–B repeat: seek back to A once position reaches B.
        if self.engine.is_playing() {
            if let (Some(a), Some(b)) = (
                self.coordinator.queue().repeat_a_ms(),
                self.coordinator.queue().repeat_b_ms(),
            ) {
                if position >= b {
                    let _ = self.engine.seek(a);
                }
            }
        }

        // A track counts as a play once half of it has been heard.
        if let Some(track) = self.current.clone() {
            if self
                .listening
                .observe(true, self.engine.is_playing(), position as i64, duration as i64)
            {
                let path = track.path.clone();
                let _ = self.store.record_play(&path);
                self.show_notice(
                    format!("Play count recorded for {}.", track.title),
                    NoticeSeverity::Informational,
                );
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
        let autoplay = self.settings.bool_or(music_player_core::settings::keys::AUTOPLAY_SIMILAR, true);

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
                self.settings
                    .set_raw(music_player_core::settings::keys::VOLUME, &volume.to_string());
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
    }

    fn poll_scan(&mut self) {
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

    fn apply_scan_progress(&mut self, progress: &ScanProgress) {
        let state = self.ui.global::<State>();
        state.set_scan_scanned(progress.files_found as i32);
        state.set_library_tracks(progress.indexed_tracks.max(0) as i32);
        if !progress.current_path.is_empty() {
            state.set_scan_path(ss(&progress.current_path));
        }
    }

    fn finish_scan(&mut self, index: &IndexResult) {
        let message = if index.removed > 0 {
            format!(
                "Library scan complete: {} added, {} removed",
                index.indexed, index.removed
            )
        } else {
            format!("Library scan complete: {} tracks indexed", index.indexed)
        };
        self.ui.global::<State>().set_scan_outcome(ss(message));
        self.settings
            .set_raw(music_player_core::settings::keys::LIBRARY_LAST_SCAN_UTC, &chrono::Utc::now().to_rfc3339());
        self.load_roots();
        self.refresh_playlists();
        self.refresh_view();
    }

    fn on_paths_removed(&mut self, paths: &[PathBuf]) {
        let strings: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let _ = self.store.purge_deleted_tracks(&strings);
        self.coordinator.queue_mut().remove_paths(&strings);
        self.save_session();
        self.publish_queue(true);
        self.refresh_view();
    }

    fn on_paths_rescan(&mut self, paths: &[PathBuf], _full: bool) {
        let roots: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        self.start_scan(&roots, false);
    }

    fn smart_autoplay(&self, track: &Track, limit: usize) -> Vec<String> {
        let in_queue: HashSet<String> = self
            .coordinator
            .queue()
            .entries()
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let mut candidates: Vec<Track> = Vec::new();
        for (genre_mode, value) in [(true, &track.genre), (false, &track.artist)] {
            if value.trim().is_empty() {
                continue;
            }
            let query = TrackQuery {
                group_column: if genre_mode { Some("genre".into()) } else { Some("artist".into()) },
                group_value: Some(value.clone()),
                sort: TrackSort::PlayCount,
                descending: true,
                page_size: 64,
                ..TrackQuery::default()
            };
            if let Ok((tracks, _)) = self.store.query_track_page(&query) {
                for t in tracks {
                    if t.path != track.path && !in_queue.contains(&t.path.to_lowercase()) {
                        candidates.push(t);
                    }
                }
            }
            if candidates.len() >= limit {
                break;
            }
        }
        candidates
            .into_iter()
            .take(limit)
            .map(|t| t.path)
            .collect()
    }

    // ---- Playback command surface (port of PlaybackService orchestration) --

    fn resolver(&self) -> impl Fn(&str) -> Option<Track> {
        let store = self.store.clone();
        move |path| store.get_track(path).ok().flatten()
    }

    fn track_by_id(&self, id: &str) -> Option<Track> {
        let path = self.id_to_path.get(id)?.clone();
        self.store.get_track(&path).ok().flatten()
    }

    fn play_track(&mut self, track: Track, reset_queue: bool, index: Option<usize>) {
        let resolver = self.resolver();
        let result = self.coordinator.start_track(
            &track,
            reset_queue,
            index,
            &self.view_paths,
            &resolver,
        );
        self.apply_result(result);
    }

    fn apply_result(&mut self, result: PlaybackCommandResult) {
        use music_player_core::audio::coordinator::PlaybackCommandKind as K;
        match result.kind {
            K::NoChange => {}
            K::TrackStarted => {
                let Some(path) = result.track_path.clone() else { return };
                if self.availability.mark_available(&path) {
                    self.refresh_view();
                }
                if let Err(err) = self.engine.play_track(&path) {
                    self.show_notice(
                        format!("Could not play {}. See Settings → Open log folder for details.", file_name(&path)),
                        NoticeSeverity::Error,
                    );
                    LocalAppLog::shared().error("playback", "play_track failed", Some(&err));
                    return;
                }
                self.listening.reset_for_track();
                self.crossfade_in_progress = false;
                self.crossfade_failure_source = None;
                self.crossfade_source_index = None;
                if let Some(track) = self.store.get_track(&path).ok().flatten() {
                    self.update_current_track(&track);
                }
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
                self.listening.mark_resumed(self.engine.position_ms() as i64);
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
                                LocalAppLog::shared().error("playback", "crossfade fallback failed", Some(&err));
                            } else if let Some(track) = self.store.get_track(&path).ok().flatten() {
                                self.update_current_track(&track);
                                self.listening.reset_for_track();
                            }
                        }
                        CrossfadeOutcome::Failed(message) => {
                            self.crossfade_in_progress = false;
                            self.crossfade_failure_source = self.current.as_ref().map(|t| t.path.clone());
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

    fn reconcile_missing(&mut self, path: &str) -> bool {
        let exists_in_expected_root = self
            .settings
            .library_roots()
            .iter()
            .any(|root| Path::new(root).join(path.trim_start_matches(root)).exists());
        if exists_in_expected_root {
            self.availability.mark_unavailable(path);
            false
        } else {
            let _ = self.store.purge_deleted_tracks(&[path.to_string()]);
            self.coordinator.queue_mut().remove_paths(&[path.to_string()]);
            self.refresh_view();
            true
        }
    }

    fn handle_playback_failure(&mut self, track: &Track) {
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
                format!("Could not play {}. See Settings → Open log folder for details.", track.title),
                NoticeSeverity::Error,
            );
        }
    }

    fn cancel_crossfade(&mut self) {
        if !self.crossfade_in_progress {
            return;
        }
        if self.engine.has_committed_crossfade() {
            self.crossfade_in_progress = false;
            self.crossfade_source_index = None;
            self.crossfade_failure_source = None;
            self.listening.reset_for_track();
            if let Some(path) = self.current.as_ref().map(|t| t.path.clone()) {
                if let Some(track) = self.store.get_track(&path).ok().flatten() {
                    self.update_current_track(&track);
                }
            }
            return;
        }
        self.engine.cancel_crossfade();
        self.crossfade_in_progress = false;
        self.restore_crossfade_queue_index();
    }

    fn restore_crossfade_queue_index(&mut self) {
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

    fn start_crossfade(&mut self, target_index: usize, duration_ms: u64) {
        let resolver = self.resolver();
        let source = if self.crossfade_in_progress {
            self.crossfade_source_index
        } else {
            None
        };
        let result = self.coordinator.prepare_crossfade(target_index, duration_ms, source, &resolver);
        self.apply_result(result);
    }

    fn advance_queue(&mut self, automatic: bool) {
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
        let autoplay = self.settings.bool_or(music_player_core::settings::keys::AUTOPLAY_SIMILAR, true);
        let fallback = if autoplay {
            self.current
                .as_ref()
                .and_then(|t| self.smart_autoplay(t, 10).into_iter().next())
                .map(|first| vec![first])
        } else {
            None
        };
        let resolver = self.resolver();
        let result = self.coordinator.advance_with_options(
            automatic,
            blocked,
            source,
            &resolver,
            fallback,
        );
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
                    let result = self.coordinator.advance_with_options(true, false, None, &resolver, None);
                    self.apply_result(result);
                    return;
                }
            }
        }
        self.apply_result(result);
    }

    fn save_session(&mut self) {
        let session = PlaybackSession {
            track_path: self.current.as_ref().map(|t| t.path.clone()),
            position_ms: self.engine.position_ms(),
            queue: self.coordinator.queue().entries().to_vec(),
            shuffle: self.coordinator.queue().shuffle,
            repeat_mode: self.coordinator.queue().repeat_mode.as_str().to_string(),
            repeat_a_ms: self.coordinator.queue().repeat_a_ms(),
            repeat_b_ms: self.coordinator.queue().repeat_b_ms(),
            queue_index: self.coordinator.queue().current_index().map(|i| i as i32).unwrap_or(-1),
        };
        let _ = self.store.save_session(&session);
    }

    fn restore_session(&mut self) {
        let Some(mut session) = self.store.load_session().ok().flatten() else { return };
        session.queue.retain(|p| {
            if Path::new(p).exists() {
                true
            } else {
                // Purge files confirmed deleted; keep temporarily unavailable ones.
                let parent_exists = Path::new(p)
                    .parent()
                    .map(|d| d.exists())
                    .unwrap_or(false);
                if parent_exists {
                    let _ = self.store.purge_deleted_tracks(&[p.clone()]);
                    false
                } else {
                    false
                }
            }
        });
        let current = session
            .track_path
            .as_ref()
            .filter(|p| Path::new(p).exists())
            .and_then(|p| self.store.get_track(p).ok().flatten());
        self.coordinator
            .queue_mut()
            .replace(session.queue, if session.queue_index >= 0 { Some(session.queue_index as usize) } else { None });
        self.coordinator.queue_mut().set_shuffle(session.shuffle);
        self.coordinator
            .queue_mut()
            .set_repeat_mode(RepeatMode::from_str_lenient(&session.repeat_mode));
        self.coordinator
            .queue_mut()
            .set_ab_repeat_points(session.repeat_a_ms, session.repeat_b_ms);
        if let Some(track) = current {
            if self.engine.play_track(&track.path).is_ok() {
                let _ = self.engine.pause();
                let _ = self.engine.seek(session.position_ms);
                self.update_current_track(&track);
                self.listening.restore_paused(session.position_ms as i64);
            }
        }
    }

    // ---- State publication --------------------------------------------------

    fn publish_playback(&mut self) {
        let position = self.engine.position_ms();
        let duration = self.engine.duration_ms();
        let progress = if duration > 0 {
            (position as f32 / duration as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let state = self.ui.global::<State>();
        state.set_is_playing(self.engine.is_playing());
        state.set_progress(progress);
        state.set_current_time(ss(fmt_time(position)));
        state.set_total_time(ss(fmt_time(duration)));
        state.set_has_track(self.current.is_some());
        state.set_crossfading(self.crossfade_in_progress);
        state.set_shuffle(self.coordinator.queue().shuffle);
        state.set_repeat_mode(ss(self.coordinator.queue().repeat_mode.as_str()));
        state.set_volume(self.volume as i32);
        if let Some(track) = &self.current {
            state.set_repeat_a(ss(
                self.coordinator
                    .queue()
                    .repeat_a_ms()
                    .map(fmt_time)
                    .unwrap_or_default(),
            ));
            state.set_repeat_b(ss(
                self.coordinator
                    .queue()
                    .repeat_b_ms()
                    .map(fmt_time)
                    .unwrap_or_default(),
            ));
            let _ = track;
        } else {
            state.set_repeat_a(ss(""));
            state.set_repeat_b(ss(""));
        }
    }

    fn update_current_track(&mut self, track: &Track) {
        self.current = Some(track.clone());
        self.id_to_path.insert(track.id(), track.path.clone());
        let state = self.ui.global::<State>();
        state.set_current_id(ss(track.id()));
        state.set_current_title(ss(&track.title));
        state.set_current_artist(ss(&track.artist));
        state.set_current_album(ss(&track.album));
        state.set_current_favorite(track.favorite);
        let cover = track
            .artwork_path
            .as_deref()
            .and_then(|p| Image::load_from_path(Path::new(p)).ok())
            .unwrap_or_default();
        state.set_current_cover(cover);
        let accent = track
            .artwork_path
            .as_deref()
            .map(|p| music_player_core::extract_palette(Path::new(p)))
            .map(|p| p.accent)
            .map(|c| slint::Color::from_rgb_u8(c.r, c.g, c.b))
            .unwrap_or_else(|| parse_hex_color("#b7ff2d"));
        state.set_accent(accent);
        self.publish_info(track);
        self.load_lyrics(track);
    }

    fn publish_info(&mut self, track: &Track) {
        let details = inspect_file(Path::new(&track.path));
        let state = self.ui.global::<State>();
        state.set_info_album(ss(&track.album));
        state.set_info_album_artist(ss(&track.album_artist));
        state.set_info_year_genre(ss(format!(
            "{} · {}",
            if track.year > 0 { track.year.to_string() } else { "—".into() },
            if track.genre.is_empty() { "—".into() } else { track.genre.clone() }
        )));
        state.set_info_format(ss(
            details
                .as_ref()
                .map(|d| d.container.clone())
                .unwrap_or_else(|| file_name(&track.path).rsplit('.').next().unwrap_or("").to_uppercase()),
        ));
        state.set_info_status(ss(if self.engine.is_playing() { "Playing" } else { "Paused" }));
        state.set_info_quality(ss(
            details
                .as_ref()
                .map(|d| format!("{} kbps · {} Hz · {} bit", d.bitrate_kbps, d.sample_rate_hz, d.bits_per_sample))
                .unwrap_or_default(),
        ));
        state.set_info_location(ss(&track.path));
    }

    fn track_to_ui(&mut self, track: &Track, _index: i32, playing_path: &str) -> TrackItem {
        self.id_to_path.insert(track.id(), track.path.clone());
        let slot = gradient_slot(if track.album.is_empty() { &track.title } else { &track.album });
        let added = chrono::DateTime::from_timestamp(track.added_utc.timestamp(), 0)
            .map(|dt| dt.timestamp() as u64)
            .unwrap_or(0);
        TrackItem {
            id: ss(track.id()),
            title: ss(&track.title),
            artist: ss(&track.artist),
            album: ss(&track.album),
            album_artist: ss(&track.album_artist),
            genre: ss(&track.genre),
            year: ss(if track.year > 0 { track.year.to_string() } else { String::new() }),
            duration: ss(fmt_time(track.duration_ms)),
            added: ss(fmt_added(added)),
            initials: ss(initials(&track.title)),
            badge_bg: badge_color(slot),
            is_favorite: track.favorite,
            rating: track.rating as i32,
            play_count: track.play_count as i32,
            is_playing: track.path.eq_ignore_ascii_case(playing_path),
            unavailable: track.unavailable,
        }
    }

    fn queue_item(&mut self, track: &Track, is_current: bool, playing_path: &str) -> QueueItem {
        self.id_to_path.insert(track.id(), track.path.clone());
        let slot = gradient_slot(&track.title);
        QueueItem {
            id: ss(track.id()),
            title: ss(&track.title),
            artist: ss(&track.artist),
            duration: ss(fmt_time(track.duration_ms)),
            initials: ss(initials(&track.title)),
            badge_bg: badge_color(slot),
            is_current,
            unavailable: track.unavailable,
            is_playing: track.path.eq_ignore_ascii_case(playing_path),
        }
    }

    /// Placeholder DTO from the WebUI contract for paths whose library row is gone.
    fn placeholder_item(&mut self, path: &str, playing_path: &str) -> TrackItem {
        let id = music_player_core::track_id_for_path(path);
        self.id_to_path.insert(id.clone(), path.to_string());
        TrackItem {
            id: ss(&id),
            title: ss(file_stem(path)),
            artist: ss("Unavailable"),
            album: ss(""),
            album_artist: ss(""),
            genre: ss(""),
            year: ss(""),
            duration: ss("0:00"),
            added: ss("Unavailable"),
            initials: ss(initials(&file_stem(path))),
            badge_bg: badge_color(3),
            is_favorite: false,
            rating: 0,
            play_count: 0,
            is_playing: path.eq_ignore_ascii_case(playing_path),
            unavailable: true,
        }
    }

    fn publish_queue(&mut self, force: bool) {
        let entries = self.coordinator.queue().entries().to_vec();
        let current_index = self.coordinator.queue().current_index();
        let start = current_index.unwrap_or(0);
        let playing_path = self.current.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        let mut items = Vec::new();
        for path in entries.iter().skip(start).take(30) {
            let item = match self.store.get_track(path).ok().flatten() {
                Some(track) => self.queue_item(&track, false, &playing_path),
                None => {
                    let id = music_player_core::track_id_for_path(path);
                    self.id_to_path.insert(id.clone(), path.clone());
                    QueueItem {
                        id: ss(&id),
                        title: ss(file_stem(path)),
                        artist: ss("Unavailable"),
                        duration: ss("0:00"),
                        initials: ss(initials(&file_stem(path))),
                        badge_bg: badge_color(3),
                        is_current: false,
                        unavailable: true,
                        is_playing: false,
                    }
                }
            };
            items.push(item);
        }
        // Mark the current entry even when the window starts past it.
        if let Some(idx) = current_index {
            if idx < start && idx < items.len() {
                items[idx].is_current = true;
            }
        }
        let more = entries.len().saturating_sub(start + 30);
        let state = self.ui.global::<State>();
        state.set_queue_items(ui_model(items).into());
        state.set_queue_count(entries.len() as i32);
        state.set_queue_more_count(more as i32);
        let _ = force;
    }

    fn publish_scan(&mut self) {
        let status = self.scan.get_status();
        let state = self.ui.global::<State>();
        state.set_scan_active(status.active);
        state.set_scan_paused(status.paused);
        state.set_scan_cancelling(status.cancelling);
        if !status.active {
            let stats = self.store.get_library_stats().unwrap_or(LibraryStats { total_tracks: 0, total_bytes: 0 });
            state.set_library_tracks(stats.total_tracks as i32);
        }
    }

    fn publish_folder_roots(&mut self, roots: &[String]) {
        let items: Vec<FolderRootItem> = roots
            .iter()
            .map(|r| FolderRootItem {
                path: ss(r),
                is_available: Path::new(r).exists(),
            })
            .collect();
        self.ui
            .global::<State>()
            .set_folder_roots(ui_model(items).into());
    }

    fn refresh_playlists(&mut self) {
        self.playlists = self.store.get_playlists().unwrap_or_default();
        let mut cards = Vec::new();
        for p in &self.playlists {
            let slot = gradient_slot(&p.name);
            cards.push(PlaylistCardItem {
                id: ss(&p.id),
                name: ss(&p.name),
                track_count: p.track_count as i32,
                is_smart: false,
                gradient: slot,
                initials: initials(&p.name).into(),
                description: ss(""),
            });
        }
        let names: Vec<SharedString> = self
            .playlists
            .iter()
            .map(|p| ss(&p.name))
            .collect();
        let state = self.ui.global::<State>();
        state.set_playlists(ui_model(cards.clone()).into());
        state.set_playlist_rows(ui_model(chunk_playlists(cards)).into());
        state.set_playlist_names(ui_model(names).into());
    }

    fn publish_settings(&mut self) {
        use music_player_core::settings::keys as k;
        let s = &self.settings;
        let state = self.ui.global::<State>();
        state.set_theme(ss(s.raw(k::THEME).unwrap_or_else(|| "Dark".into())));
        state.set_accent_mode(ss(s.raw(k::ACCENT_MODE).unwrap_or_else(|| "Artwork".into())));
        state.set_accent_manual(parse_hex_color(&s.raw(k::ACCENT_MANUAL).unwrap_or_else(|| "#b7ff2d".into())));
        state.set_transparent_window(s.bool_or(k::TRANSPARENT_WINDOW, false));
        state.set_window_transparency(s.int_or(k::WINDOW_TRANSPARENCY, 40) as i32);
        state.set_motion_style(ss(s.raw(k::MOTION_STYLE).unwrap_or_else(|| "Subtle".into())));
        state.set_hide_exact_duplicates(s.bool_or(k::HIDE_DUPLICATES, true));
        state.set_autoplay_similar(s.bool_or(k::AUTOPLAY_SIMILAR, true));
        state.set_auto_open_side_panel(s.bool_or(k::AUTO_OPEN_PANEL, false));
        state.set_auto_load_lyrics(s.bool_or(k::AUTO_LOAD_LYRICS, true));
        state.set_seek_from_lyrics(s.bool_or(k::SEEK_FROM_LYRICS, true));
        state.set_show_list_artwork(s.bool_or(k::SHOW_ARTWORK, true));
        state.set_show_list_artist(s.bool_or(k::SHOW_ARTIST, true));
        state.set_show_list_album(s.bool_or(k::SHOW_ALBUM, true));
        state.set_show_list_added(s.bool_or(k::SHOW_ADDED, true));
        state.set_show_list_year(s.bool_or(k::SHOW_YEAR, true));
        state.set_show_list_duration(s.bool_or(k::SHOW_DURATION, true));
        state.set_show_list_favorite(s.bool_or(k::SHOW_FAVORITE, true));
        state.set_check_updates(s.bool_or(k::CHECK_UPDATES, true));
        state.set_minimize_to_tray(s.bool_or(k::MINIMIZE_TO_TRAY, false));
        state.set_navigation_width(s.int_or(k::NAVIGATION_WIDTH, 232) as i32);
        state.set_browse_width(s.int_or(k::BROWSE_WIDTH, 280) as i32);
        state.set_right_panel_width(s.int_or(k::RIGHT_PANEL_WIDTH, 294) as i32);
        state.set_right_panel_open(s.raw(k::RIGHT_PANEL_OPEN).map(|v| v != "false").unwrap_or(true));
        state.set_crossfade_label(ss(crossfade_label(s.crossfade_seconds())));
        state.set_eq_preset(ss(s.eq_current()));
        state.set_recent_searches(ui_model(
            s.recent_searches().into_iter().map(ss).collect::<Vec<_>>(),
        ).into());
        self.publish_eq();
        self.publish_exclusions();
    }

    fn coordinator_eq_name(&self) -> String {
        self.ui.global::<State>().get_eq_preset().to_string()
    }

    fn publish_eq(&mut self) {
        let labels: Vec<String> = EQUALIZER_BANDS
            .iter()
            .map(|f| if *f >= 1000 { format!("{}k", f / 1000) } else { f.to_string() })
            .collect();
        let bands: Vec<EqBandItem> = labels
            .iter()
            .zip(self.eq_bands.iter())
            .map(|(label, value)| EqBandItem {
                label: ss(label),
                value: *value,
            })
            .collect();
        let mut presets: Vec<String> = vec!["Off".into()];
        presets.extend(self.engine.built_in_preset_names());
        presets.extend(self.settings.saved_eq_preset_names());
        let state = self.ui.global::<State>();
        state.set_eq_bands(ui_model(bands).into());
        state.set_eq_preamp(self.eq_preamp);
        state.set_eq_presets(ui_model(presets.into_iter().map(ss).collect::<Vec<_>>()).into());
        state.set_output_devices(ui_model(vec![ss("System default")]).into());
        state.set_output_device(ss(
            self.settings
                .raw(music_player_core::settings::keys::AUDIO_OUTPUT_NAME)
                .unwrap_or_else(|| "System default".into()),
        ));
    }

    fn publish_exclusions(&mut self) {
        let enabled = self.settings.extension_enabled();
        let kinds = self.settings.extension_types();
        let mut exts: Vec<String> = music_player_core::AUDIO_EXTENSIONS
            .iter()
            .map(|e| e.to_string())
            .collect();
        for ext in [".mp4", ".m4v", ".mkv", ".avi", ".mov", ".wmv", ".webm"] {
            if !exts.contains(&ext.to_string()) {
                exts.push(ext.to_string());
            }
        }
        for ext in enabled.keys() {
            if !exts.contains(ext) {
                exts.push(ext.clone());
            }
        }
        exts.sort();
        let items: Vec<ExtensionItem> = exts
            .into_iter()
            .map(|e| ExtensionItem {
                extension: ss(&e),
                kind: ss(kinds.get(&e).cloned().unwrap_or_else(|| {
                    if matches!(e.as_str(), ".mp4" | ".m4v" | ".mkv" | ".avi" | ".mov" | ".wmv" | ".webm") {
                        "Video".into()
                    } else {
                        "Audio".into()
                    }
                })),
                enabled: *enabled.get(&e).unwrap_or(&true),
            })
            .collect();
        let ignored: Vec<SharedString> = self
            .settings
            .ignored_directories()
            .into_iter()
            .map(ss)
            .collect();
        let state = self.ui.global::<State>();
        state.set_exclusion_extensions(ui_model(items).into());
        state.set_ignored_directories(ui_model(ignored).into());
    }

    fn publish_about(&mut self) {
        let stats = self.store.get_library_stats().unwrap_or(LibraryStats { total_tracks: 0, total_bytes: 0 });
        let rows = vec![
            DetailRowItem { label: ss("Version"), value: ss(env!("CARGO_PKG_VERSION")) },
            DetailRowItem { label: "Repository".into(), value: ss("https://github.com/kalabhaftu/music-player") },
            DetailRowItem { label: ss("Library"), value: ss(self.store.db_path().to_string_lossy()) },
            DetailRowItem { label: ss("Log folder"), value: ss(self.log_folder.to_string_lossy()) },
            DetailRowItem { label: ss("Indexed tracks"), value: ss(stats.total_tracks.to_string()) },
        ];
        self.ui.global::<State>().set_about_rows(ui_model(rows).into());
    }

    // ---- View data (port of MainWindow.WebUi.Data.cs query layer) -----------

    fn refresh_view(&mut self) {
        let state = self.ui.global::<State>();
        let view = state.get_view().to_string();
        let search = state.get_view_search().to_string();
        let group_column = state.get_view_group_column().to_string();
        let group_value = state.get_view_group_value().to_string();
        let playlist_id = state.get_view_playlist_id().to_string();
        let sort_name = state.get_view_sort().to_string();
        let descending = state.get_view_descending();
        drop(state);

        let playing = self.current.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        let hide_dupes = self.settings.bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        let query = TrackQuery {
            favorites: view == "Favorites",
            most_played: view == "Most Played",
            recently_played: view == "Recently Played",
            with_lyrics: view == "With Lyrics",
            search: if view == "Search" && !search.trim().is_empty() { Some(search.clone()) } else { None },
            group_column: if matches!(view.as_str(), "Album" | "Artist" | "Genre" | "Folder") {
                Some(group_column.clone())
            } else { None },
            group_value: if matches!(view.as_str(), "Album" | "Artist" | "Genre" | "Folder") {
                Some(group_value.clone())
            } else { None },
            extension_filter: if view == "Videos" {
                Some(vec![".mp4".into(), ".m4v".into(), ".mkv".into(), ".avi".into(), ".mov".into(), ".wmv".into(), ".webm".into()])
            } else { None },
            hide_duplicates: hide_dupes && view == "Songs",
            sort: song_sort(&sort_name, song_sort(&default_sort_for_view(&view).0, TrackSort::Title)),
            descending,
            offset: self.tracks_offset as u32,
            page_size: 100,
            ..TrackQuery::default()
        };
        let (tracks, total) = self.store.query_track_page(&query).unwrap_or_default();
        let items: Vec<TrackItem> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| self.track_to_ui(t, (self.tracks_offset + i + 1) as i32, &playing))
            .collect();
        self.view_paths = tracks.iter().map(|t| t.path.clone()).collect();
        self.id_to_path.clear();
        for t in &tracks {
            self.id_to_path.insert(t.id(), t.path.clone());
        }
        for item in &items {
            self.id_to_path.insert(item.id.to_string(), item.id.to_string());
        }

        let state = self.ui.global::<State>();
        state.set_tracks(ui_model(items).into());
        state.set_tracks_total(total as i32);
        state.set_can_load_more(self.tracks_offset + 100 < total as usize);
        state.set_library_empty(total == 0 && self.settings.library_roots().is_empty());
        state.set_page_loading(false);

        // Home sections.
        if view == "Home" {
            self.publish_home(&playing);
        }
        if matches!(view.as_str(), "Albums" | "Artists" | "Genres" | "Folders" | "Search") {
            self.publish_groups(&view, &search);
        }
        if view == "Playlist" {
            self.publish_playlist(&playlist_id, &playing);
        }
        if matches!(view.as_str(), "Album" | "Artist" | "Genre" | "Folder") {
            self.publish_detail_hero(&view, &group_value);
        }
        if view == "Search" {
            self.publish_search_counts(&search);
        }
    }

    fn publish_home(&mut self, playing: &str) {
        let greeting = match chrono::Local::now().hour() {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            18..=22 => "Good evening",
            _ => "Good night",
        };
        let recent = self
            .store
            .query_track_page(&TrackQuery {
                recently_played: true,
                sort: TrackSort::LastPlayed,
                descending: true,
                page_size: 6,
                ..TrackQuery::default()
            })
            .map(|(tracks, _)| tracks)
            .unwrap_or_default();
        let most = self.store.get_most_played(6).unwrap_or_default();
        let added = self.store.get_recently_added(6).unwrap_or_default();
        let albums = self.store.get_album_groups().unwrap_or_default();
        let map = |tracks: &[Track], this: &mut Self| -> Vec<TrackItem> {
            tracks.iter().enumerate().map(|(i, t)| this.track_to_ui(t, i as i32 + 1, playing)).collect()
        };
        let recent_items = map(&recent, self);
        let most_items = map(&most, self);
        let added_items = map(&added, self);
        let album_items: Vec<GroupItem> = albums
            .iter()
            .take(8)
            .map(|g| group_to_ui(g))
            .collect();
        let state = self.ui.global::<State>();
        state.set_home_greeting(ss(greeting));
        state.set_home_recent(ui_model(recent_items).into());
        state.set_home_most(ui_model(most_items).into());
        state.set_home_added(ui_model(added_items).into());
        state.set_home_albums(ui_model(album_items).into());
    }

    fn publish_groups(&mut self, view: &str, search: &str) {
        let column = match view {
            "Albums" => "album",
            "Artists" => "artist",
            "Genres" => "genre",
            "Folders" => "folder",
            "Search" => "album",
            _ => "album",
        };
        let (groups, total) = self
            .store
            .query_group_page(column, if view == "Search" { Some(search) } else { None }, self.groups_offset as u32, 60)
            .unwrap_or_default();
        self.group_total = total;
        let items: Vec<GroupItem> = groups.iter().map(group_to_ui).collect();
        let rows = chunk_groups(items);
        let state = self.ui.global::<State>();
        state.set_group_rows(ui_model(rows).into());
        state.set_groups_total(total as i32);
    }

    fn publish_search_counts(&mut self, search: &str) {
        if search.trim().is_empty() {
            return;
        }
        let (_, songs) = self.store.query_track_page(&TrackQuery { search: Some(search.into()), page_size: 1, ..TrackQuery::default() }).unwrap_or_default();
        let (_, albums) = self.store.query_group_page("album", Some(search), 0, 1).unwrap_or_default();
        let (_, artists) = self.store.query_group_page("artist", Some(search), 0, 1).unwrap_or_default();
        let state = self.ui.global::<State>();
        state.set_search_song_count(songs as i32);
        state.set_search_album_count(albums as i32);
        state.set_search_artist_count(artists as i32);
        state.set_search_playlist_count(
            self.playlists.iter().filter(|p| p.name.to_lowercase().contains(&search.to_lowercase())).count() as i32,
        );
    }

    fn publish_playlist(&mut self, playlist_id: &str, playing: &str) {
        let Some(playlist) = self.store.get_playlist(playlist_id).ok().flatten() else {
            self.ui.global::<State>().set_playlist_name(ss("Playlist not found."));
            return;
        };
        let paths = self.store.get_playlist_paths(playlist_id).unwrap_or_default();
        let tracks: Vec<Track> = paths
            .iter()
            .filter_map(|p| self.store.get_track(p).ok().flatten())
            .collect();
        let items: Vec<TrackItem> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| self.track_to_ui(t, i as i32 + 1, playing))
            .collect();
        for path in &paths {
            if self.store.get_track(path).ok().flatten().is_none() {
                self.placeholder_item(path, playing);
            }
        }
        let state = self.ui.global::<State>();
        state.set_playlist_tracks(ui_model(items).into());
        state.set_playlist_name(ss(&playlist.name));
        state.set_playlist_track_total(paths.len() as i32);
        state.set_playlist_empty_message(ss(if paths.is_empty() {
            "This playlist is empty. Use a track's menu to add songs."
        } else { "" }));
        self.view_paths = paths;
    }

    fn publish_detail_hero(&mut self, view: &str, value: &str) {
        let mut subtitle = String::new();
        let mut cover = Image::default();
        let mut has_cover = false;
        let column = match view {
            "Album" => "album",
            "Artist" => "artist",
            "Genre" => "genre",
            _ => "folder",
        };
        if let Ok((groups, _)) = self.store.query_group_page(column, None, 0, 10000) {
            if let Some(g) = groups.iter().find(|g| g.name.eq_ignore_ascii_case(value)) {
                subtitle = format!("{} song{}", g.track_count, if g.track_count == 1 { "" } else { "s" });
                if let Some(art) = &g.artwork_path {
                    if let Ok(image) = Image::load_from_path(Path::new(art)) {
                        cover = image;
                        has_cover = true;
                    }
                }
            }
        }
        let mut albums = Vec::new();
        if column == "artist" {
            if let Ok((groups, _)) = self.store.query_group_page("album", Some(value), 0, 30) {
                albums = groups.iter().map(group_to_ui).collect();
            }
        }
        let state = self.ui.global::<State>();
        state.set_hero_subtitle(ss(subtitle));
        state.set_hero_cover(cover);
        state.set_hero_has_cover(has_cover);
        state.set_artist_albums(ui_model(albums).into());
    }

    // ---- Lyrics -------------------------------------------------------------

    fn load_lyrics(&mut self, track: &Track) {
        self.lyrics_target = Some(track.path.clone());
        self.lyrics_doc = None;
        self.lyrics_timed = false;
        self.lyrics_active = -1;
        let (text, source) = if let Some(sidecar) = read_sidecar(&track.path) {
            (Some(sidecar), "sidecar")
        } else if track.has_lyrics {
            // Embedded text is exposed as a flag by core; the Edit dialog can
            // write it back. Sidecar/find-lyrics remain the display sources.
            (None, "embedded")
        } else {
            (None, "none")
        };
        let mut lines = Vec::new();
        let has_text = text.is_some();
        if let Some(text) = text {
            let doc = parse_lrc(&text);
            self.lyrics_timed = !doc.lines.is_empty();
            for line in &doc.lines {
                lines.push(LyricLineItem {
                    text: ss(&line.text),
                    is_active: false,
                    is_past: false,
                    is_upcoming: true,
                });
            }
            if !self.lyrics_timed {
                for plain in text.lines().filter(|l| !l.trim().is_empty()) {
                    lines.push(LyricLineItem {
                        text: ss(plain),
                        is_active: false,
                        is_past: false,
                        is_upcoming: true,
                    });
                }
            }
            self.lyrics_doc = Some(doc);
        }
        let state = self.ui.global::<State>();
        state.set_lyrics_lines(ui_model(lines).into());
        state.set_has_lyrics(!self.lyrics_doc.as_ref().map(|d| d.lines.is_empty()).unwrap_or(true)
            || has_text);
        state.set_lyrics_source(ss(source));
        state.set_lyrics_timed(self.lyrics_timed);
        state.set_lyrics_active_line(-1);
    }

    fn update_lyrics_position(&mut self, position_ms: u64) {
        if !self.lyrics_timed {
            return;
        }
        let Some(doc) = &self.lyrics_doc else { return };
        let mut active = -1i32;
        for (i, line) in doc.lines.iter().enumerate() {
            if line.time_ms <= position_ms {
                active = i as i32;
            } else {
                break;
            }
        }
        if active == self.lyrics_active {
            return;
        }
        self.lyrics_active = active;
        let timed = self.lyrics_timed;
        let lines: Vec<LyricLineItem> = doc
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| LyricLineItem {
                text: ss(&line.text),
                is_active: i as i32 == active,
                is_past: (i as i32) < active,
                is_upcoming: (i as i32) > active,
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_lyrics_lines(ui_model(lines).into());
        state.set_lyrics_active_line(active);
        let _ = timed;
    }

    fn poll_lyrics_search(&mut self) {
        let Some(rx) = &self.lyrics_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.lyrics_rx = None;
        let state = self.ui.global::<State>();
        state.set_lyrics_searching(false);
        match result {
            Ok(results) => {
                state.set_lyrics_search_status(ss(if results.is_empty() {
                    "No matching lyrics were found."
                } else {
                    ""
                }));
                let items: Vec<LyricResultItem> = results
                    .iter()
                    .map(|r| LyricResultItem {
                        title: ss(&r.track_name),
                        artist: ss(&r.artist_name),
                        badge: ss(if r.synced_lyrics.is_some() { "Timed" } else { "Plain" }),
                        timed: r.synced_lyrics.is_some(),
                        synced_id: ss(r.id.to_string()),
                        plain_id: ss(r.id.to_string()),
                    })
                    .collect();
                state.set_lyrics_results(ui_model(items).into());
                self.lyrics_results = results;
            }
            Err(message) => state.set_lyrics_search_status(ss(message)),
        }
    }

    // ---- Duplicates ---------------------------------------------------------

    fn load_duplicates(&mut self, sort: &str, descending: bool, page: usize) {
        self.dup_sort = sort.to_string();
        self.dup_desc = descending;
        self.dup_page = page;
        let page_size = 20usize;
        let groups = self
            .store
            .get_duplicate_groups(page * page_size, page_size)
            .unwrap_or_default();
        let total = self.store.get_duplicate_groups(0, 10000).map(|g| g.len()).unwrap_or(0);
        let playing = self.current.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        let items: Vec<DuplicateItem> = groups
            .iter()
            .map(|g| self.duplicate_to_ui(g, &playing))
            .collect();
        let state = self.ui.global::<State>();
        state.set_duplicates(ui_model(items).into());
        state.set_duplicates_total(total as i32);
        state.set_duplicates_page(page as i32);
        state.set_duplicates_pages(((total + page_size - 1) / page_size).max(1) as i32);
        state.set_duplicates_sort(ss(sort));
        state.set_duplicates_descending(descending);
    }

    fn duplicate_to_ui(&mut self, group: &ExactDuplicateGroup, _playing: &str) -> DuplicateItem {
        DuplicateItem {
            title: ss(&group.representative.title),
            artist: ss(&group.representative.artist),
            copies: group.copy_count as i32,
            locations: ss(format!("{} copies", group.copy_count)),
            first_id: ss(group.representative.id()),
        }
    }

    fn open_duplicate_files(&mut self, id: &str) {
        let Some(path) = self.id_to_path.get(id).cloned() else { return };
        let Some(group) = self
            .store
            .get_duplicate_groups(0, 10000)
            .ok()
            .and_then(|groups| groups.into_iter().find(|g| g.representative.path == path))
        else { return };
        let playing = self.current.as_ref().map(|t| t.path.clone()).unwrap_or_default();
        let files: Vec<TrackItem> = vec![group.representative.clone()]
            .iter()
            .map(|t| self.track_to_ui(t, 1, &playing))
            .collect();
        let state = self.ui.global::<State>();
        state.set_duplicate_files(ui_model(files).into());
        state.set_duplicate_group_title(ss(&group.representative.title));
    }

    // ---- Toasts (C# 4-second InfoBar notice) -------------------------------

    fn show_notice(&mut self, message: impl Into<String>, severity: NoticeSeverity) {
        let severity_text = match severity {
            NoticeSeverity::Informational => "Informational",
            NoticeSeverity::Warning => "Warning",
            NoticeSeverity::Error => "Error",
            NoticeSeverity::Success => "Success",
        };
        let state = self.ui.global::<State>();
        state.set_notice_message(ss(message));
        state.set_notice_severity(ss(severity_text));
        state.set_notice_visible(true);
        let weak = self.ui.as_weak();
        self.notice_timer.start(TimerMode::SingleShot, Duration::from_secs(4), move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<State>().set_notice_visible(false);
            }
        });
    }

    // ---- Navigation ---------------------------------------------------------

    fn set_view(&mut self, view: &str, search: &str, group: &str, playlist_id: &str) {
        let state = self.ui.global::<State>();
        let current = NavContext::snapshot(&state);
        if current.view != view
            || current.search != search
            || current.group_value != group
            || current.playlist_id != playlist_id
        {
            self.history.truncate(self.history_pos + 1);
            self.history.push(current);
            if self.history.len() > 50 {
                self.history.remove(0);
            }
            self.history_pos = self.history.len();
        }
        self.tracks_offset = 0;
        self.groups_offset = 0;
        state.set_view(ss(view));
        state.set_view_search(ss(search));
        state.set_view_group_column(ss(if matches!(view, "Album" | "Artist" | "Genre" | "Folder") {
            match view { "Album" => "album", "Artist" => "artist", "Genre" => "genre", _ => "folder" }
        } else { "" }));
        state.set_view_group_value(ss(group));
        state.set_view_playlist_id(ss(playlist_id));
        let (sort, desc) = if matches!(view, "Album" | "Artist" | "Genre" | "Folder") {
            detail_sort_for_column(match view { "Album" => "album", "Artist" => "artist", "Genre" => "genre", _ => "folder" })
        } else {
            default_sort_for_view(view)
        };
        state.set_view_sort(ss(sort));
        state.set_view_descending(desc);
        state.set_can_go_back(self.history_pos > 0);
        state.set_can_go_forward(false);
        self.settings.set_raw(music_player_core::settings::keys::LAST_VIEW_CONTEXT, view);
        self.refresh_view();
    }

    fn go_back(&mut self) {
        if self.history_pos == 0 {
            return;
        }
        self.history_pos -= 1;
        let ctx = self.history[self.history_pos].clone();
        {
            let state = self.ui.global::<State>();
            ctx.restore(&state);
            state.set_can_go_back(self.history_pos > 0);
            state.set_can_go_forward(true);
        }
        self.tracks_offset = 0;
        self.refresh_view();
    }

    fn go_forward(&mut self) {
        if self.history_pos + 1 >= self.history.len() {
            return;
        }
        self.history_pos += 1;
        let ctx = self.history[self.history_pos].clone();
        {
            let state = self.ui.global::<State>();
            ctx.restore(&state);
            state.set_can_go_back(true);
            state.set_can_go_forward(self.history_pos + 1 < self.history.len());
        }
        self.tracks_offset = 0;
        self.refresh_view();
    }

    // ---- Playlists ----------------------------------------------------------

    fn new_playlist_id(&self) -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        format!("{:x}-{:x}", nanos, std::process::id())
    }

    fn create_playlist(&mut self, name: &str) {
        if name.trim().is_empty() {
            self.show_notice("Playlist name cannot be empty.", NoticeSeverity::Warning);
            return;
        }
        let id = self.new_playlist_id();
        if self.store.create_playlist(&id, name.trim()).is_ok() {
            self.refresh_playlists();
            self.set_view("Playlist", "", "", &id);
            self.close_modal();
        } else {
            self.show_notice("Could not create the playlist.", NoticeSeverity::Error);
        }
    }

    fn rename_playlist(&mut self, id: &str, name: &str) {
        if name.trim().is_empty() {
            return;
        }
        if self.store.rename_playlist(id, name.trim()).is_ok() {
            self.refresh_playlists();
            self.close_modal();
        }
    }

    fn delete_playlist(&mut self, id: &str) {
        if self.store.delete_playlist(id).is_ok() {
            self.refresh_playlists();
            self.set_view("Playlists", "", "", "");
            self.close_modal();
        }
    }

    fn play_playlist(&mut self, id: &str, shuffle: bool) {
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
            self.show_notice("That playlist has no available tracks.", NoticeSeverity::Warning);
            return;
        }
        let Some(track) = self.store.get_track(&paths[0]).ok().flatten() else { return };
        self.view_paths = paths;
        self.coordinator.queue_mut().replace(self.view_paths.clone(), Some(0));
        if shuffle {
            self.coordinator.queue_mut().shuffle_upcoming(1);
        }
        self.play_track(track, false, Some(0));
    }

    // ---- Context menu -------------------------------------------------------

    const CONTEXT_ITEMS: [&'static str; 15] = [
        "Play",
        "Play next",
        "Add to queue",
        "Add to playlist",
        "Create playlist with this song…",
        "Toggle favorite",
        "Set rating…",
        "—",
        "Lyrics",
        "Find lyrics…",
        "Edit lyrics…",
        "Edit tags",
        "Restore previous tags",
        "Track details",
        "Show in File Explorer",
    ];

    fn open_context_menu(&mut self, id: &str, x: i32, y: i32) {
        self.context_target = Some(id.to_string());
        let items: Vec<ContextMenuItem> = Self::CONTEXT_ITEMS
            .iter()
            .enumerate()
            .map(|(i, label)| ContextMenuItem {
                label: ss(*label),
                danger: false,
                separator: i == 7,
                enabled: true,
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_context_items(ui_model(items).into());
        state.set_context_x(if x > 0 { x as f32 } else { 240.0 });
        state.set_context_y(if y > 0 { y as f32 } else { 140.0 });
        state.set_context_visible(true);
    }

    fn context_action(&mut self, index: usize) {
        let Some(id) = self.context_target.clone() else { return };
        self.ui.global::<State>().set_context_visible(false);
        match index {
            0 => { if let Some(t) = self.track_by_id(&id) { self.play_track(t, true, None); } }
            1 => { if let Some(t) = self.track_by_id(&id) { self.coordinator.queue_mut().insert_next(t.path); self.publish_queue(true); } }
            2 => { if let Some(t) = self.track_by_id(&id) { self.coordinator.queue_mut().append_one(t.path); self.publish_queue(true); } }
            3 => self.open_add_to_playlist(&id),
            4 => {
                if let Some(t) = self.track_by_id(&id) {
                    let name = t.title.clone();
                    self.create_playlist(&format!("{} Mix", name));
                    self.add_playlist_track = Some(id.clone());
                }
            }
            5 => self.toggle_favorite(&id),
            6 => { self.ui.global::<State>().set_modal(ss("rating")); self.ui.global::<State>().set_rate_value(0); }
            8 => self.set_view("Lyrics", "", "", ""),
            9 => self.open_find_lyrics(&id),
            10 => self.open_edit_lyrics(),
            11 => self.open_tags(&id),
            12 => {
                if let Some(t) = self.track_by_id(&id) {
                    self.confirm_action = Some(ConfirmAction::RestoreBackup(String::new()));
                    let backups = TagEditor::new(tag_backups_dir()).list_backups(&t.path);
                    let choices: Vec<SharedString> = backups.iter().map(|b| ss(file_name(&b.backup_path))).collect();
                    self.confirm_action = Some(ConfirmAction::RestoreBackup(backups.first().map(|b| b.backup_path.clone()).unwrap_or_default()));
                    let state = self.ui.global::<State>();
                    state.set_confirm_message(ss("Restore the most recent tag backup?"));
                    state.set_confirm_has_choices(false);
                    state.set_confirm_choices(ui_model(choices).into());
                    state.set_modal(ss("confirm"));
                }
            }
            13 => self.open_details(&id),
            14 => { if let Some(t) = self.track_by_id(&id) { open_in_explorer(&t.path); } }
            _ => {}
        }
    }

    // ---- Modals -------------------------------------------------------------

    fn open_add_to_playlist(&mut self, id: &str) {
        self.add_playlist_track = Some(id.to_string());
        self.ui
            .global::<State>()
            .set_add_to_playlist_id(ss(id));
        self.open_modal("add-to-playlist");
    }

    fn open_modal(&mut self, name: &str) {
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
        state.set_modal(ss(name));
        state.set_modal_title(ss(title));
    }

    fn close_modal(&mut self) {
        self.ui.global::<State>().set_modal(ss(""));
        self.confirm_action = None;
    }

    fn open_details(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else { return };
        self.details_target = Some(track.path.clone());
        let details = inspect_file(Path::new(&track.path));
        let mut rows = vec![
            DetailRowItem { label: ss("File"), value: ss(file_name(&track.path)) },
            DetailRowItem { label: ss("Title"), value: ss(&track.title) },
            DetailRowItem { label: ss("Artist"), value: ss(&track.artist) },
            DetailRowItem { label: ss("Album"), value: ss(&track.album) },
            DetailRowItem { label: ss("Album artist"), value: ss(&track.album_artist) },
            DetailRowItem { label: ss("Genre"), value: ss(&track.genre) },
            DetailRowItem { label: ss("Year"), value: ss(track.year.to_string()) },
            DetailRowItem { label: ss("Duration"), value: ss(fmt_time(track.duration_ms)) },
            DetailRowItem { label: ss("Size"), value: ss(format!("{:.1} MB", track.file_size as f64 / 1_048_576.0)) },
        ];
        if let Some(d) = details {
            rows.push(DetailRowItem { label: ss("Container"), value: ss(d.container) });
            rows.push(DetailRowItem { label: ss("Bitrate"), value: ss(format!("{} kbps", d.bitrate_kbps)) });
        }
        let additional: Vec<DetailRowItem> = read_additional_standard_fields(Path::new(&track.path))
            .into_iter()
            .map(|(k, v)| DetailRowItem { label: ss(k), value: ss(v) })
            .collect();
        let custom: Vec<DetailRowItem> = read_custom_fields(Path::new(&track.path))
            .into_iter()
            .map(|(k, v)| DetailRowItem { label: ss(k), value: ss(v) })
            .collect();
        let backups: Vec<BackupItem> = TagEditor::new(tag_backups_dir())
            .list_backups(&track.path)
            .into_iter()
            .map(|b| BackupItem { label: ss(file_name(&b.backup_path)) })
            .collect();
        let state = self.ui.global::<State>();
        state.set_detail_rows(ui_model(rows).into());
        state.set_detail_additional(ui_model(additional).into());
        state.set_detail_custom(ui_model(custom).into());
        state.set_detail_backups(ui_model(backups).into());
        self.open_modal("details");
    }

    fn open_tags(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else { return };
        self.tags_target = Some(track.path.clone());
        let tags = music_player_core::read_tags(&track.path);
        let state = self.ui.global::<State>();
        state.set_tags_title(ss(tags.title.unwrap_or_else(|| track.title.clone())));
        state.set_tags_artist(ss(tags.artist.unwrap_or_else(|| track.artist.clone())));
        state.set_tags_album(ss(tags.album.unwrap_or_else(|| track.album.clone())));
        state.set_tags_album_artist(ss(tags.album_artist.unwrap_or_else(|| track.album_artist.clone())));
        state.set_tags_genre(ss(tags.genre.unwrap_or_else(|| track.genre.clone())));
        state.set_tags_year(ss(if tags.year > 0 { tags.year.to_string() } else { track.year.to_string() }));
        state.set_tags_track(ss(if tags.track_number > 0 { tags.track_number.to_string() } else { String::new() }));
        state.set_tags_artwork_name(ss(tags.artwork_path.as_deref().map(file_name).unwrap_or_default()));
        state.set_tags_has_artwork(tags.artwork_path.is_some());
        state.set_tags_format_note(ss(music_player_core::custom_field_format(Path::new(&track.path)).unwrap_or("")));
        self.tags_additional = read_additional_standard_fields(Path::new(&track.path))
            .into_iter()
            .collect();
        self.tags_custom = read_custom_fields(Path::new(&track.path))
            .into_iter()
            .collect();
        self.publish_tag_fields();
        self.open_modal("tags");
    }

    fn publish_tag_fields(&mut self) {
        let additional: Vec<TagFieldItem> = self.tags_additional
            .iter()
            .map(|(k, v)| TagFieldItem { key: ss(k), value: ss(v) })
            .collect();
        let custom: Vec<TagFieldItem> = self.tags_custom
            .iter()
            .map(|(k, v)| TagFieldItem { key: ss(k), value: ss(v) })
            .collect();
        let state = self.ui.global::<State>();
        state.set_tags_additional(ui_model(additional).into());
        state.set_tags_custom(ui_model(custom).into());
    }

    fn save_tags(&mut self) {
        let Some(path) = self.tags_target.clone() else { return };
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
            custom_fields: if custom.is_empty() { None } else { Some(custom) },
            additional_fields: if additional.is_empty() { None } else { Some(additional) },
            ..TagEdit::default()
        };
        let editor = TagEditor::new(tag_backups_dir());
        match editor.save(&path, &edit) {
            Ok(_) => {
                self.show_notice("Tags saved. A backup of the original was kept.", NoticeSeverity::Success);
                if let Ok(Some(track)) = self.store.get_track(&path) {
                    self.update_current_track(&track);
                }
                self.close_modal();
                self.refresh_view();
            }
            Err(err) => {
                LocalAppLog::shared().error("tags", "save failed", Some(&err));
                self.show_notice("Tags could not be saved. See Settings → Open log folder for details.", NoticeSeverity::Error);
            }
        }
    }

    fn restore_backup(&mut self, path: &str) {
        let Some(target) = self.tags_target.clone().or_else(|| {
            self.context_target.as_ref().and_then(|id| self.id_to_path.get(id).cloned())
        }) else { return };
        let Some(backups) = TagEditor::new(tag_backups_dir())
            .list_backups(&target)
            .into_iter()
            .find(|b| b.backup_path == path || file_name(&b.backup_path) == file_name(path))
        else { return };
        match TagEditor::new(tag_backups_dir()).restore(&backups) {
            Ok(_) => {
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

    fn open_find_lyrics(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else { return };
        self.lyrics_target = Some(track.path.clone());
        let state = self.ui.global::<State>();
        state.set_find_lyrics_title(ss(&track.title));
        state.set_find_lyrics_artist(ss(&track.artist));
        state.set_lyrics_results(ui_model(Vec::<LyricResultItem>::new()).into());
        state.set_lyrics_search_status(ss(""));
        self.open_modal("find-lyrics");
    }

    fn find_lyrics(&mut self) {
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
        });
    }

    fn use_lyric_result(&mut self, id: &str, timed: bool) {
        let Some(result) = self.lyrics_results.iter().find(|r| r.id.to_string() == id).cloned() else { return };
        let text = if timed {
            result.synced_lyrics.clone().unwrap_or_default()
        } else {
            result.plain_lyrics.clone().unwrap_or_default()
        };
        if text.trim().is_empty() {
            return;
        }
        let Some(path) = self.lyrics_target.clone() else { return };
        let sidecar = self.ui.global::<State>().get_edit_lyrics_sidecar();
        let stored = if sidecar {
            write_sidecar(&path, &text).is_ok()
        } else {
            TagEditor::new(tag_backups_dir())
                .save(&path, &TagEdit { lyrics: Some(text.clone()), ..TagEdit::default() })
                .is_ok()
        };
        if !stored && sidecar {
            // Embed-fallback-to-sidecar equivalent: if the sidecar write fails,
            // try embedding instead.
            let _ = TagEditor::new(tag_backups_dir())
                .save(&path, &TagEdit { lyrics: Some(text.clone()), ..TagEdit::default() });
        }
        if let Ok(Some(track)) = self.store.get_track(&path) {
            self.load_lyrics(&track);
        }
        self.close_modal();
        self.show_notice("Lyrics saved.", NoticeSeverity::Success);
    }

    fn open_edit_lyrics(&mut self) {
        let Some(path) = self
            .edit_target
            .clone()
            .or_else(|| self.lyrics_target.clone())
            .or_else(|| self.current.as_ref().map(|t| t.path.clone()))
        else { return };
        self.edit_target = Some(path.clone());
        let (text, source) = if let Some(sidecar) = read_sidecar(&path) {
            (sidecar, "sidecar file")
        } else if self.ui.global::<State>().get_lyrics_timed() {
            self.lyrics_doc
                .as_ref()
                .map(format_lrc)
                .unwrap_or_default()
                .pipe(|s| (s, "embedded tags"))
        } else {
            (String::new(), "no existing lyrics")
        };
        let state = self.ui.global::<State>();
        state.set_edit_lyrics_text(ss(text));
        state.set_edit_lyrics_source_label(ss(format!("Source: {}", source)));
        state.set_edit_lyrics_sidecar(true);
        state.set_edit_lyrics_offset(ss(
            self.lyrics_doc
                .as_ref()
                .map(|d| d.offset_ms.to_string())
                .unwrap_or_else(|| "0".into()),
        ));
        self.open_modal("edit-lyrics");
    }

    fn save_lyrics_modal(&mut self) {
        let Some(path) = self.edit_target.clone() else { return };
        let state = self.ui.global::<State>();
        let mut text = state.get_edit_lyrics_text().to_string();
        let offset: i64 = state.get_edit_lyrics_offset().to_string().parse().unwrap_or(0);
        if offset != 0 {
            text = format!("[offset:{}]\n{}", offset, text);
        }
        let sidecar = state.get_edit_lyrics_sidecar();
        let stored = if sidecar {
            write_sidecar(&path, &text).is_ok()
        } else {
            TagEditor::new(tag_backups_dir())
                .save(&path, &TagEdit { lyrics: Some(text.clone()), ..TagEdit::default() })
                .is_ok()
        };
        if !stored {
            self.show_notice("Lyrics could not be saved. Check the file location.", NoticeSeverity::Error);
            return;
        }
        if let Ok(Some(track)) = self.store.get_track(&path) {
            self.load_lyrics(&track);
        }
        self.close_modal();
        self.show_notice("Lyrics saved.", NoticeSeverity::Success);
    }

    fn remove_lyrics(&mut self) {
        let Some(path) = self.edit_target.clone().or_else(|| self.lyrics_target.clone()) else { return };
        let _ = std::fs::remove_file(music_player_core::lyrics::sidecar_path(&path));
        let _ = TagEditor::new(tag_backups_dir())
            .save(&path, &TagEdit { lyrics: Some(String::new()), ..TagEdit::default() });
        if let Ok(Some(track)) = self.store.get_track(&path) {
            self.load_lyrics(&track);
        }
        self.close_modal();
    }

    fn stamp_lyric_position(&mut self) {
        let position = fmt_time(self.engine.position_ms());
        let current = self.ui.global::<State>().get_edit_lyrics_text().to_string();
        self.ui
            .global::<State>()
            .set_edit_lyrics_text(ss(format!("[{}]{}\n{}", position, " ", current)));
    }

    fn seek_to_lyric(&mut self, index: usize) {
        if let Some(doc) = &self.lyrics_doc {
            if let Some(line) = doc.lines.get(index) {
                let mut time = line.time_ms as i64 + doc.offset_ms;
                if time < 0 { time = 0; }
                let _ = self.engine.seek(time as u64);
            }
        }
    }

    // ---- Settings / equalizer ----------------------------------------------

    fn set_setting(&mut self, name: &str, value: &str) {
        let web_name = match name {
            "theme" => "theme",
            "accent-mode" => "accentMode",
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
        } else {
            json!(value)
        };
        match web_name {
            "theme" | "accentMode" | "motionStyle" | "transparentWindow" | "hideDuplicates"
            | "autoplaySimilar" | "autoOpenPanel" | "autoLoadLyrics" | "seekFromLyrics"
            | "showArtwork" | "showArtist" | "showAlbum" | "showAdded" | "showYear"
            | "showDuration" | "showFavorite" | "checkUpdates" | "minimizeToTray"
            | "rightPanelOpen" => {
                if let Err(message) = self.settings.update_web_setting(web_name, &json_value) {
                    self.show_notice(message, NoticeSeverity::Warning);
                }
            }
            _ => self.settings.set_raw(name, value),
        }
        // Re-publish affected UI state.
        let state = self.ui.global::<State>();
        state.set_transparent_window(self.settings.bool_or(music_player_core::settings::keys::TRANSPARENT_WINDOW, false));
        if name == "check-updates" {
            self.check_updates_if_enabled();
        }
        self.refresh_view();
    }

    fn set_crossfade_label(&mut self, label: &str) {
        let seconds = match label {
            "2 s" => 2,
            "3 s" => 3,
            "5 s" => 5,
            "8 s" => 8,
            "10 s" => 10,
            _ => 0,
        };
        self.settings.set_raw(music_player_core::settings::keys::CROSSFADE_SECONDS, &seconds.to_string());
        self.coordinator.set_crossfade_ms(seconds * 1000);
        self.ui.global::<State>().set_crossfade_label(ss(label));
    }

    fn apply_equalizer(&mut self) {
        let preset = self.coordinator_eq_name();
        let name = if preset == "Off" { None } else { Some(preset.as_str()) };
        self.engine.apply_equalizer(name, Some(self.eq_bands), self.eq_preamp);
    }

    fn set_eq_preset(&mut self, name: &str) {
        if name == "Off" {
            self.eq_bands = [0.0; 10];
            self.eq_preamp = NEUTRAL_EQUALIZER_PREAMP;
        } else if let Some((bands, preamp)) = self.settings.saved_eq_preset(name) {
            self.eq_bands = bands;
            self.eq_preamp = preamp;
        } else {
            match name {
                "Flat" => self.eq_bands = [0.0; 10],
                "Bass boost" | "Bass Boost" => self.eq_bands = EqualizerStatePreset::bass(),
                "Rock" => self.eq_bands = EqualizerStatePreset::rock(),
                "Vocal" => self.eq_bands = EqualizerStatePreset::vocal(),
                "Electronic" => self.eq_bands = EqualizerStatePreset::electronic(),
                _ => {}
            }
        }
        self.settings.set_eq_state(name, self.eq_bands, self.eq_preamp);
        self.ui.global::<State>().set_eq_preset(ss(name));
        self.apply_equalizer();
        self.publish_eq();
    }

    fn begin_search(&mut self) {
        self.ui.global::<State>().set_page_loading(true);
    }

    fn clear_recent_searches(&mut self) {
        self.settings
            .set_string_list(music_player_core::settings::keys::RECENT_SEARCHES, &[]);
        self.ui
            .global::<State>()
            .set_recent_searches(ui_model(Vec::<SharedString>::new()).into());
    }

    fn check_updates_if_enabled(&mut self) {
        if !self.settings.bool_or(music_player_core::settings::keys::CHECK_UPDATES, true) {
            return;
        }
        let tag = self.settings.raw(music_player_core::settings::keys::UPDATE_RELEASE_TAG);
        if let Some(tag) = tag {
            self.ui.global::<State>().set_update_tag(ss(tag));
        }
    }

    // ---- Helpers for native commands ---------------------------------------

    fn add_root(&mut self, path: &str) {
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

    fn clear_queue(&mut self) {
        self.coordinator.queue_mut().clear_upcoming();
        self.publish_queue(true);
        self.save_session();
    }

    fn toggle_favorite(&mut self, id: &str) {
        let Some(track) = self.track_by_id(id) else { return };
        let new_value = !track.favorite;
        if self.store.set_favorite(&track.path, new_value).is_ok() {
            self.refresh_view();
            if self.current.as_ref().map(|t| t.path.as_str()) == Some(track.path.as_str()) {
                if let Ok(Some(updated)) = self.store.get_track(&track.path) {
                    self.update_current_track(&updated);
                }
            }
        }
    }

    fn rate_track(&mut self, rating: i32) {
        let Some(path) = self.context_target.as_ref().and_then(|id| self.id_to_path.get(id)).cloned()
            .or_else(|| self.current.as_ref().map(|t| t.path.clone()))
        else { return };
        if self.store.set_rating(&path, rating.clamp(0, 5) as u8).is_ok() {
            self.close_modal();
            self.refresh_view();
        }
    }

    fn confirm_action(&mut self) {
        let Some(action) = self.confirm_action.clone() else { return };
        match action {
            ConfirmAction::RemoveRoot(path) => {
                let mut roots: Vec<String> = self
                    .settings
                    .library_roots()
                    .into_iter()
                    .filter(|r| !r.eq_ignore_ascii_case(&path))
                    .collect();
                let _ = self.store.purge_tracks_under_directory(&path);
                self.settings.set_library_roots(&roots);
                roots.retain(|r| Path::new(r).exists());
                self.load_roots();
                self.refresh_view();
            }
            ConfirmAction::DeletePlaylist(id) => self.delete_playlist(&id),
            ConfirmAction::RestoreBackup(path) => self.restore_backup(&path),
            ConfirmAction::ClearQueue => self.clear_queue(),
            ConfirmAction::RemoveLyrics => self.remove_lyrics(),
            ConfirmAction::Reset(groups) => {
                let list: Vec<String> = groups.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
                self.settings.reset_groups(&list);
                self.publish_settings();
                self.refresh_view();
            }
        }
        self.close_modal();
    }
}

// Stand-in for the small preset tables in core's EqualizerState.
struct EqualizerStatePreset;
impl EqualizerStatePreset {
    fn bass() -> [f32; 10] { [4.0, 3.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] }
    fn rock() -> [f32; 10] { [2.0, 1.0, 0.0, -1.0, -2.0, 0.0, 1.0, 2.0, 2.0, 2.0] }
    fn vocal() -> [f32; 10] { [-1.0, -1.0, 0.0, 1.0, 3.0, 3.0, 2.0, 0.0, 0.0, 0.0] }
    fn electronic() -> [f32; 10] { [3.0, 2.0, 0.0, -1.0, 0.0, 2.0, 3.0, 2.0, 1.0, 0.0] }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T { f(self) }
}
impl<T> Pipe for T {}

fn group_to_ui(group: &music_player_core::LibraryGroup) -> GroupItem {
    let slot = gradient_slot(&group.name);
    let subtitle = if let Some(artist) = &group.artist {
        format!("{} · {} song{}", artist, group.track_count, if group.track_count == 1 { "" } else { "s" })
    } else if group.year > 0 {
        format!("{} song{} · {}", group.track_count, if group.track_count == 1 { "" } else { "s" }, group.year)
    } else {
        format!("{} song{}", group.track_count, if group.track_count == 1 { "" } else { "s" })
    };
    GroupItem {
        name: ss(&group.name),
        subtitle: ss(subtitle),
        initials: ss(initials(&group.name)),
        gradient: slot,
        track_count: group.track_count as i32,
    }
}

fn chunk_groups(groups: Vec<GroupItem>) -> Vec<GroupRowItem> {
    let mut rows = Vec::new();
    let mut iter = groups.into_iter();
    loop {
        let a = iter.next();
        let b = iter.next();
        let c = iter.next();
        let filled = [&a, &b, &c].iter().filter(|x| x.is_some()).count() as i32;
        if filled == 0 { break; }
        let empty = GroupItem {
            name: ss(""), subtitle: ss(""), initials: ss(""), gradient: 1, track_count: 0,
        };
        rows.push(GroupRowItem {
            a: a.unwrap_or_else(|| empty.clone()),
            b: b.unwrap_or_else(|| empty.clone()),
            c: c.unwrap_or(empty),
            filled,
        });
    }
    rows
}

fn chunk_playlists(cards: Vec<PlaylistCardItem>) -> Vec<PlaylistRowItem> {
    let mut rows = Vec::new();
    let mut iter = cards.into_iter();
    loop {
        let a = iter.next();
        let b = iter.next();
        let c = iter.next();
        let filled = [&a, &b, &c].iter().filter(|x| x.is_some()).count() as i32;
        if filled == 0 { break; }
        let empty = PlaylistCardItem {
            id: ss(""), name: ss(""), track_count: 0, is_smart: false,
            gradient: 1, initials: ss(""), description: ss(""),
        };
        rows.push(PlaylistRowItem {
            a: a.unwrap_or_else(|| empty.clone()),
            b: b.unwrap_or_else(|| empty.clone()),
            c: c.unwrap_or(empty),
            filled,
        });
    }
    rows
}

fn crossfade_label(seconds: i64) -> String {
    match seconds {
        2 => "2 s".into(),
        3 => "3 s".into(),
        5 => "5 s".into(),
        8 => "8 s".into(),
        10 => "10 s".into(),
        _ => "Off".into(),
    }
}

fn main() -> std::result::Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let app = Rc::new(RefCell::new(App::new(ui.clone_strong())));

    // Navigation.
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_view(move |view, search, group, playlist_id| {
            a.borrow_mut().set_view(&view, &search, &group, &playlist_id);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_go_back(move || a.borrow_mut().go_back());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_go_forward(move || a.borrow_mut().go_forward());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_begin_search(move || a.borrow_mut().begin_search());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_search(move |query, filter| {
            let mut app = a.borrow_mut();
            let view = if filter == "all" { "Search".to_string() } else { filter.to_string() };
            app.set_view(&view, &query, "", "");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_search_input(move |query| {
            let mut app = a.borrow_mut();
            let current = app.ui.global::<State>().get_view().to_string();
            if current == "Search" {
                app.ui.global::<State>().set_view_search(ss(&query));
                app.set_view("Search", &query, "", "");
            } else {
                app.set_view("Search", &query, "", "");
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_recent_search(move |term| {
            let app = a.borrow_mut();
            app.settings.save_recent_search(&term);
            let searches: Vec<SharedString> = app.settings.recent_searches().into_iter().map(ss).collect();
            app.ui.global::<State>().set_recent_searches(ui_model(searches).into());
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_clear_recent_searches(move || a.borrow_mut().clear_recent_searches());
    }

    // Playback.
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_track(move |id| {
            let app = a.borrow();
            if let Some(track) = app.track_by_id(&id) {
                drop(app);
                let view_paths = a.borrow().view_paths.clone();
                let mut app = a.borrow_mut();
                app.coordinator.queue_mut().replace(view_paths.clone(), None);
                app.play_track(track, false, None);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_view(move |view| {
            a.borrow_mut().set_view(&view, "", "", "");
            let paths = a.borrow().view_paths.clone();
            if let Some(path) = paths.first() {
                if let Some(track) = a.borrow().store.get_track(path).ok().flatten() {
                    a.borrow_mut().play_track(track, true, Some(0));
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_group(move |column, value| {
            let mut app = a.borrow_mut();
            app.set_view(match column.as_str() {
                "album" => "Album", "artist" => "Artist", "genre" => "Genre", _ => "Folder",
            }, "", &value, "");
            let paths = app.view_paths.clone();
            if let Some(path) = paths.first() {
                if let Some(track) = app.store.get_track(path).ok().flatten() {
                    app.play_track(track, true, Some(0));
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_group(move |column, value| {
            let view = match column.as_str() {
                "album" => "Album", "artist" => "Artist", "genre" => "Genre", _ => "Folder",
            };
            a.borrow_mut().set_view(view, "", &value, "");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_group_shuffle(move |column, value| {
            let view = match column.as_str() {
                "album" => "Album", "artist" => "Artist", "genre" => "Genre", _ => "Folder",
            };
            let mut app = a.borrow_mut();
            app.set_view(view, "", &value, "");
            let paths = app.view_paths.clone();
            if !paths.is_empty() {
                app.coordinator.queue_mut().replace(paths.clone(), Some(0));
                app.coordinator.queue_mut().shuffle_upcoming(1);
                if let Some(track) = app.store.get_track(&paths[0]).ok().flatten() {
                    app.play_track(track, false, Some(0));
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
            let desc = if column == current { !state.get_view_descending() } else { false };
            state.set_view_sort(ss(&column));
            state.set_view_descending(desc);
            drop(state);
            app.tracks_offset = 0;
            app.refresh_view();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_playlist(move |id| a.borrow_mut().play_playlist(&id, false));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_playlist_shuffle(move |id| a.borrow_mut().play_playlist(&id, true));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_queue_entry(move |index| {
            let mut app = a.borrow_mut();
            let path = app.coordinator.queue().entries().get(index as usize).cloned();
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
        ui.global::<Actions>().on_toggle_play(move || {
            let mut app = a.borrow_mut();
            if app.current.is_none() {
                let paths = app.view_paths.clone();
                if let Some(path) = paths.first() {
                    if let Some(track) = app.store.get_track(path).ok().flatten() {
                        app.coordinator.queue_mut().replace(paths, Some(0));
                        app.play_track(track, false, Some(0));
                        return;
                    }
                }
                return;
            }
            if app.engine.is_playing() {
                app.cancel_crossfade();
            }
            let is_playing = app.engine.is_playing();
            let position = app.engine.position_ms();
            let duration = app.engine.duration_ms();
            let has_ended = duration > 0 && position >= duration;
            let current = app.current.clone();
            let resolver = app.resolver();
            let result = if is_playing {
                app.coordinator.pause(current.as_ref())
            } else if has_ended || (duration > 0 && position + 250 >= duration) {
                let index = app.coordinator.queue().current_index();
                app.coordinator.start_track(current.as_ref().unwrap(), false, index, &[], &resolver)
            } else {
                app.coordinator.resume(current.as_ref())
            };
            app.apply_result(result);
        });
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
            let _ = app.engine.stop();
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_next(move || a.borrow_mut().advance_queue(false));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_previous(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            let position = app.engine.position_ms();
            let current = app.current.clone();
            let resolver = app.resolver();
            let result = app.coordinator.previous(current.as_ref(), position, &resolver);
            app.apply_result(result);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_seek_fraction(move |fraction| {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            let duration = app.engine.duration_ms();
            let target = (duration as f64 * fraction as f64).max(0.0) as u64;
            let _ = app.engine.seek(target);
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_volume_changed(move |value| {
            let mut app = a.borrow_mut();
            app.volume = (value as u32).clamp(0, 100);
            let vol = app.volume; let _ = app.engine.set_volume(vol);
            app.pending_volume = Some(app.volume);
            app.last_vol_debounce = Instant::now();
            app.publish_playback();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_volume_muted(move || {
            let mut app = a.borrow_mut();
            app.volume = if app.volume > 0 { 0 } else { 75 };
            let vol = app.volume; let _ = app.engine.set_volume(vol);
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
            app.coordinator.queue_mut().toggle_shuffle(&paths, current.as_deref());
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
        ui.global::<Actions>().on_toggle_ab(move || {
            let mut app = a.borrow_mut();
            app.cancel_crossfade();
            let position = app.engine.position_ms();
            let queue = app.coordinator.queue_mut();
            let (a_ms, b_ms) = (queue.repeat_a_ms(), queue.repeat_b_ms());
            let next = match (a_ms, b_ms) {
                (None, _) => (Some(position), None),
                (Some(_), None) if position > 0 => (Some(a_ms.unwrap()), Some(position)),
                _ => (None, None),
            };
            queue.set_ab_repeat_points(next.0, next.1);

            let state = app.ui.global::<State>();
            state.set_repeat_a(ss(next.0.map(fmt_time).unwrap_or_default()));
            state.set_repeat_b(ss(next.1.map(fmt_time).unwrap_or_default()));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_favorite(move |id| a.borrow_mut().toggle_favorite(&id));
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
    {
        let a = app.clone();
        ui.global::<Actions>().on_queue_move(move |index, target| {
            let mut app = a.borrow_mut();
            if app.coordinator.queue_mut().move_entry_to(index as usize, target as usize) {
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
        ui.global::<Actions>().on_clear_queue(move || a.borrow_mut().clear_queue());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_add_to_queue(move |id| {
            let mut app = a.borrow_mut();
            if let Some(track) = app.track_by_id(&id) {
                app.coordinator.queue_mut().append_one(track.path);
                app.publish_queue(true);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_play_next_in_queue(move |id| {
            let mut app = a.borrow_mut();
            if let Some(track) = app.track_by_id(&id) {
                app.coordinator.queue_mut().insert_next(track.path);
                app.publish_queue(true);
            }
        });
    }

    // Library / scan.
    {
        let a = app.clone();
        ui.global::<Actions>().on_scan_library(move || {
            let roots = a.borrow().settings.library_roots();
            a.borrow_mut().start_scan(&roots, false);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_pause_scan(move || {
            a.borrow_mut().scan.toggle_pause();
            let mut app = a.borrow_mut();
            app.publish_scan();
            if app.scan.get_status().paused {
                app.show_notice("Scan paused.", NoticeSeverity::Informational);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_cancel_scan(move || {
            a.borrow_mut().scan.cancel();
            a.borrow_mut().ui.global::<State>().set_scan_outcome(ss("Scan cancelled · tracks already indexed were kept"));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_add_root(move |path| a.borrow_mut().add_root(&path));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_remove_root(move |path| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::RemoveRoot(path.to_string()));
            app.ui.global::<State>().set_confirm_message(ss(format!("Remove {} from the library? Indexed tracks under this folder will be removed.", path)));
            app.ui.global::<State>().set_confirm_danger(true);
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_rescan_root(move |path| {
            a.borrow_mut().start_scan(&[path.to_string()], true);
        });
    }

    // Metadata / tags / files.
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_details(move |id| a.borrow_mut().open_details(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_tags(move |id| a.borrow_mut().open_tags(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_tags(move || a.borrow_mut().save_tags());
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
                        &TagEdit { artwork_path: Some(path), ..TagEdit::default() },
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
                    .map(|b| BackupItem { label: ss(file_name(&b.backup_path)) })
                    .collect();
                app.ui.global::<State>().set_detail_backups(ui_model(backups).into());
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
        ui.global::<Actions>().on_open_file_location_at(move |path| open_in_explorer(&path));
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
        ui.global::<Actions>().on_confirm_restore_backup(move |label| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::RestoreBackup(label.to_string()));
            app.ui.global::<State>().set_confirm_message(ss(format!("Restore tag backup {}?", file_name(&label))));
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
        ui.global::<Actions>().on_open_find_lyrics(move |id| a.borrow_mut().open_find_lyrics(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_find_lyrics(move || a.borrow_mut().find_lyrics());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_use_lyric_result(move |id, timed| a.borrow_mut().use_lyric_result(&id, timed));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_edit_lyrics(move || a.borrow_mut().open_edit_lyrics());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_lyrics_modal(move || a.borrow_mut().save_lyrics_modal());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_lyrics(move |text, sidecar, offset| {
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
            app.ui.global::<State>().set_confirm_message(ss("Remove the current lyrics?"));
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_stamp_lyric_position(move || a.borrow_mut().stamp_lyric_position());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_seek_to_lyric(move |index| a.borrow_mut().seek_to_lyric(index as usize));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_lyrics_scroll_changed(move |paused| {
            a.borrow().ui.global::<State>().set_lyrics_scroll_paused(paused);
        });
    }

    // Playlists.
    {
        let a = app.clone();
        ui.global::<Actions>().on_create_playlist(move |name| a.borrow_mut().create_playlist(&name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_rename_playlist(move |id, name| a.borrow_mut().rename_playlist(&id, &name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_delete_playlist(move |id| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::DeletePlaylist(id.to_string()));
            app.ui.global::<State>().set_confirm_message(ss("Delete this playlist? The tracks stay in your library."));
            app.ui.global::<State>().set_confirm_danger(true);
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_add_track_to_playlist(move |track_id, playlist_name| {
            let mut app = a.borrow_mut();
            let Some(id) = app.playlists.iter().find(|p| playlist_name == p.name).map(|p| p.id.clone()) else { return };
            if let Some(track) = app.track_by_id(&track_id) {
                if app.store.add_tracks_to_playlist(&id, &[track.path]).is_ok() {
                    app.refresh_playlists();
                    app.close_modal();
                    app.show_notice("Added to playlist.", NoticeSeverity::Success);
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_remove_from_playlist(move |position| {
            let mut app = a.borrow_mut();
            let id = app.ui.global::<State>().get_view_playlist_id().to_string();
            if app.store.remove_from_playlist(&id, position as u32).unwrap_or(false) {
                app.refresh_view();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_playlist_move(move |from, to| {
            let mut app = a.borrow_mut();
            let id = app.ui.global::<State>().get_view_playlist_id().to_string();
            if app.store.move_playlist_track(&id, from as u32, to as u32).unwrap_or(false) {
                app.refresh_view();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_import_m3u(move || {
            if let Some(path) = pick_file("M3U playlists (*.m3u;*.m3u8)|*.m3u;*.m3u8") {
                let name = file_stem(&path);
                if let Ok(paths) = music_player_core::import_m3u8(
                    Path::new(&path),
                    |p| music_player_core::is_supported_audio(Path::new(p)),
                ) {
                    let mut app = a.borrow_mut();
                    let id = app.new_playlist_id();
                    if app.store.create_playlist(&id, &name).is_ok() {
                        let _ = app.store.add_tracks_to_playlist(&id, &paths);
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
            let Some(playlist) = a.borrow().store.get_playlist(&id).ok().flatten() else { return };
            let tracks: Vec<Track> = playlist
                .paths
                .iter()
                .filter_map(|p| a.borrow().store.get_track(p).ok().flatten())
                .collect();
            if let Some(path) = save_file(&playlist.name, "M3U8 playlist (*.m3u8)|*.m3u8") {
                let _ = music_player_core::export_m3u8(&playlist, &tracks, Path::new(&path));
                a.borrow_mut().show_notice("Playlist exported.", NoticeSeverity::Success);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_add_to_playlist(move |id| {
            a.borrow_mut().open_add_to_playlist(&id);
        });
    }

    // Audio / EQ.
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_eq_band(move |index, value| {
            let mut app = a.borrow_mut();
            if (index as usize) < app.eq_bands.len() {
                app.eq_bands[index as usize] = value;
                app.eq_dirty = true;
                app.last_vol_debounce = Instant::now();
                app.apply_equalizer();
                app.publish_eq();
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_eq_preamp(move |value| {
            let mut app = a.borrow_mut();
            app.eq_preamp = value;
            app.eq_dirty = true;
            app.last_vol_debounce = Instant::now();
            app.apply_equalizer();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_eq_preset(move |name| a.borrow_mut().set_eq_preset(&name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_eq_preset(move |name| {
            let mut app = a.borrow_mut();
            if name.trim().is_empty() { return; }
            app.settings.save_eq_preset(&name, app.eq_bands, app.eq_preamp);
            app.close_modal();
            app.publish_eq();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_output_device(move |name| {
            a.borrow().settings.set_raw(music_player_core::settings::keys::AUDIO_OUTPUT_NAME, &name);
        });
    }

    // Settings.
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_setting(move |name, value| a.borrow_mut().set_setting(&name, &value));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_accent_preset(move |name| {
            let color = match name.as_str() {
                "Sky" => "#4ab8ff", "Violet" => "#b48cff", "Rose" => "#ff7ba6", "Amber" => "#ffc24a",
                _ => "#b7ff2d",
            };
            a.borrow().settings.set_raw(music_player_core::settings::keys::ACCENT_COLOR, color);
            a.borrow().ui.global::<State>().set_accent(parse_hex_color(color));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_crossfade_label(move |label| a.borrow_mut().set_crossfade_label(&label));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_reset_settings(move |groups| {
            a.borrow_mut().settings.reset_groups(&groups.split(',').map(str::to_string).collect::<Vec<_>>());
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
                if !dirs.contains(&path) { dirs.push(path); }
                app.settings.set_ignored_directories(&dirs);
                app.publish_exclusions();
                let roots = app.settings.library_roots();
                app.reconfigure_watcher(&roots);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_extension_enabled(move |ext, enabled| {
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
            let ext = state.get_exclusion_new_extension().to_string().trim().to_lowercase();
            let kind = state.get_exclusion_new_kind().to_string();
            drop(state);
            let valid = ext.starts_with('.')
                && ext.len() >= 2
                && ext.len() <= 13
                && ext[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
            if !valid {
                app.show_notice("Use an extension like .mkv (letters and digits only).", NoticeSeverity::Warning);
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
        ui.global::<Actions>().on_remove_ignored_directory(move |dir| {
            let mut app = a.borrow_mut();
            let dirs: Vec<String> = app.settings.ignored_directories().into_iter().filter(|d| d.as_str() != dir.as_str()).collect();
            app.settings.set_ignored_directories(&dirs);
            app.publish_exclusions();
        });
    }

    // Duplicates.
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_duplicates(move |sort, desc, page| {
            a.borrow_mut().load_duplicates(&sort, desc, page as usize);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more_duplicates(move || {
            let mut app = a.borrow_mut();
            let page = app.dup_page + 1;
            { let sort = app.dup_sort.clone(); let desc = app.dup_desc; app.load_duplicates(&sort, desc, page); }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_duplicate_files(move |id| a.borrow_mut().open_duplicate_files(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_duplicate_files(move || {
            a.borrow().ui.global::<State>().set_duplicate_files(ui_model(Vec::<TrackItem>::new()).into());
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_prepare_duplicates(move || {
            let mut app = a.borrow_mut();
            match app.store.prepare_duplicate_fingerprints() {
                Ok(_) => {
                    { let sort = app.dup_sort.clone(); let desc = app.dup_desc; app.load_duplicates(&sort, desc, 0); }
                    app.ui.global::<State>().set_duplicate_files(ui_model(Vec::<TrackItem>::new()).into());
                }
                Err(err) => {
                    LocalAppLog::shared().error("duplicates", "fingerprint failed", Some(&err));
                    app.show_notice("Duplicates could not be prepared.", NoticeSeverity::Error);
                }
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more(move || {
            let mut app = a.borrow_mut();
            app.tracks_offset += 100;
            app.refresh_view();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more_groups(move || {
            let mut app = a.borrow_mut();
            app.groups_offset += 60;
            app.refresh_view();
        });
    }

    // Context menu.
    {
        let a = app.clone();
        ui.global::<Actions>().on_open_context_menu(move |id, x, y| a.borrow_mut().open_context_menu(&id, x, y));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_context_action(move |index| a.borrow_mut().context_action(index as usize));
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
        ui.global::<Actions>().on_open_modal(move |name| a.borrow_mut().open_modal(&name));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_modal(move || a.borrow_mut().close_modal());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_confirm_action(move || a.borrow_mut().confirm_action());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_confirm_remove_root(move |path| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::RemoveRoot(path.to_string()));
            app.ui.global::<State>().set_confirm_message(ss(format!("Remove {} from the library?", path)));
            app.ui.global::<State>().set_confirm_danger(true);
            app.open_modal("confirm");
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_rate_track(move |rating| a.borrow_mut().rate_track(rating));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_tags_field_changed(move |index, value| {
            let mut app = a.borrow_mut();
            if let Some(field) = app.tags_additional.get_mut(index as usize) { field.1 = value.to_string(); }
            app.publish_tag_fields();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_tags_custom_key_changed(move |index, value| {
            let mut app = a.borrow_mut();
            if let Some(field) = app.tags_custom.get_mut(index as usize) { field.0 = value.to_string(); }
            app.publish_tag_fields();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_tags_custom_value_changed(move |index, value| {
            let mut app = a.borrow_mut();
            if let Some(field) = app.tags_custom.get_mut(index as usize) { field.1 = value.to_string(); }
            app.publish_tag_fields();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_tags_remove_custom(move |index| {
            let mut app = a.borrow_mut();
            if (index as usize) < app.tags_custom.len() { app.tags_custom.remove(index as usize); }
            app.publish_tag_fields();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_tags_add_custom_field(move |key, value| {
            let mut app = a.borrow_mut();
            app.tags_custom.push((key.to_string(), value.to_string()));
            app.publish_tag_fields();
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
            // Wave 5 wires the Win32 show-state call; hiding is the safe fallback.
            let _ = a.borrow().ui.hide();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_toggle_immersive(move |kind| {
            let app = a.borrow_mut();
            let state = app.ui.global::<State>();
            let open = state.get_immersive_open();
            state.set_immersive_kind(ss(&kind));
            state.set_immersive_open(!open);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_immersive(move || {
            a.borrow().ui.global::<State>().set_immersive_open(false);
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
            app.settings.set_raw(music_player_core::settings::keys::RIGHT_PANEL_OPEN, &open.to_string());
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_set_right_tab(move |tab| {
            a.borrow().ui.global::<State>().set_right_panel_tab(ss(&tab));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_ui_ready(move || a.borrow_mut().startup());
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
            let url = a.borrow().settings.raw(music_player_core::settings::keys::UPDATE_RELEASE_URL)
                .unwrap_or_else(|| "https://github.com/kalabhaftu/music-player/releases".into());
            open_url(&url);
        });
    }

    // Startup and the 250 ms engine-room clock.
    app.borrow_mut().startup();
    let clock = Timer::default();
    {
        let a = app.clone();
        clock.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            a.borrow_mut().tick();
        });
    }

    ui.run()?;
    app.borrow_mut().save_session();
    Ok(())
}

