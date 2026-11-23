//! Music Player — Slint desktop host.
//!
//! Port of the C# `MainWindow.*` orchestration layer. The engine room runs on
//! the UI thread behind a 250 ms timer (`Clock_Tick`), exactly like the
//! original DispatcherTimer loop. The `core` crate remains headless.
#![allow(deprecated)]

mod controllers;
mod native_io;
mod os;
mod os_shell;
mod splash;
mod ui;
mod updates;

use ui::*;

use chrono::Timelike;
use music_player_core::audio::coordinator::{
    Notice, NoticeSeverity, PlaybackCommandKind, PlaybackCommandResult,
};
use music_player_core::audio::engine::CrossfadeOutcome;
use music_player_core::audio::equalizer::NEUTRAL_EQUALIZER_PREAMP;
use music_player_core::audio::listening::PlaybackListeningTracker;
use music_player_core::audio::PlaybackCoordinator;
use music_player_core::lyrics::lrclib::{LrclibSearchClient, LyricsSearchResult};
use music_player_core::metadata::tag_backups_dir;
use music_player_core::metadata::tag_reader::{
    read_additional_standard_fields, read_custom_fields,
};
use music_player_core::scanner::scan::default_roots;
use music_player_core::{
    artwork_cache_dir, inspect_file, parse_lrc, read_sidecar, read_tags, write_sidecar,
    ExactDuplicateGroup, IndexResult, LibraryFileWatcher, LibraryScanCoordinator, LibraryStats,
    LibraryStore, LocalAppLog, PlaybackSession, PlayerSettings, PlaylistSummary, QueueCoordinator,
    RepeatMode, ScanProgress, TagEdit, TagEditor, Track, TrackAvailabilityService, TrackQuery,
    TrackSort, EQUALIZER_BANDS,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::json;
use slint::{ComponentHandle, Image, SharedString, Timer, TimerMode};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

slint::include_modules!();

const TRACK_PAGE_SIZE: usize = 100;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: isize,
        dwAttribute: u32,
        pvAttribute: *const std::ffi::c_void,
        cbAttribute: u32,
    ) -> i32;
}
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_CAPTION_COLOR: u32 = 35;

use native_io::{
    open_folder, open_in_explorer, open_url, pick_file, pick_folder, save_file, set_clipboard,
};

