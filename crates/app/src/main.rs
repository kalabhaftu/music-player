//! Music Player — Slint desktop host.
//!
//! Port of the C# `MainWindow.*` orchestration layer. The engine room runs on
//! the UI thread behind a 250 ms timer (`Clock_Tick`), exactly like the
//! original DispatcherTimer loop. The `core` crate remains headless.
#![allow(deprecated)]
#![windows_subsystem = "windows"]

mod controllers;
mod native_io;
mod os;
mod os_associations;
mod os_shell;
mod shell_drop;
mod splash;
mod ui;
mod updates;
mod video_window;

#[cfg(test)]
mod app_tests;

use controllers::*;
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
    artwork_cache_dir, parse_lrc, read_sidecar, write_sidecar,
    ExactDuplicateGroup, HybridMetadataEngine, IndexResult, LibraryFileWatcher,
    LibraryScanCoordinator, LibraryStats, LibraryStore, LocalAppLog, PlaybackSession,
    PlayerSettings, PlaylistSummary, QueueCoordinator, RepeatMode, ScanProgress,
    TagBackup, TagEdit, TagEditor, Track, TrackAvailabilityService, TrackQuery, TrackSort,
    EQUALIZER_BANDS,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::json;
use slint::{ComponentHandle, Image, SharedString, Timer, TimerMode};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
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

thread_local! {
    static APP_INSTANCE: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP_INSTANCE.with(|cell| {
        let binding = cell.borrow();
        let app_rc = binding.as_ref()?;
        let mut app = app_rc.try_borrow_mut().ok()?;
        Some(f(&mut *app))
    })
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
    pub(crate) prefetch_lyrics_tx: Sender<(String, Option<music_player_core::lyrics::LyricsMatch>, bool)>,
    pub(crate) prefetch_lyrics_rx: Receiver<(String, Option<music_player_core::lyrics::LyricsMatch>, bool)>,
    pub(crate) lyrics_cache: HashMap<String, (Instant, Option<music_player_core::lyrics::LyricsMatch>, bool)>,
    pub(crate) lyrics_in_flight: HashSet<String>,
    pub(crate) lyrics_results: Vec<LyricsSearchResult>,
    pub(crate) notice_timer: Timer,
    pub(crate) seek_wheel_timer: Timer,
    pub(crate) scan_timer: Timer,
    pub(crate) crossfade_timer: Timer,
    pub(crate) seek_preview_ms: Option<u64>,
    pub(crate) current: Option<Track>,
    pub(crate) id_to_path: HashMap<String, String>,
    pub(crate) view_paths: Vec<String>,
    pub(crate) history: Vec<NavContext>,
    pub(crate) history_pos: usize,
    pub(crate) crossfade_in_progress: bool,
    pub(crate) crossfade_source_index: Option<usize>,
    pub(crate) crossfade_failure_source: Option<String>,
    pub(crate) crossfade_target_path: Option<String>,
    pub(crate) volume: u32,
    pub(crate) persisted_volume: i64,
    pub(crate) pending_volume: Option<u32>,
    pub(crate) last_session_save: Instant,
    pub(crate) last_wal: Instant,
    pub(crate) last_vol_debounce: Instant,
    pub(crate) last_artwork_refresh: Instant,
    pub(crate) artwork_refresh_pending: bool,
    pub(crate) eq_preset: String,
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
    pub(crate) lyrics_raw: Option<String>,
    pub(crate) lyrics_timed: bool,
    pub(crate) lyrics_active: i32,
    pub(crate) edit_target: Option<String>,
    pub(crate) context_target: Option<String>,
    pub(crate) confirm_action: Option<ConfirmAction>,
    pub(crate) add_playlist_track: Option<String>,
    pub(crate) pending_external_playlist_paths: Option<Vec<String>>,
    pub(crate) playlists: Vec<PlaylistSummary>,
    pub(crate) group_items: Vec<GroupItem>,
    pub(crate) artist_album_items: Vec<GroupItem>,
    pub(crate) log_folder: PathBuf,
    pub(crate) os: Option<os::OsIntegration>,
    pub(crate) os_attached: bool,
    pub(crate) video_host: Option<video_window::VideoHost>,
    pub(crate) audio_devices: Vec<os_shell::AudioOutputDevice>,
    pub(crate) vlc_prep: Option<std::thread::JoinHandle<Option<music_player_core::audio::PreparedVlc>>>,
    pub(crate) deferred_audio_pending: bool,
    pub(crate) pending_session_resume: Option<(Track, u64)>,
    pub(crate) search_debounce: Option<(Instant, String)>,
    pub(crate) update_rx: Option<Receiver<UpdateOutcome>>,
    pub(crate) artwork_repair_rx: Option<Receiver<usize>>,
    pub(crate) unmuted_volume: Option<u32>,
    pub(crate) scan_outcome_time: Option<Instant>,
    pub(crate) track_cache: HashMap<String, Track>,
    pub(crate) startup_scan_deferred: bool,
    pub(crate) started: bool,
    pub(crate) allow_quit: bool,
    pub(crate) library_epoch: u64,
    pub(crate) query_generation: u64,
    pub(crate) track_view_cache: HashMap<String, (u64, Vec<TrackItem>, Vec<String>, HashMap<String, String>, i64)>,
    pub(crate) group_view_cache: HashMap<String, (u64, Vec<GroupItem>, i64)>,
    pub(crate) metadata_engine: HybridMetadataEngine,
    pub(crate) query_dispatcher: QueryDispatcher,
}