#[derive(Clone)]
pub(crate) struct NavContext {
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
pub(crate) enum ConfirmAction {
    RemoveRoot(String),
    DeletePlaylist(String),
    RestoreBackup(String),
    RemoveLyrics,
}

pub(crate) struct UpdateOutcome {
    channel: String,
    result: std::result::Result<updates::UpdateCheck, updates::UpdateError>,
}

pub(crate) struct App {
    pub(crate) ui: AppWindow,
    pub(crate) store: LibraryStore,
    pub(crate) settings: PlayerSettings,
    pub(crate) engine: music_player_core::AudioEngine,
    pub(crate) coordinator: PlaybackCoordinator,
    pub(crate) listening: PlaybackListeningTracker,
    pub(crate) scan: LibraryScanCoordinator,
    pub(crate) watcher: LibraryFileWatcher,
    pub(crate) availability: TrackAvailabilityService,
    pub(crate) scan_progress_rx: Option<Receiver<ScanProgress>>,
    pub(crate) lyrics_rx: Option<Receiver<std::result::Result<Vec<LyricsSearchResult>, String>>>,
    pub(crate) auto_lyrics_rx: Option<Receiver<(String, std::result::Result<Option<music_player_core::lyrics::LyricsMatch>, String>)>>,
    pub(crate) lyrics_cache: HashMap<String, (Instant, Option<music_player_core::lyrics::LyricsMatch>)>,
    pub(crate) lyrics_results: Vec<LyricsSearchResult>,
    pub(crate) notice_timer: Timer,
    pub(crate) seek_wheel_timer: Timer,
    pub(crate) seek_preview_ms: Option<u64>,
    pub(crate) current: Option<Track>,
    pub(crate) id_to_path: HashMap<String, String>,
    pub(crate) view_paths: Vec<String>,
    pub(crate) history: Vec<NavContext>,
    pub(crate) history_pos: usize,
    pub(crate) crossfade_in_progress: bool,
    pub(crate) crossfade_source_index: Option<usize>,
    pub(crate) crossfade_failure_source: Option<String>,
    pub(crate) volume: u32,
    pub(crate) persisted_volume: i64,
    pub(crate) pending_volume: Option<u32>,
    pub(crate) last_session_save: Instant,
    pub(crate) last_wal: Instant,
    pub(crate) last_vol_debounce: Instant,
    pub(crate) last_artwork_refresh: Instant,
    pub(crate) eq_bands: [f32; 10],
    pub(crate) eq_preamp: f32,
    pub(crate) eq_dirty: bool,
    pub(crate) tracks_offset: usize,
    pub(crate) groups_offset: usize,
    pub(crate) queue_display_limit: usize,
    pub(crate) group_total: i64,
    pub(crate) dup_sort: String,
    pub(crate) dup_desc: bool,
    pub(crate) dup_page: usize,
    pub(crate) duplicate_fingerprints_ready: bool,
    pub(crate) duplicate_prepare_rx: Option<Receiver<std::result::Result<(), String>>>,
    pub(crate) details_target: Option<String>,
    pub(crate) tags_target: Option<String>,
    pub(crate) tags_additional: Vec<(String, String)>,
    pub(crate) tags_custom: Vec<(String, String)>,
    pub(crate) lyrics_target: Option<String>,
    pub(crate) lyrics_doc: Option<music_player_core::LyricsDocument>,
    pub(crate) lyrics_timed: bool,
    pub(crate) lyrics_active: i32,
    pub(crate) edit_target: Option<String>,
    pub(crate) context_target: Option<String>,
    pub(crate) confirm_action: Option<ConfirmAction>,
    pub(crate) add_playlist_track: Option<String>,
    pub(crate) playlists: Vec<PlaylistSummary>,
    pub(crate) group_items: Vec<GroupItem>,
    pub(crate) artist_album_items: Vec<GroupItem>,
    pub(crate) log_folder: PathBuf,
    pub(crate) os: Option<os::OsIntegration>,
    pub(crate) os_attached: bool,
    pub(crate) audio_devices: Vec<os_shell::AudioOutputDevice>,
    pub(crate) search_debounce: Option<(Instant, String)>,
    pub(crate) update_rx: Option<Receiver<UpdateOutcome>>,
    pub(crate) artwork_repair_rx: Option<Receiver<usize>>,
    pub(crate) unmuted_volume: Option<u32>,
    pub(crate) scan_outcome_time: Option<Instant>,
    pub(crate) track_cache: HashMap<String, Track>,
    pub(crate) startup_scan_deferred: bool,
    pub(crate) started: bool,
}

impl App {
    pub(crate) fn new(ui: AppWindow) -> Self {
        let store = LibraryStore::in_app_data().expect("could not open the library database");
        let settings = PlayerSettings::new(store.clone());
        let scan = LibraryScanCoordinator::new(&store).with_artwork_cache(artwork_cache_dir());
        let log_folder = LocalAppLog::shared().folder().to_path_buf();
        let persisted_volume = settings.int_or(music_player_core::settings::keys::VOLUME, 75);
        let volume = persisted_volume.clamp(0, 100) as u32;
        let crossfade_ms = settings.crossfade_seconds() as u64 * 1000;
        let coordinator =
            PlaybackCoordinator::with_crossfade_ms(QueueCoordinator::new(), crossfade_ms);
        let eq_bands = settings.eq_bands().unwrap_or([0.0; 10]);
        let eq_preamp = settings.eq_preamp().unwrap_or(NEUTRAL_EQUALIZER_PREAMP);
        let mut engine = music_player_core::AudioEngine::new();
        let selected_device = settings
            .raw(music_player_core::settings::keys::AUDIO_OUTPUT_DEVICE)
            .unwrap_or_default();
        if !selected_device.trim().is_empty() {
            let direct_sound =
                os_shell::direct_sound_device_id(&selected_device).unwrap_or_default();
            let _ = engine.set_audio_output_device(&direct_sound);
        }
        let now = Instant::now();
        Self {
            ui,
            availability: TrackAvailabilityService::new(&store)
                .with_artwork_directory(artwork_cache_dir()),
            store,
            settings,
            engine,
            coordinator,
            listening: PlaybackListeningTracker::new(),
            scan,
            watcher: LibraryFileWatcher::new(),
            scan_progress_rx: None,
            lyrics_rx: None,
            auto_lyrics_rx: None,
            lyrics_cache: HashMap::new(),
            lyrics_results: Vec::new(),
            notice_timer: Timer::default(),
            seek_wheel_timer: Timer::default(),
            seek_preview_ms: None,
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
            last_artwork_refresh: now,
            eq_bands,
            eq_preamp,
            eq_dirty: false,
            tracks_offset: 0,
            groups_offset: 0,
            queue_display_limit: 200,
            group_total: 0,
            dup_sort: "Title".into(),
            dup_desc: false,
            dup_page: 0,
            duplicate_fingerprints_ready: false,
            duplicate_prepare_rx: None,
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
            group_items: Vec::new(),
            artist_album_items: Vec::new(),
            log_folder,
            os: os::OsIntegration::start(),
            os_attached: false,
            audio_devices: Vec::new(),
            search_debounce: None,
            update_rx: None,
            artwork_repair_rx: None,
            unmuted_volume: None,
            scan_outcome_time: None,
            track_cache: HashMap::new(),
            startup_scan_deferred: true,
            started: false,
        }
    }
}

fn main() -> std::result::Result<(), slint::PlatformError> {
    // Single instance: a duplicate launch asks the running instance to show
    // its window and exits, handing off any files passed via CLI.
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if !os::acquire_single_instance(&cli_args) {
        return Ok(());
    }
    let startup = splash::Splash::try_show();
    if let Some(splash) = &startup {
        splash.set_status("Preparing your library…");
    }
    let ui = AppWindow::new()?;
    let app = Rc::new(RefCell::new(App::new(ui.clone_strong())));

    if !cli_args.is_empty() {
        let (verb, paths) = os::parse_activation_args(&cli_args);
        if !paths.is_empty() {
            if verb == "addtoqueue" {
                app.borrow_mut().add_external_files_to_queue(&paths);
            } else if verb == "createplaylist" {
                app.borrow_mut().create_playlist_with_external_files(&paths);
            } else {
                app.borrow_mut().play_external_files(&paths);
            }
        }
    }

    // Register all Slint action callbacks.
    ui::callbacks::register_all(&ui, app.clone());

    // Restore window dimensions from database settings, or default to middle size 1024x640.
    {
        let store = &app.borrow().store;
        let w = store
            .get_setting("window-width")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1024)
            .clamp(720, 1600);
        let h = store
            .get_setting("window-height")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(640)
            .clamp(500, 1000);
        ui.window().set_size(slint::LogicalSize::new(w as f32, h as f32));
    }

    // Show UI and dismiss splash immediately for instantaneous launch
    ui.show()?;
    drop(startup);

    // Startup runs on the first 250 ms clock tick (after the first frame), and
    // the engine-room clock drives playback from there.
    let clock = Timer::default();
    {
        let a = app.clone();
        clock.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            a.borrow_mut().tick();
        });
    }

    ui.run()?;
    os_shell::detach();
    app.borrow_mut().save_session();
    Ok(())
}