impl App {
    pub(crate) fn new(ui: AppWindow) -> Self {
        let store = LibraryStore::in_app_data().expect("could not open the library database");
        let settings = PlayerSettings::new(store.clone());
        let scan = LibraryScanCoordinator::new(&store)
            .with_artwork_cache(artwork_cache_dir())
            .with_completion_notifier(|| {
                let _ = slint::invoke_from_event_loop(|| {
                    with_app(|app| {
                        app.poll_scan();
                    });
                });
            });
        let metadata_engine = HybridMetadataEngine::new();
        let log_folder = LocalAppLog::shared().folder().to_path_buf();
        let persisted_volume = settings.int_or(music_player_core::settings::keys::VOLUME, 75);
        let volume = persisted_volume.clamp(0, 100) as u32;
        let crossfade_ms = settings.crossfade_seconds() as u64 * 1000;
        let coordinator =
            PlaybackCoordinator::with_crossfade_ms(QueueCoordinator::new(), crossfade_ms);
        let engine = music_player_core::AudioEngine::deferred();
        let mut eq_preset = settings.eq_current();
        if eq_preset.trim().is_empty() {
            eq_preset = "Off".into();
        }
        let (eq_bands, eq_preamp) = if eq_preset == "Off" {
            (
                settings.eq_bands().unwrap_or([0.0; 10]),
                settings.eq_preamp().unwrap_or(NEUTRAL_EQUALIZER_PREAMP),
            )
        } else if let Some((bands, preamp)) = settings.saved_eq_preset(&eq_preset) {
            (bands, preamp)
        } else if let Some((bands, preamp)) = engine.preset_curve(&eq_preset) {
            (bands, preamp)
        } else {
            (
                settings.eq_bands().unwrap_or([0.0; 10]),
                settings.eq_preamp().unwrap_or(NEUTRAL_EQUALIZER_PREAMP),
            )
        };
        let now = Instant::now();
        let (prefetch_lyrics_tx, prefetch_lyrics_rx) = mpsc::channel();
        static WATCHER_WAKE_PENDING: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        let watcher = LibraryFileWatcher::new().with_wake_notifier(|| {
            if !WATCHER_WAKE_PENDING.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let _ = slint::invoke_from_event_loop(|| {
                    WATCHER_WAKE_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
                    with_app(|app| {
                        app.pump_watcher();
                    });
                });
            }
        });
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
            watcher,
            scan_progress_rx: None,
            lyrics_rx: None,
            auto_lyrics_rx: None,
            prefetch_lyrics_tx,
            prefetch_lyrics_rx,
            lyrics_cache: HashMap::new(),
            lyrics_in_flight: HashSet::new(),
            lyrics_results: Vec::new(),
            notice_timer: Timer::default(),
            seek_wheel_timer: Timer::default(),
            scan_timer: Timer::default(),
            crossfade_timer: Timer::default(),
            seek_preview_ms: None,
            current: None,
            id_to_path: HashMap::new(),
            view_paths: Vec::new(),
            history: Vec::new(),
            history_pos: 0,
            crossfade_in_progress: false,
            crossfade_source_index: None,
            crossfade_failure_source: None,
            crossfade_target_path: None,
            volume,
            persisted_volume,
            pending_volume: None,
            last_session_save: now,
            last_wal: now,
            last_vol_debounce: now,
            last_artwork_refresh: now,
            artwork_refresh_pending: false,
            eq_preset,
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
            lyrics_raw: None,
            lyrics_timed: false,
            lyrics_active: -1,
            edit_target: None,
            context_target: None,
            confirm_action: None,
            add_playlist_track: None,
            pending_external_playlist_paths: None,
            playlists: Vec::new(),
            group_items: Vec::new(),
            artist_album_items: Vec::new(),
            log_folder,
            os: os::OsIntegration::start(),
            os_attached: false,
            video_host: None,
            audio_devices: Vec::new(),
            vlc_prep: None,
            deferred_audio_pending: true,
            pending_session_resume: None,
            search_debounce: None,
            update_rx: None,
            artwork_repair_rx: None,
            unmuted_volume: None,
            scan_outcome_time: None,
            track_cache: HashMap::new(),
            startup_scan_deferred: true,
            started: false,
            allow_quit: false,
            library_epoch: 0,
            query_generation: 0,
            track_view_cache: HashMap::new(),
            group_view_cache: HashMap::new(),
            metadata_engine,
            query_dispatcher: QueryDispatcher::new(),
        }
    }

    pub(crate) fn invalidate_view_caches(&mut self) {
        self.library_epoch = self.library_epoch.wrapping_add(1);
        self.query_generation = self.query_generation.wrapping_add(1);
        self.query_dispatcher.cancel_below(self.query_generation);
        self.track_view_cache.clear();
        self.group_view_cache.clear();
    }
}

fn main() -> std::result::Result<(), slint::PlatformError> {
    LocalAppLog::shared().install_panic_hook();
    let cli_args: Vec<String> = std::env::args().skip(1).collect();

    // Native file association registration and COM DropTarget local server:
    if cli_args.iter().any(|a| a == "--shell-drop-target") {
        if let Err(e) = shell_drop::run_drop_target_server() {
            LocalAppLog::shared().error("shell-selection", "Explorer drop-target server failed.", Some(&e));
            std::process::exit(1);
        }
        return Ok(());
    }
    if cli_args.iter().any(|a| a == "--register-associations" || a == "registerassociations") {
        if let Err(e) = os_associations::register_associations() {
            eprintln!("Failed to register file associations: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if cli_args.iter().any(|a| a == "--unregister-associations" || a == "unregisterassociations") {
        if let Err(e) = os_associations::unregister_associations() {
            eprintln!("Failed to unregister file associations: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // Single instance: a duplicate launch asks the running instance to show
    // its window and exits, handing off any files passed via CLI.
    if !os::acquire_single_instance(&cli_args) {
        return Ok(());
    }
    let startup = splash::Splash::try_show();
    if let Some(splash) = &startup {
        splash.set_status("Preparing your library…");
    }
    // Pre-warm LibVLC plugin initialization off the UI thread in parallel with window creation.
    let vlc_prep = std::thread::Builder::new()
        .name("vlc-prewarm".into())
        .spawn(music_player_core::AudioEngine::prepare_vlc)
        .ok();
    let ui = AppWindow::new()?;
    let app = Rc::new(RefCell::new(App::new(ui.clone_strong())));
    app.borrow_mut().vlc_prep = vlc_prep;
    APP_INSTANCE.with(|cell| *cell.borrow_mut() = Some(app.clone()));

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
            .clamp(800, 1440);
        let h = store
            .get_setting("window-height")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(640)
            .clamp(520, 920);
        ui.window().set_size(slint::LogicalSize::new(w as f32, h as f32));
    }

    // Initialize startup state before showing window so UI opens with data populated,
    // then apply CLI / Explorer file activations afterwards so `restore_session` never
    // clobbers or pauses an Explorer-launched track.
    app.borrow_mut().startup();
    if !cli_args.is_empty() {
        let (verb, paths) = os::parse_activation_args(&cli_args);
        if !paths.is_empty() {
            let mut app_mut = app.borrow_mut();
            app_mut.ensure_audio_engine_ready();
            if verb == "addtoqueue" {
                app_mut.add_external_files_to_queue(&paths);
            } else if verb == "createplaylist" {
                app_mut.create_playlist_with_external_files(&paths);
            } else {
                app_mut.play_external_files(&paths);
            }
        }
    }

    // Show UI and dismiss splash immediately for instantaneous launch
    ui.show()?;
    drop(startup);

    // Intercept window close button to keep running in the Windows notification
    // area when `minimize-to-tray` is enabled (matching TrayIconService.cs).
    {
        let a = app.clone();
        let ui_weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            let mut app_ref = a.borrow_mut();
            let minimize_to_tray = app_ref.ui.global::<State>().get_minimize_to_tray();
            if minimize_to_tray && !app_ref.allow_quit && app_ref.os.is_some() {
                app_ref.flush_pending_persistence();
                if let Some(ui) = ui_weak.upgrade() {
                    let _ = ui.hide();
                }
                slint::CloseRequestResponse::KeepWindowShown
            } else {
                slint::CloseRequestResponse::HideWindow
            }
        });
    }

    let hwnd = ui
        .window()
        .window_handle()
        .window_handle()
        .ok()
        .and_then(|handle| match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
            _ => None,
        })
        .unwrap_or(0);
    native_io::restore_window_placement(hwnd, &app.borrow().store);

    // Run heavy non-critical hydration (LibVLC attach, audio device discovery,
    // session track pre-open, smart playlist counts, background scan) right after the
    // first frame paints so the window opens nearly instantaneously.
    {
        let a = app.clone();
        Timer::single_shot(Duration::from_millis(16), move || {
            a.borrow_mut().complete_deferred_startup();
        });
    }

    {
        use slint::winit_030::{winit::event::WindowEvent, EventResult, WinitWindowAccessor};
        let drop_queue: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let drop_timer = Rc::new(Timer::default());
        let a = app.clone();
        let dq = drop_queue.clone();
        let dt = drop_timer.clone();
        ui.window().on_winit_window_event(move |_win, event| {
            if let WindowEvent::DroppedFile(path) = event {
                dq.borrow_mut().push(path.clone());
                let a2 = a.clone();
                let dq2 = dq.clone();
                dt.start(TimerMode::SingleShot, Duration::from_millis(40), move || {
                    let paths = std::mem::take(&mut *dq2.borrow_mut());
                    if !paths.is_empty() {
                        a2.borrow_mut().handle_dropped_paths(&paths);
                    }
                });
            }
            EventResult::Propagate
        });
    }

    // Engine-room clock drives playback and background pollers.
    let clock = Timer::default();
    {
        let a = app.clone();
        clock.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            a.borrow_mut().tick();
        });
    }

    ui.run()?;
    native_io::save_window_placement(hwnd, &app.borrow().store);
    os_shell::detach();
    {
        let mut app_borrow = app.borrow_mut();
        app_borrow.close_video_window(false);
        app_borrow.scan.cancel();
        app_borrow.flush_pending_persistence();
    }
    APP_INSTANCE.with(|cell| *cell.borrow_mut() = None);
    Ok(())
}
