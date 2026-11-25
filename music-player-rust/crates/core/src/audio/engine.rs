use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::equalizer::{EQUALIZER_BANDS, EQUALIZER_PREAMP_LIMIT, NEUTRAL_EQUALIZER_PREAMP};

/// Video subtitle track entry returned by LibVLC SPU enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSubtitleTrack {
    pub id: i32,
    pub name: String,
}

#[repr(C)]
struct VlcTrackDescription {
    id: i32,
    name: *const c_char,
    next: *mut VlcTrackDescription,
}

/// Opaque initialized LibVLC instance ready to attach to `AudioEngine`.
pub struct PreparedVlc(VlcContext);

// SAFETY: `PreparedVlc` contains only a freshly constructed, unplayed `libvlc_instance_t`
// handle with no active `PlayerHandle` instances or thread-bound state; moving it once
// to the owner UI thread after initialization on a background worker is safe.
unsafe impl Send for PreparedVlc {}

/// VLC Windows volume scale cubes the slider value; crossfade gains are
/// converted back into that scale (see `crossfade_volumes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

/// libvlc event type identifiers (from libvlc_events.h, VLC 3.x).
pub mod vlc_event {
    pub const MEDIA_PLAYER_TIME_CHANGED: i32 = 0x100;
    pub const MEDIA_PLAYER_LENGTH_CHANGED: i32 = 0x101;
    pub const MEDIA_PLAYER_PLAYING: i32 = 0x104;
    pub const MEDIA_PLAYER_END_REACHED: i32 = 0x106;
    pub const MEDIA_PLAYER_ENCOUNTERED_ERROR: i32 = 0x108;
    pub const MEDIA_PLAYER_STOPPED: i32 = 0x10A;
}

/// libvlc_media_player_get_state values (libvlc_state_t, VLC 3.x).
pub mod vlc_state {
    pub const NOTHING_SPECIAL: i32 = 0;
    pub const OPENING: i32 = 1;
    pub const BUFFERING: i32 = 2;
    pub const PLAYING: i32 = 3;
    pub const PAUSED: i32 = 4;
    pub const STOPPED: i32 = 5;
    pub const ENDED: i32 = 6;
    pub const ERROR: i32 = 7;
}

/// Flags queued by native VLC event callbacks. Callbacks only ever set flags;
/// the owner thread polls and consumes them (mirroring the C# QueuePlayerEvent
/// thread-pool pattern without calling back into libvlc from the callback).
#[derive(Debug, Default)]
pub struct EventFlags {
    pub end_reached: AtomicBool,
    pub encountered_error: AtomicBool,
    pub started: AtomicBool,
    /// Media generation the flags belong to; the owner bumps the engine's
    /// generation on play/seek/stop and rebinding media, so stale events can
    /// be rejected (mirrors `PlayerSubscription.Generation`).
    pub generation: AtomicU64,
}

impl EventFlags {
    fn take_end_reached(&self) -> bool {
        self.end_reached.swap(false, Ordering::SeqCst)
    }

    fn take_encountered_error(&self) -> bool {
        self.encountered_error.swap(false, Ordering::SeqCst)
    }

    fn take_started(&self) -> bool {
        self.started.swap(false, Ordering::SeqCst)
    }
}

/// Native VLC event callback shape: arg1 is the event pointer, arg2 the user
/// data attached at `libvlc_event_attach`.
type VlcEventCallback = unsafe extern "C" fn(*const c_void, *mut c_void);

// Native callbacks must leave immediately: they never call back into libvlc
// and only publish flags for the owner thread to poll.
unsafe extern "C" fn vlc_end_reached_callback(_event: *const c_void, user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let flags = &*(user_data as *const EventFlags);
    flags.end_reached.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn vlc_encountered_error_callback(_event: *const c_void, user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let flags = &*(user_data as *const EventFlags);
    flags.encountered_error.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn vlc_playing_callback(_event: *const c_void, user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let flags = &*(user_data as *const EventFlags);
    flags.started.store(true, Ordering::SeqCst);
}

struct PlayerHandle {
    player: *mut c_void,
    /// Owned by the player; never released separately.
    event_manager: *mut c_void,
    media: *mut c_void,
    /// Leaked to the native callbacks (see `create_player_handle`).
    user_data: *mut c_void,
    flags: Arc<EventFlags>,
    fn_event_detach: unsafe extern "C" fn(*mut c_void, i32, VlcEventCallback, *mut c_void),
    fn_player_stop: unsafe extern "C" fn(*mut c_void),
    fn_player_release: unsafe extern "C" fn(*mut c_void),
    fn_media_release: unsafe extern "C" fn(*mut c_void),
}

impl Drop for PlayerHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.player.is_null() {
                (self.fn_player_stop)(self.player);
            }
            if !self.event_manager.is_null() {
                (self.fn_event_detach)(
                    self.event_manager,
                    vlc_event::MEDIA_PLAYER_END_REACHED,
                    vlc_end_reached_callback,
                    self.user_data,
                );
                (self.fn_event_detach)(
                    self.event_manager,
                    vlc_event::MEDIA_PLAYER_ENCOUNTERED_ERROR,
                    vlc_encountered_error_callback,
                    self.user_data,
                );
                (self.fn_event_detach)(
                    self.event_manager,
                    vlc_event::MEDIA_PLAYER_PLAYING,
                    vlc_playing_callback,
                    self.user_data,
                );
            }
            if !self.player.is_null() {
                (self.fn_player_release)(self.player);
            }
            if !self.media.is_null() {
                (self.fn_media_release)(self.media);
            }
        }
    }
}

struct VlcContext {
    _lib: libloading::Library,
    active: Option<PlayerHandle>,
    spare: Option<PlayerHandle>,
    instance: *mut c_void,

    // Function pointers
    fn_release: unsafe extern "C" fn(*mut c_void),
    fn_media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    fn_media_release: unsafe extern "C" fn(*mut c_void),
    fn_media_add_option: unsafe extern "C" fn(*mut c_void, *const c_char),
    fn_player_new_from_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    fn_player_play: unsafe extern "C" fn(*mut c_void) -> i32,
    /// Part of the loaded symbol table; playback state changes go through
    /// `set_pause` (see `pause`).
    #[allow(dead_code)]
    fn_player_pause: unsafe extern "C" fn(*mut c_void),
    fn_player_set_pause: unsafe extern "C" fn(*mut c_void, i32),    fn_player_stop: unsafe extern "C" fn(*mut c_void),
    fn_player_release: unsafe extern "C" fn(*mut c_void),
    fn_player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    fn_player_set_time: unsafe extern "C" fn(*mut c_void, i64),
    fn_player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    fn_player_is_playing: unsafe extern "C" fn(*mut c_void) -> i32,
    fn_player_get_state: unsafe extern "C" fn(*mut c_void) -> i32,
    fn_audio_set_volume: unsafe extern "C" fn(*mut c_void, i32) -> i32,
    fn_audio_get_volume: unsafe extern "C" fn(*mut c_void) -> i32,
    fn_audio_output_set: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> i32>,
    fn_audio_output_device_set:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    fn_event_manager: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    fn_event_attach:
        unsafe extern "C" fn(*mut c_void, i32, VlcEventCallback, *mut c_void) -> i32,
    fn_event_detach: unsafe extern "C" fn(*mut c_void, i32, VlcEventCallback, *mut c_void),
    fn_event_type_name: Option<unsafe extern "C" fn(i32) -> *const c_char>,

    // Equalizer function pointers
    fn_equalizer_new: unsafe extern "C" fn() -> *mut c_void,
    fn_equalizer_new_from_preset: unsafe extern "C" fn(u32) -> *mut c_void,
    fn_equalizer_get_preamp: unsafe extern "C" fn(*mut c_void) -> f32,
    fn_equalizer_set_preamp: unsafe extern "C" fn(*mut c_void, f32) -> i32,
    fn_equalizer_get_amp: unsafe extern "C" fn(*mut c_void, u32) -> f32,
    fn_equalizer_set_amp: unsafe extern "C" fn(*mut c_void, f32, u32) -> i32,
    fn_equalizer_release: unsafe extern "C" fn(*mut c_void),
    fn_player_set_equalizer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32,
    fn_equalizer_preset_count: unsafe extern "C" fn() -> u32,
    fn_equalizer_preset_name: unsafe extern "C" fn(u32) -> *const c_char,
    fn_equalizer_band_count: unsafe extern "C" fn() -> u32,
    fn_equalizer_band_frequency: unsafe extern "C" fn(u32) -> f32,
    fn_player_set_rate: unsafe extern "C" fn(*mut c_void, f32) -> i32,
    fn_player_get_rate: unsafe extern "C" fn(*mut c_void) -> f32,

    // Video surface / subtitle / snapshot function pointers
    fn_media_player_set_hwnd: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    fn_video_get_spu: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
    fn_video_set_spu: Option<unsafe extern "C" fn(*mut c_void, i32) -> i32>,
    fn_video_get_spu_description:
        Option<unsafe extern "C" fn(*mut c_void) -> *mut VlcTrackDescription>,
    fn_track_description_list_release: Option<unsafe extern "C" fn(*mut VlcTrackDescription)>,
    fn_video_take_snapshot:
        Option<unsafe extern "C" fn(*mut c_void, u32, *const c_char, u32, u32) -> i32>,
}

impl Drop for VlcContext {
    fn drop(&mut self) {
        unsafe {
            // Drop players (which stops, detaches events, and releases) before
            // releasing the libvlc instance, and before the library unloads.
            self.spare = None;
            self.active = None;
            if !self.instance.is_null() {
                (self.fn_release)(self.instance);
            }
        }
    }
}

/// Outcome of `AudioEngine::begin_crossfade` / `AudioEngine::start_crossfade` /
/// `AudioEngine::poll_crossfade`.
#[derive(Debug, Clone, PartialEq)]
pub enum CrossfadeOutcome {
    /// Non-blocking fade initialized; caller should poll `poll_crossfade`
    /// every ~20 ms until `Committed`, `Canceled`, or `Failed`.
    InProgress,
    /// The incoming track is now active; the old player was stopped and
    /// swapped into the spare slot.
    Committed,
    /// The fade was canceled; the outgoing track keeps playing at full volume.
    Canceled,
    /// Crossfade was not attempted (no engine, no current track, zero window,
    /// or an empty tail); the caller should perform a plain start instead.
    Rejected,
    /// The fade was attempted but failed; the outgoing track was restored.
    Failed(String),
}

enum CrossfadeStage {
    Opening { started_at: Instant },
    Holding,
    Resuming { resumed_at: Instant },
    Ramping { ramp_start: Instant },
}

struct CrossfadeMachine {
    next_path: String,
    requested_fade_ms: u64,
    fade_ms: u64,
    started_generation: u64,
    stage: CrossfadeStage,
}

/// Port of `PlaybackService.CrossfadeVolumes`: equal-power cube-law gains
/// converted into VLC's cubed Windows volume scale, avoiding a deep midpoint
/// dip. Outgoing = volume * cbrt(cos(progress*pi/2)), incoming =
/// volume * cbrt(sin(progress*pi/2)).
pub fn crossfade_volumes(volume: u32, progress: f64) -> (u32, u32) {
    let volume = volume.min(100) as f64;
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return (volume as u32, 0);
    }
    if progress >= 1.0 {
        return (0, volume as u32);
    }
    let angle = progress * std::f64::consts::PI / 2.0;
    (
        (volume * angle.cos().cbrt()).round() as u32,
        (volume * angle.sin().cbrt()).round() as u32,
    )
}

pub struct AudioEngine {
    current_path: Option<String>,
    duration_ms: u64,
    volume: u32, // 0..100
    status: PlaybackStatus,
    vlc: Option<VlcContext>,
    /// Mirrors `_mediaGeneration`: bumped on play/seek/stop so stale native
    /// events can be rejected.
    media_generation: u64,
    crossfade_committed_generation: i64,
    crossfade_active: bool,
    crossfade_progress: f64,
    crossfade_machine: Option<CrossfadeMachine>,
    /// Epoch-ms monotonic window (`reconcile_until_ms`) during which freshly
    /// started DirectSound playback reconciles its actual LibVLC volume.
    reconcile_start: Option<(u64, Instant)>,
    // Remembered equalizer settings, applied to future players as well.
    equalizer_preset: Option<String>,
    equalizer_bands: Option<[f32; 10]>,
    equalizer_preamp: f32,
    /// Playback rate (0.5x to 2.0x, normal 1.0)
    playback_rate: f32,
    /// Separate playback rate remembered for video playback (0.5x to 2.0x).
    video_playback_rate: f32,
    /// Video extensions set for detecting video playback tracks.
    video_extensions: HashSet<String>,
    /// True when the active media is a video file.
    video_track: bool,
    /// Native Win32 HWND of the child video host surface (`0` when detached).
    video_hwnd: isize,
    /// DirectSound device GUID selected in Settings (empty = system default).
    audio_output_device: String,
    /// Flags used when LibVLC is unavailable and playback runs through MCI.
    fallback_flags: EventFlags,
    mci_last_position: AtomicU64,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    fn default_video_extensions() -> HashSet<String> {
        crate::scanner::scan::VIDEO_EXTENSIONS
            .iter()
            .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
            .collect()
    }

    /// Initializes LibVLC on a background thread so UI construction can run in parallel.
    pub fn prepare_vlc() -> Option<PreparedVlc> {
        Self::try_init_vlc().map(PreparedVlc)
    }

    /// Non-blocking constructor: skips synchronous `libvlc_new()` so startup can
    /// paint the main window immediately and attach `PreparedVlc` via `attach_vlc`.
    pub fn deferred() -> Self {
        Self::with_vlc(None)
    }

    pub fn new() -> Self {
        let vlc = Self::try_init_vlc();
        if vlc.is_none() {
            crate::log::LocalAppLog::shared().warning(
                "audio-engine",
                "LibVLC is unavailable. Audio playback will fall back to platform native audio (MCI).",
            );
        }
        Self::with_vlc(vlc)
    }

    fn with_vlc(vlc: Option<VlcContext>) -> Self {
        Self {
            current_path: None,
            duration_ms: 0,
            volume: 75,
            status: PlaybackStatus::Stopped,
            vlc,
            media_generation: 0,
            crossfade_committed_generation: -1,
            crossfade_active: false,
            crossfade_progress: 0.0,
            crossfade_machine: None,
            reconcile_start: None,
            equalizer_preset: None,
            equalizer_bands: None,
            equalizer_preamp: NEUTRAL_EQUALIZER_PREAMP,
            playback_rate: 1.0,
            video_playback_rate: 1.0,
            video_extensions: Self::default_video_extensions(),
            video_track: false,
            video_hwnd: 0,
            audio_output_device: String::new(),
            fallback_flags: EventFlags::default(),
            mci_last_position: AtomicU64::new(0),
        }
    }

    pub fn attach_prepared_vlc(&mut self, prepared: Option<PreparedVlc>) {
        if self.vlc.is_none() {
            self.vlc = prepared.map(|p| p.0);
        }
    }

    pub fn set_video_extensions(&mut self, extensions: &[String]) {
        let mut set = Self::default_video_extensions();
        for ext in extensions {
            let trimmed = ext.trim().to_ascii_lowercase();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('.') {
                set.insert(trimmed);
            } else {
                set.insert(format!(".{trimmed}"));
            }
        }
        self.video_extensions = set;
    }

    pub fn is_video_path(&self, path: &str) -> bool {
        let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) else {
            return false;
        };
        let dotted = format!(".{}", ext.to_ascii_lowercase());
        self.video_extensions.contains(&dotted)
    }

    pub fn is_video_mode(&self) -> bool {
        self.video_track
    }

    fn try_init_vlc() -> Option<VlcContext> {
        let mut candidates = Vec::new();

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                candidates.push(parent.join("libvlc.dll"));
                candidates.push(parent.join("libvlc").join("win-x64").join("libvlc.dll"));
                candidates.push(parent.join("libvlc").join("libvlc.dll"));

                let mut cur = parent.to_path_buf();
                for _ in 0..6 {
                    if let Some(p) = cur.parent() {
                        cur = p.to_path_buf();
                        candidates.push(cur.join("libvlc.dll"));
                        candidates.push(cur.join("artifacts").join("local-installer").join("payload-x64").join("libvlc").join("win-x64").join("libvlc.dll"));
                        candidates.push(cur.join("libvlc").join("win-x64").join("libvlc.dll"));
                    } else {
                        break;
                    }
                }
            }
        }

        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("libvlc.dll"));
            candidates.push(cwd.join("artifacts").join("local-installer").join("payload-x64").join("libvlc").join("win-x64").join("libvlc.dll"));

            let mut cur = cwd;
            for _ in 0..6 {
                if let Some(p) = cur.parent() {
                    cur = p.to_path_buf();
                    candidates.push(cur.join("libvlc.dll"));
                    candidates.push(cur.join("artifacts").join("local-installer").join("payload-x64").join("libvlc").join("win-x64").join("libvlc.dll"));
                    candidates.push(cur.join("libvlc").join("win-x64").join("libvlc.dll"));
                } else {
                    break;
                }
            }
        }

        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local_app_data).join("Programs").join("Music Player").join("libvlc").join("win-x64").join("libvlc.dll"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            candidates.push(PathBuf::from(pf).join("VideoLAN").join("VLC").join("libvlc.dll"));
        }
        if let Ok(pfx86) = std::env::var("ProgramFiles(x86)") {
            candidates.push(PathBuf::from(pfx86).join("VideoLAN").join("VLC").join("libvlc.dll"));
        }

        candidates.push(PathBuf::from("libvlc.dll"));
        candidates.push(PathBuf::from("artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"));
        candidates.push(PathBuf::from("../artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"));
        candidates.push(PathBuf::from("../../artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"));
        candidates.push(PathBuf::from("../../../artifacts/local-installer/payload-x64/libvlc/win-x64/libvlc.dll"));

        let mut dll_path = None;
        for c in candidates {
            if c.exists() {
                dll_path = Some(c);
                break;
            }
        }

        let Some(dll_path) = dll_path else {
            crate::log::LocalAppLog::shared().warning("audio-engine", "No candidate for libvlc.dll found");
            return None;
        };

        // On Windows, set DLL directory so libvlccore.dll and dependencies resolve
        #[cfg(windows)]
        if let Some(parent) = dll_path.parent() {
            use std::os::windows::ffi::OsStrExt;
            let mut wide: Vec<u16> = parent.as_os_str().encode_wide().collect();
            wide.push(0);
            #[link(name = "kernel32")]
            extern "system" {
                fn SetDllDirectoryW(lpPathName: *const u16) -> i32;
            }
            unsafe {
                SetDllDirectoryW(wide.as_ptr());
            }
        }

        // Set VLC_PLUGIN_PATH if plugins directory exists next to dll
        if let Some(parent) = dll_path.parent() {
            let plugins_dir = parent.join("plugins");
            if plugins_dir.exists() {
                std::env::set_var("VLC_PLUGIN_PATH", &plugins_dir);
            }
        }

        unsafe {
            let lib = match libloading::Library::new(&dll_path) {
                Ok(l) => l,
                Err(e) => {
                    crate::log::LocalAppLog::shared().warning(
                        "audio-engine",
                        &format!("Failed to load {:?}: {:?}", dll_path, e),
                    );
                    return None;
                }
            };
            macro_rules! get_sym {
                ($lib:expr, $name:literal) => {
                    match $lib.get($name) {
                        Ok(s) => *s,
                        Err(e) => {
                            crate::log::LocalAppLog::shared().warning(
                                "audio-engine",
                                &format!("Missing symbol {:?}: {:?}", String::from_utf8_lossy($name), e),
                            );
                            return None;
                        }
                    }
                };
            }

        let fn_new: unsafe extern "C" fn(i32, *const *const c_char) -> *mut c_void =
            get_sym!(lib, b"libvlc_new");
        let fn_release: unsafe extern "C" fn(*mut c_void) =
            get_sym!(lib, b"libvlc_release");
        let fn_media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
            get_sym!(lib, b"libvlc_media_new_path");
        let fn_media_release: unsafe extern "C" fn(*mut c_void) =
            get_sym!(lib, b"libvlc_media_release");
        let fn_media_add_option: unsafe extern "C" fn(*mut c_void, *const c_char) =
            get_sym!(lib, b"libvlc_media_add_option");
        let fn_player_new_from_media: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
            get_sym!(lib, b"libvlc_media_player_new_from_media");
        let fn_player_play: unsafe extern "C" fn(*mut c_void) -> i32 =
            get_sym!(lib, b"libvlc_media_player_play");
        let fn_player_pause: unsafe extern "C" fn(*mut c_void) =
            get_sym!(lib, b"libvlc_media_player_pause");
        let fn_player_set_pause: unsafe extern "C" fn(*mut c_void, i32) =
            get_sym!(lib, b"libvlc_media_player_set_pause");
        let fn_player_stop: unsafe extern "C" fn(*mut c_void) =
            get_sym!(lib, b"libvlc_media_player_stop");
        let fn_player_release: unsafe extern "C" fn(*mut c_void) =
            get_sym!(lib, b"libvlc_media_player_release");
        let fn_player_get_time: unsafe extern "C" fn(*mut c_void) -> i64 =
            get_sym!(lib, b"libvlc_media_player_get_time");
        let fn_player_set_time: unsafe extern "C" fn(*mut c_void, i64) =
            get_sym!(lib, b"libvlc_media_player_set_time");
        let fn_player_get_length: unsafe extern "C" fn(*mut c_void) -> i64 =
            get_sym!(lib, b"libvlc_media_player_get_length");
        let fn_player_is_playing: unsafe extern "C" fn(*mut c_void) -> i32 =
            get_sym!(lib, b"libvlc_media_player_is_playing");
        let fn_player_get_state: unsafe extern "C" fn(*mut c_void) -> i32 =
            get_sym!(lib, b"libvlc_media_player_get_state");
        let fn_audio_set_volume: unsafe extern "C" fn(*mut c_void, i32) -> i32 =
            get_sym!(lib, b"libvlc_audio_set_volume");
        let fn_audio_get_volume: unsafe extern "C" fn(*mut c_void) -> i32 =
            get_sym!(lib, b"libvlc_audio_get_volume");
        let fn_audio_output_set: Option<
            unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
        > = lib
            .get(b"libvlc_audio_output_set")
            .ok()
            .map(|symbol| *symbol);
        let fn_audio_output_device_set: Option<
            unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
        > = lib
            .get(b"libvlc_audio_output_device_set")
            .ok()
            .map(|symbol| *symbol);
        let fn_event_manager: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
            get_sym!(lib, b"libvlc_media_player_event_manager");
        let fn_event_attach:
            unsafe extern "C" fn(*mut c_void, i32, VlcEventCallback, *mut c_void) -> i32 =
                get_sym!(lib, b"libvlc_event_attach");
        let fn_event_detach:
            unsafe extern "C" fn(*mut c_void, i32, VlcEventCallback, *mut c_void) =
                get_sym!(lib, b"libvlc_event_detach");
        let fn_event_type_name: Option<unsafe extern "C" fn(i32) -> *const c_char> =
            lib.get(b"libvlc_event_type_name").ok().map(|symbol| *symbol);

        let fn_equalizer_new: unsafe extern "C" fn() -> *mut c_void =
            get_sym!(lib, b"libvlc_audio_equalizer_new");
        let fn_equalizer_new_from_preset: unsafe extern "C" fn(u32) -> *mut c_void =
            get_sym!(lib, b"libvlc_audio_equalizer_new_from_preset");
        let fn_equalizer_get_preamp: unsafe extern "C" fn(*mut c_void) -> f32 =
            get_sym!(lib, b"libvlc_audio_equalizer_get_preamp");
        let fn_equalizer_set_preamp: unsafe extern "C" fn(*mut c_void, f32) -> i32 =
            get_sym!(lib, b"libvlc_audio_equalizer_set_preamp");
        let fn_equalizer_get_amp: unsafe extern "C" fn(*mut c_void, u32) -> f32 =
            get_sym!(lib, b"libvlc_audio_equalizer_get_amp_at_index");
        let fn_equalizer_set_amp: unsafe extern "C" fn(*mut c_void, f32, u32) -> i32 =
            get_sym!(lib, b"libvlc_audio_equalizer_set_amp_at_index");
        let fn_equalizer_release: unsafe extern "C" fn(*mut c_void) =
            get_sym!(lib, b"libvlc_audio_equalizer_release");
        let fn_player_set_equalizer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32 =
            get_sym!(lib, b"libvlc_media_player_set_equalizer");
        let fn_equalizer_preset_count: unsafe extern "C" fn() -> u32 =
            get_sym!(lib, b"libvlc_audio_equalizer_get_preset_count");
        let fn_equalizer_preset_name: unsafe extern "C" fn(u32) -> *const c_char =
            get_sym!(lib, b"libvlc_audio_equalizer_get_preset_name");
        let fn_equalizer_band_count: unsafe extern "C" fn() -> u32 =
            get_sym!(lib, b"libvlc_audio_equalizer_get_band_count");
        let fn_equalizer_band_frequency: unsafe extern "C" fn(u32) -> f32 =
            get_sym!(lib, b"libvlc_audio_equalizer_get_band_frequency");
        let fn_player_set_rate: unsafe extern "C" fn(*mut c_void, f32) -> i32 =
            get_sym!(lib, b"libvlc_media_player_set_rate");
        let fn_player_get_rate: unsafe extern "C" fn(*mut c_void) -> f32 =
            get_sym!(lib, b"libvlc_media_player_get_rate");

        let fn_media_player_set_hwnd: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)> = lib
            .get(b"libvlc_media_player_set_hwnd")
            .ok()
            .map(|symbol| *symbol);
        let fn_video_get_spu: Option<unsafe extern "C" fn(*mut c_void) -> i32> = lib
            .get(b"libvlc_video_get_spu")
            .ok()
            .map(|symbol| *symbol);
        let fn_video_set_spu: Option<unsafe extern "C" fn(*mut c_void, i32) -> i32> = lib
            .get(b"libvlc_video_set_spu")
            .ok()
            .map(|symbol| *symbol);
        let fn_video_get_spu_description: Option<
            unsafe extern "C" fn(*mut c_void) -> *mut VlcTrackDescription,
        > = lib
            .get(b"libvlc_video_get_spu_description")
            .ok()
            .map(|symbol| *symbol);
        let fn_track_description_list_release: Option<
            unsafe extern "C" fn(*mut VlcTrackDescription),
        > = lib
            .get(b"libvlc_track_description_list_release")
            .ok()
            .map(|symbol| *symbol);
        let fn_video_take_snapshot: Option<
            unsafe extern "C" fn(*mut c_void, u32, *const c_char, u32, u32) -> i32,
        > = lib
            .get(b"libvlc_video_take_snapshot")
            .ok()
            .map(|symbol| *symbol);

        const ARGS: &[&[u8]] = &[
            b"--aout=directsound\0",
            b"--directx-volume=0\0",
            b"--no-video-title-show\0",
            b"--no-volume-save\0",
            b"--no-equalizer-2pass\0",
            b"--no-equalizer-vlcfreqs\0",
        ];
        let argv: Vec<*const c_char> = ARGS.iter().map(|s| s.as_ptr() as *const c_char).collect();
        let mut instance = fn_new(argv.len() as i32, argv.as_ptr());
        if instance.is_null() {
            instance = fn_new(0, std::ptr::null());
        }
        if instance.is_null() {
            eprintln!("[AudioEngine] fn_new returned null instance!");
            return None;
        }

            Some(VlcContext {
                _lib: lib,
                active: None,
                spare: None,
                instance,
                fn_release,
                fn_media_new_path,
                fn_media_release,
                fn_media_add_option,
                fn_player_new_from_media,
                fn_player_play,
                fn_player_pause,
                fn_player_set_pause,
                fn_player_stop,
                fn_player_release,
                fn_player_get_time,
                fn_player_set_time,
                fn_player_get_length,
                fn_player_is_playing,
                fn_player_get_state,
                fn_audio_set_volume,
                fn_audio_get_volume,
                fn_audio_output_set,
                fn_audio_output_device_set,
                fn_event_manager,
                fn_event_attach,
                fn_event_detach,
                fn_event_type_name,
                fn_equalizer_new,
                fn_equalizer_new_from_preset,
                fn_equalizer_get_preamp,
                fn_equalizer_set_preamp,
                fn_equalizer_get_amp,
                fn_equalizer_set_amp,
                fn_equalizer_release,
                fn_player_set_equalizer,
                fn_equalizer_preset_count,
                fn_equalizer_preset_name,
                fn_equalizer_band_count,
                fn_equalizer_band_frequency,
                fn_player_set_rate,
                fn_player_get_rate,
                fn_media_player_set_hwnd,
                fn_video_get_spu,
                fn_video_set_spu,
                fn_video_get_spu_description,
                fn_track_description_list_release,
                fn_video_take_snapshot,
            })
        }
    }

    /// Applies the DirectSound (audio) or MMDevice (video) output/device
    /// selection to a freshly created player, mirroring `ConfigureAudioOutputCore`.
    unsafe fn apply_audio_output(
        vlc: &VlcContext,
        player: *mut c_void,
        device: &str,
        is_video: bool,
    ) {
        if is_video {
            if let Some(output_set) = vlc.fn_audio_output_set {
                let name = CString::new("mmdevice").unwrap();
                let _ = (output_set)(player, name.as_ptr());
            }
            return;
        }
        if let Some(device_set) = vlc.fn_audio_output_device_set {
            let module = CString::new("directx").unwrap();
            let device = CString::new(device).unwrap_or_default();
            (device_set)(player, module.as_ptr(), device.as_ptr());
        }
        if let Some(output_set) = vlc.fn_audio_output_set {
            let name = CString::new("directsound").unwrap();
            let _ = (output_set)(player, name.as_ptr());
        }
    }

    fn find_nearby_subtitle_files(video_path: &str) -> Vec<PathBuf> {
        let p = Path::new(video_path);
        let Some(dir) = p.parent() else {
            return Vec::new();
        };
        let Some(media_name) = p.file_stem().and_then(|s| s.to_str()) else {
            return Vec::new();
        };
        if media_name.trim().is_empty() || !dir.is_dir() {
            return Vec::new();
        }
        let media_lower = media_name.to_ascii_lowercase();
        let dot_prefix = format!("{media_lower}.");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut exact = Vec::new();
        let mut prefixed = Vec::new();
        for entry in entries.flatten() {
            let candidate = entry.path();
            if !candidate.is_file() {
                continue;
            }
            let Some(ext) = candidate.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext.to_ascii_lowercase().as_str(), "srt" | "ass" | "ssa" | "vtt") {
                continue;
            }
            let Some(sub_stem) = candidate.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let sub_lower = sub_stem.to_ascii_lowercase();
            if sub_lower == media_lower {
                exact.push(candidate);
            } else if sub_lower.starts_with(&dot_prefix) {
                prefixed.push(candidate);
            }
        }
        exact.sort_by(|a, b| a.to_string_lossy().to_lowercase().cmp(&b.to_string_lossy().to_lowercase()));
        prefixed.sort_by(|a, b| a.to_string_lossy().to_lowercase().cmp(&b.to_string_lossy().to_lowercase()));
        exact.extend(prefixed);
        exact.truncate(8);
        exact
    }

    /// Creates a player for `media` and attaches EndReached, EncounteredError,
    /// and Playing callbacks whose user data is a leaked `Arc<EventFlags>`.
    unsafe fn create_player_handle(vlc: &VlcContext, media: *mut c_void) -> Option<PlayerHandle> {
        let player = (vlc.fn_player_new_from_media)(media);
        if player.is_null() {
            return None;
        }
        let event_manager = (vlc.fn_event_manager)(player);
        let flags = Arc::new(EventFlags::default());
        // The callbacks only touch the flags through this pointer, which is
        // intentionally leaked: detaching happens in Drop before the player is
        // released, after which no callback can run again.
        let user_data = Arc::into_raw(flags.clone()) as *mut c_void;
        if !event_manager.is_null() {
            (vlc.fn_event_attach)(
                event_manager,
                vlc_event::MEDIA_PLAYER_END_REACHED,
                vlc_end_reached_callback,
                user_data,
            );
            (vlc.fn_event_attach)(
                event_manager,
                vlc_event::MEDIA_PLAYER_ENCOUNTERED_ERROR,
                vlc_encountered_error_callback,
                user_data,
            );
            (vlc.fn_event_attach)(
                event_manager,
                vlc_event::MEDIA_PLAYER_PLAYING,
                vlc_playing_callback,
                user_data,
            );
        }
        Some(PlayerHandle {
            player,
            event_manager,
            media,
            user_data,
            flags,
            fn_event_detach: vlc.fn_event_detach,
            fn_player_stop: vlc.fn_player_stop,
            fn_player_release: vlc.fn_player_release,
            fn_media_release: vlc.fn_media_release,
        })
    }

    /// Creates media for a path; `.ogg`/`.oga` get VLC's bundled FFmpeg
    /// demuxer because the native Ogg reader can report EOF right after a
    /// time seek (mirrors `SetMedia`).
    unsafe fn create_media(vlc: &VlcContext, path: &str, is_video: bool) -> Result<*mut c_void, String> {
        let c_path = CString::new(path.as_bytes()).map_err(|e| format!("Invalid path: {}", e))?;
        let media = (vlc.fn_media_new_path)(vlc.instance, c_path.as_ptr());
        if media.is_null() {
            return Err("Failed to create LibVLC media".into());
        }
        if matches!(
            Path::new(path)
                .extension()
                .map(|ext| ext.to_string_lossy().to_ascii_lowercase()),
            Some(ref ext) if ext == "ogg" || ext == "oga"
        ) {
            let option = CString::new(":demux=avformat").unwrap();
            (vlc.fn_media_add_option)(media, option.as_ptr());
        }
        if is_video {
            for sub in Self::find_nearby_subtitle_files(path) {
                if let Ok(opt) = CString::new(format!(":sub-file={}", sub.to_string_lossy())) {
                    (vlc.fn_media_add_option)(media, opt.as_ptr());
                }
            }
        }
        Ok(media)
    }

    pub fn play_track<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        let path_str = path_ref.to_string_lossy().to_string();
        let is_video = self.is_video_path(&path_str);

        self.cancel_crossfade_core();
        self.media_generation += 1;
        let generation = self.media_generation;
        let volume = self.volume;
        let eq_preset = self.equalizer_preset.clone();
        let eq_bands = self.equalizer_bands;
        let eq_preamp = self.equalizer_preamp;
        let rate = if is_video {
            self.video_playback_rate
        } else {
            self.playback_rate
        };
        let video_hwnd = if is_video { self.video_hwnd } else { 0 };
        self.video_track = is_video;

        if let Some(ref mut vlc) = self.vlc {
            unsafe {
                // Release the previous player and media (stop, detach, release).
                vlc.active = None;

                let media = Self::create_media(vlc, &path_str, is_video)?;
                let handle = match Self::create_player_handle(vlc, media) {
                    Some(handle) => handle,
                    None => {
                        (vlc.fn_media_release)(media);
                        return Err("Failed to create LibVLC media player".into());
                    }
                };
                handle.flags.generation.store(generation, Ordering::SeqCst);

                Self::apply_audio_output(vlc, handle.player, &self.audio_output_device, is_video);
                if let Some(set_hwnd) = vlc.fn_media_player_set_hwnd {
                    (set_hwnd)(handle.player, video_hwnd as *mut c_void);
                }
                (vlc.fn_audio_set_volume)(handle.player, volume as i32);
                (vlc.fn_player_set_rate)(handle.player, rate);
                Self::apply_equalizer_to_player(vlc, &eq_preset, &eq_bands, eq_preamp, handle.player);

                let played = (vlc.fn_player_play)(handle.player) == 0;
                vlc.active = Some(handle);

                if !played {
                    self.status = PlaybackStatus::Stopped;
                    return Err("LibVLC could not start playback".into());
                }
            }

            self.current_path = Some(path_str);
            self.status = PlaybackStatus::Playing;
            self.reconcile_start = Some((generation, Instant::now()));
            return Ok(());
        }

        self.fallback_flags.end_reached.store(false, Ordering::SeqCst);
        self.fallback_flags.encountered_error.store(false, Ordering::SeqCst);
        self.fallback_flags.started.store(false, Ordering::SeqCst);
        self.fallback_flags
            .generation
            .store(self.media_generation, Ordering::SeqCst);
        self.mci_last_position.store(0, Ordering::SeqCst);

        // Fallback to MCI on Windows if LibVLC is not available
        #[cfg(windows)]
        {
            let _ = Self::send_mci("close mp_audio");
            let mci_target = Self::get_short_path(&path_str).unwrap_or_else(|| path_str.clone());
            let open_cmd = format!("open \"{}\" type mpegvideo alias mp_audio", mci_target);
            if let Err(e) = Self::send_mci(&open_cmd) {
                let fallback_cmd = format!("open \"{}\" alias mp_audio", mci_target);
                Self::send_mci(&fallback_cmd).map_err(|e2| format!("{}: {}", e, e2))?;
            }
            let _ = Self::send_mci("set mp_audio time format milliseconds");
            if let Ok(len_str) = Self::send_mci("status mp_audio length") {
                if let Ok(len) = len_str.trim().parse::<u64>() {
                    self.duration_ms = len;
                }
            }
            let _ = Self::send_mci("play mp_audio");
            let _ = self.set_volume(self.volume);
            self.current_path = Some(path_str);
            self.status = PlaybackStatus::Playing;
            Ok(())
        }

        #[cfg(not(windows))]
        {
            self.current_path = Some(path_str);
            self.status = PlaybackStatus::Playing;
            Ok(())
        }
    }

    /// Selects the LibVLC DirectSound render endpoint (`""` = system default).
    /// DirectSound picks the endpoint when the stream is created, so an active
    /// song is reopened at the same position (port of PlaybackService.
    /// SelectAudioOutputDevice).
    pub fn set_audio_output_device(&mut self, device_id: &str) -> Result<(), String> {
        let device_id = device_id.trim().to_string();
        if self.audio_output_device == device_id {
            return Ok(());
        }
        self.audio_output_device = device_id.clone();
        if self.vlc.is_none() || self.current_path.is_none() {
            return Ok(());
        }

        let position = self.position_ms();
        let was_playing = self.is_playing();
        self.cancel_crossfade_core();
        self.media_generation += 1;
        let generation = self.media_generation;
        let volume = self.volume;
        let eq_preset = self.equalizer_preset.clone();
        let eq_bands = self.equalizer_bands;
        let eq_preamp = self.equalizer_preamp;
        let path = self.current_path.clone().unwrap_or_default();

        let Some(vlc) = self.vlc.as_mut() else {
            return Ok(());
        };
        let device = self.audio_output_device.clone();
        let is_video = self.video_track;
        let video_hwnd = if is_video { self.video_hwnd } else { 0 };
        let rate = if is_video {
            self.video_playback_rate
        } else {
            self.playback_rate
        };
        unsafe {
            vlc.active = None;
            let media = Self::create_media(vlc, &path, is_video)?;
            let handle = match Self::create_player_handle(vlc, media) {
                Some(handle) => handle,
                None => {
                    (vlc.fn_media_release)(media);
                    return Err("Failed to create LibVLC media player".into());
                }
            };
            handle.flags.generation.store(generation, Ordering::SeqCst);
            Self::apply_audio_output(vlc, handle.player, &device, is_video);
            if let Some(set_hwnd) = vlc.fn_media_player_set_hwnd {
                (set_hwnd)(handle.player, video_hwnd as *mut c_void);
            }
            (vlc.fn_audio_set_volume)(handle.player, volume as i32);
            (vlc.fn_player_set_rate)(handle.player, rate);
            Self::apply_equalizer_to_player(vlc, &eq_preset, &eq_bands, eq_preamp, handle.player);

            if (vlc.fn_player_play)(handle.player) != 0 {
                self.status = PlaybackStatus::Stopped;
                return Err("LibVLC could not reopen audio on the selected output".into());
            }
            if !was_playing {
                (vlc.fn_player_set_pause)(handle.player, 1);
            }
            if position > 0 {
                (vlc.fn_player_set_time)(handle.player, position as i64);
            }
            vlc.active = Some(handle);
        }
        self.status = if was_playing {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        };
        self.reconcile_start = Some((generation, Instant::now()));
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.cancel_crossfade_core();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    // Pause() toggles based on native playing state, so a pause
                    // request must set the desired state explicitly.
                    (vlc.fn_player_set_pause)(active.player, 1);
                }
                self.status = PlaybackStatus::Paused;
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            Self::send_mci("pause mp_audio")?;
        }
        self.status = PlaybackStatus::Paused;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    (vlc.fn_player_set_pause)(active.player, 0);
                }
                self.status = PlaybackStatus::Playing;
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            Self::send_mci("resume mp_audio")?;
        }
        self.status = PlaybackStatus::Playing;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.cancel_crossfade_core();
        self.video_track = false;
        self.media_generation += 1;
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    (vlc.fn_player_stop)(active.player);
                }
                self.status = PlaybackStatus::Stopped;
                return Ok(());
            }
        }

        self.fallback_flags
            .generation
            .store(self.media_generation, Ordering::SeqCst);
        self.fallback_flags.end_reached.store(false, Ordering::SeqCst);
        self.mci_last_position.store(0, Ordering::SeqCst);

        #[cfg(windows)]
        {
            Self::send_mci("stop mp_audio")?;
        }
        self.status = PlaybackStatus::Stopped;
        Ok(())
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        self.cancel_crossfade_core();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    // A seek invalidates pending events for this media, so it
                    // bumps the generation (mirrors Seek's mediaGeneration++).
                    self.media_generation += 1;
                    active.flags.generation.store(self.media_generation, Ordering::SeqCst);
                    active.flags.end_reached.store(false, Ordering::SeqCst);
                    (vlc.fn_player_set_time)(active.player, position_ms as i64);
                }
                return Ok(());
            }
        }

        self.media_generation += 1;
        self.fallback_flags
            .generation
            .store(self.media_generation, Ordering::SeqCst);
        self.fallback_flags.end_reached.store(false, Ordering::SeqCst);
        self.mci_last_position.store(0, Ordering::SeqCst);

        #[cfg(windows)]
        {
            let cmd = format!("seek mp_audio to {}", position_ms);
            Self::send_mci(&cmd)?;
            if self.status == PlaybackStatus::Playing {
                Self::send_mci("play mp_audio")?;
            }
        }
        Ok(())
    }

    pub fn set_volume(&mut self, volume: u32) -> Result<(), String> {
        self.volume = volume.min(100);

        if self.crossfade_active {
            self.apply_crossfade_volume_core();
            return Ok(());
        }

        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    (vlc.fn_audio_set_volume)(active.player, self.volume as i32);
                }
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            let mci_vol = (self.volume as f64 * 10.0) as u32; // 0..1000
            let cmd = format!("setaudio mp_audio volume to {}", mci_vol);
            let _ = Self::send_mci(&cmd);
        }
        Ok(())
    }

    /// Returns the normalized perceived loudness power curve (0.0 to 1.0)
    /// for the current volume, matching C# `Math.Pow(volume / 100.0, 2.0)`.
    pub fn perceived_volume(&self) -> f64 {
        super::volume::quadratic_volume_curve(self.volume as f64)
    }

    /// Sets volume via perceived loudness mapping (0..=100) using the quadratic curve
    /// so that low volumes do not drop off precipitously.
    pub fn set_perceived_volume(&mut self, volume: u32) -> Result<(), String> {
        let linear = super::volume::perceived_to_linear_volume(volume);
        self.set_volume(linear)
    }

    /// Playback rate (0.5x to 2.0x) matching LibVLC rate control and C# SetVideoPlaybackRate.
    pub fn set_rate(&mut self, rate: f32) -> Result<(), String> {
        let clamped = rate.clamp(0.5, 2.0);
        self.playback_rate = clamped;

        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    if (vlc.fn_player_set_rate)(active.player, clamped) != 0 {
                        return Err("LibVLC could not set playback rate".into());
                    }
                }
            }
            if let Some(ref spare) = vlc.spare {
                unsafe {
                    let _ = (vlc.fn_player_set_rate)(spare.player, clamped);
                }
            }
            return Ok(());
        }

        #[cfg(windows)]
        {
            let mci_speed = (clamped * 1000.0) as u32; // 1000 = normal
            let cmd = format!("set mp_audio speed {}", mci_speed);
            let _ = Self::send_mci(&cmd);
        }
        Ok(())
    }

    /// Returns the current playback rate (1.0 = normal).
    pub fn rate(&self) -> f32 {
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                let r = unsafe { (vlc.fn_player_get_rate)(active.player) };
                if r > 0.0 {
                    return r;
                }
            }
        }
        self.playback_rate
    }

    pub fn set_video_surface_handle(&mut self, hwnd: isize) {
        self.video_hwnd = hwnd;
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                if let Some(set_hwnd) = vlc.fn_media_player_set_hwnd {
                    let target = if self.video_track { hwnd } else { 0 };
                    unsafe {
                        (set_hwnd)(active.player, target as *mut c_void);
                    }
                }
            }
        }
    }

    pub fn video_rate(&self) -> f32 {
        self.video_playback_rate
    }

    pub fn set_video_rate(&mut self, rate: f32) -> Result<(), String> {
        let clamped = rate.clamp(0.5, 2.0);
        self.video_playback_rate = clamped;
        if !self.video_track {
            return Ok(());
        }
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                unsafe {
                    if (vlc.fn_player_set_rate)(active.player, clamped) != 0 {
                        return Err("LibVLC could not set video playback rate".into());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn video_subtitle_id(&self) -> i32 {
        if !self.video_track {
            return -1;
        }
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                if let Some(get_spu) = vlc.fn_video_get_spu {
                    return unsafe { (get_spu)(active.player) };
                }
            }
        }
        -1
    }

    pub fn video_subtitles(&self) -> Vec<VideoSubtitleTrack> {
        if !self.video_track {
            return Vec::new();
        }
        let Some(ref vlc) = self.vlc else {
            return Vec::new();
        };
        let Some(ref active) = vlc.active else {
            return Vec::new();
        };
        let Some(get_desc) = vlc.fn_video_get_spu_description else {
            return Vec::new();
        };
        let mut tracks = Vec::new();
        unsafe {
            let head = (get_desc)(active.player);
            let mut cursor = head;
            while !cursor.is_null() {
                let entry = &*cursor;
                if entry.id != -1 {
                    let name = if entry.name.is_null() {
                        format!("Track {}", entry.id)
                    } else {
                        CStr::from_ptr(entry.name)
                            .to_string_lossy()
                            .trim()
                            .to_string()
                    };
                    tracks.push(VideoSubtitleTrack {
                        id: entry.id,
                        name: if name.is_empty() {
                            format!("Track {}", entry.id)
                        } else {
                            name
                        },
                    });
                }
                cursor = entry.next;
            }
            if !head.is_null() {
                if let Some(release) = vlc.fn_track_description_list_release {
                    (release)(head);
                }
            }
        }
        tracks
    }

    pub fn set_video_subtitle(&mut self, track_id: i32) -> bool {
        if !self.video_track {
            return false;
        }
        let Some(ref vlc) = self.vlc else {
            return false;
        };
        let Some(ref active) = vlc.active else {
            return false;
        };
        let Some(set_spu) = vlc.fn_video_set_spu else {
            return false;
        };
        unsafe { (set_spu)(active.player, track_id) == 0 }
    }

    pub fn take_video_snapshot(&self, output_path: &Path) -> bool {
        if !self.video_track {
            return false;
        }
        let Some(ref vlc) = self.vlc else {
            return false;
        };
        let Some(ref active) = vlc.active else {
            return false;
        };
        let Some(snap) = vlc.fn_video_take_snapshot else {
            return false;
        };
        if let Some(parent) = output_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(c_path) = CString::new(output_path.to_string_lossy().as_bytes()) else {
            return false;
        };
        unsafe { (snap)(active.player, 0, c_path.as_ptr(), 0, 0) == 0 }
    }

    pub fn position_ms(&self) -> u64 {
        self.poll_state();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                let pos = unsafe { (vlc.fn_player_get_time)(active.player) };
                if pos >= 0 {
                    return pos as u64;
                }
            }
        }

        #[cfg(windows)]
        {
            if let Ok(pos_str) = Self::send_mci("status mp_audio position") {
                if let Ok(pos) = pos_str.trim().parse::<u64>() {
                    return pos;
                }
            }
        }
        0
    }

    pub fn duration_ms(&self) -> u64 {
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                let len = unsafe { (vlc.fn_player_get_length)(active.player) };
                if len > 0 {
                    return len as u64;
                }
            }
        }
        self.duration_ms
    }

    pub fn is_playing(&self) -> bool {
        self.poll_state();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                let playing = unsafe { (vlc.fn_player_is_playing)(active.player) };
                return playing != 0;
            }
        }
        self.status == PlaybackStatus::Playing && !self.fallback_flags.end_reached.load(Ordering::SeqCst)
    }

    pub fn vlc_is_available(&self) -> bool {
        self.vlc.is_some()
    }

    /// Mirrors `_mediaGeneration`.
    pub fn media_generation(&self) -> u64 {
        self.media_generation
    }

    /// Whether the last completed crossfade belongs to the current media.
    pub fn has_committed_crossfade(&self) -> bool {
        self.crossfade_committed_generation == self.media_generation as i64
    }

    pub fn crossfade_active(&self) -> bool {
        self.crossfade_active
    }

    pub fn equalizer_preamp(&self) -> f32 {
        self.equalizer_preamp
    }

    /// Consumes the queued EndReached flag of the active player when it
    /// belongs to the current media generation (suppresses stale ends while
    /// a crossfade is advancing `media_generation`).
    pub fn take_end_reached(&self) -> bool {
        self.poll_state();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                if active.flags.generation.load(Ordering::SeqCst) != self.media_generation {
                    return false;
                }
                return active.flags.take_end_reached();
            }
        }
        if self.fallback_flags.generation.load(Ordering::SeqCst) != self.media_generation {
            return false;
        }
        self.fallback_flags.take_end_reached()
    }

    /// Consumes the queued EncounteredError flag of the active player when it
    /// belongs to the current media generation.
    pub fn take_encountered_error(&self) -> bool {
        self.poll_state();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                if active.flags.generation.load(Ordering::SeqCst) != self.media_generation {
                    return false;
                }
                return active.flags.take_encountered_error();
            }
        }
        if self.fallback_flags.generation.load(Ordering::SeqCst) != self.media_generation {
            return false;
        }
        self.fallback_flags.take_encountered_error()
    }

    /// Consumes the queued Playing flag of the active player.
    pub fn take_started(&self) -> bool {
        self.poll_state();
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                return active.flags.take_started();
            }
        }
        self.fallback_flags.take_started()
    }

    pub fn event_generation(&self) -> u64 {
        if let Some(ref vlc) = self.vlc {
            if let Some(ref active) = vlc.active {
                return active.flags.generation.load(Ordering::SeqCst);
            }
        }
        self.fallback_flags.generation.load(Ordering::SeqCst)
    }

    /// Resolves a libvlc event type to its canonical name, when the optional
    /// `libvlc_event_type_name` symbol is available.
    pub fn event_type_name(&self, event_type: i32) -> Option<String> {
        let vlc = self.vlc.as_ref()?;
        let describe = unsafe {
            let describe = (*vlc.fn_event_type_name.as_ref()?)(event_type);
            if describe.is_null() {
                return None;
            }
            CStr::from_ptr(describe).to_string_lossy().into_owned()
        };
        Some(describe)
    }

    /// MCI end detection for the fallback backend: while playing, if the
    /// position has stopped advancing at (or past) the cached duration, the
    /// end flag is queued so the owner thread sees a synthetic end event.
    fn poll_state(&self) {
        if self.vlc.is_some() || self.status != PlaybackStatus::Playing {
            return;
        }
        #[cfg(windows)]
        {
            let pos = match Self::send_mci("status mp_audio position") {
                Ok(text) => text.trim().parse::<u64>().unwrap_or(0),
                Err(_) => return,
            };
            let last = self.mci_last_position.load(Ordering::SeqCst);
            if pos > last {
                self.mci_last_position.store(pos, Ordering::SeqCst);
            }
            let duration = self.duration_ms;
            if duration > 0 && pos >= duration && pos == last {
                self.fallback_flags.end_reached.store(true, Ordering::SeqCst);
            }
        }
    }

    // ------------------------------------------------------------------
    // Equalizer
    // ------------------------------------------------------------------

    /// Port of `PlaybackService.EqualizerPresets`: the built-in VLC preset
    /// labels in order. Empty when LibVLC is unavailable.
    pub fn built_in_preset_names(&self) -> Vec<String> {
        let mut names: Vec<String> = super::equalizer::canonical_preset_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let Some(ref vlc) = self.vlc {
            unsafe {
                let count = (vlc.fn_equalizer_preset_count)();
                for index in 0..count {
                    let name_ptr = (vlc.fn_equalizer_preset_name)(index);
                    if !name_ptr.is_null() {
                        let name = CStr::from_ptr(name_ptr).to_string_lossy().into_owned();
                        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                            names.push(name);
                        }
                    }
                }
            }
        }
        names
    }

    /// Returns `(bands, preamp)` for a built-in preset name: prefers canonical
    /// overrides if defined (using neutral 12.0412 preamp), otherwise queries
    /// LibVLC's built-in equalizer preset at its matching index.
    pub fn preset_curve(&self, preset_name: &str) -> Option<([f32; 10], f32)> {
        if let Some(gains) = super::equalizer::canonical_preset_by_name(preset_name) {
            let preamp = self
                .vlc
                .as_ref()
                .and_then(|vlc| unsafe { Self::exact_vlc_preset_preamp(vlc, preset_name) })
                .unwrap_or(NEUTRAL_EQUALIZER_PREAMP);
            return Some((gains, preamp));
        }
        let vlc = self.vlc.as_ref()?;
        let index = Self::find_vlc_preset_index(vlc, preset_name)?;
        unsafe {
            let eq = (vlc.fn_equalizer_new_from_preset)(index);
            if eq.is_null() {
                return None;
            }
            let preamp = (vlc.fn_equalizer_get_preamp)(eq);
            let mut bands = [0.0f32; 10];
            let count = (vlc.fn_equalizer_band_count)().min(10);
            for i in 0..count {
                let raw = (vlc.fn_equalizer_get_amp)(eq, i);
                let rounded = (raw * 10.0).round() / 10.0;
                bands[i as usize] = if rounded == 0.0 { 0.0 } else { rounded };
            }
            (vlc.fn_equalizer_release)(eq);
            let rounded_preamp = (preamp * 10000.0).round() / 10000.0;
            Some((bands, rounded_preamp))
        }
    }

    unsafe fn exact_vlc_preset_preamp(vlc: &VlcContext, preset_name: &str) -> Option<f32> {
        let index = Self::find_vlc_preset_index(vlc, preset_name)?;
        let eq = (vlc.fn_equalizer_new_from_preset)(index);
        if eq.is_null() {
            return None;
        }
        let preamp = (vlc.fn_equalizer_get_preamp)(eq);
        (vlc.fn_equalizer_release)(eq);
        Some(preamp)
    }

    fn find_vlc_preset_index(vlc: &VlcContext, preset_name: &str) -> Option<u32> {
        let count = unsafe { (vlc.fn_equalizer_preset_count)() };
        for index in 0..count {
            let name_ptr = unsafe { (vlc.fn_equalizer_preset_name)(index) };
            if name_ptr.is_null() {
                continue;
            }
            let name = unsafe { CStr::from_ptr(name_ptr).to_string_lossy() };
            if name.eq_ignore_ascii_case(preset_name) {
                return Some(index);
            }
        }
        None
    }

    /// Port of `PlaybackService.EqualizerBands` semantics, as raw
    /// frequencies in Hz for the ten VLC equalizer bands.
    pub fn preset_band_frequencies(&self) -> [f32; 10] {
        let Some(ref vlc) = self.vlc else {
            return [
                EQUALIZER_BANDS[0] as f32,
                EQUALIZER_BANDS[1] as f32,
                EQUALIZER_BANDS[2] as f32,
                EQUALIZER_BANDS[3] as f32,
                EQUALIZER_BANDS[4] as f32,
                EQUALIZER_BANDS[5] as f32,
                EQUALIZER_BANDS[6] as f32,
                EQUALIZER_BANDS[7] as f32,
                EQUALIZER_BANDS[8] as f32,
                EQUALIZER_BANDS[9] as f32,
            ];
        };
        unsafe {
            let band_count = (vlc.fn_equalizer_band_count)();
            let mut frequencies = [0.0f32; 10];
            for (index, frequency) in frequencies.iter_mut().enumerate() {
                if (index as u32) < band_count {
                    *frequency = (vlc.fn_equalizer_band_frequency)(index as u32);
                }
            }
            frequencies
        }
    }

    /// Port of `PlaybackService.ApplyEqualizer`.
    ///
    /// A named preset selects one of VLC's built-in curves; explicit bands
    /// override per-band gains. `None`/`None` attaches a neutral equalizer so
    /// later curve changes update coefficients without rebuilding the audio
    /// pipeline. The settings are remembered for future players.
    pub fn apply_equalizer(
        &mut self,
        preset_name: Option<&str>,
        bands: Option<[f32; 10]>,
        preamp: f32,
    ) {
        self.equalizer_preset = preset_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        let canonical_bands = preset_name.and_then(super::equalizer::canonical_preset_by_name);
        self.equalizer_bands = bands.or(canonical_bands);
        if self.equalizer_preset.is_some() {
            self.equalizer_preamp = if preamp.is_finite() {
                preamp.clamp(-EQUALIZER_PREAMP_LIMIT, EQUALIZER_PREAMP_LIMIT)
            } else {
                NEUTRAL_EQUALIZER_PREAMP
            };
        } else if preamp.is_finite() {
            self.equalizer_preamp = preamp.clamp(-EQUALIZER_PREAMP_LIMIT, EQUALIZER_PREAMP_LIMIT);
        } else if bands.is_none() {
            self.equalizer_preamp = NEUTRAL_EQUALIZER_PREAMP;
        }

        let Some(ref vlc) = self.vlc else { return };
        let preset = self.equalizer_preset.clone();
        let preamp = self.equalizer_preamp;
        if let Some(ref active) = vlc.active {
            Self::apply_equalizer_to_player(vlc, &preset, &self.equalizer_bands, preamp, active.player);
        }
        if let Some(ref spare) = vlc.spare {
            Self::apply_equalizer_to_player(vlc, &preset, &self.equalizer_bands, preamp, spare.player);
        }
    }

    /// Sets the equalizer preamp in real time (-20 dB to +20 dB).
    pub fn set_equalizer_preamp(&mut self, preamp: f32) {
        if preamp.is_finite() {
            self.equalizer_preamp = preamp.clamp(-EQUALIZER_PREAMP_LIMIT, EQUALIZER_PREAMP_LIMIT);
        } else {
            self.equalizer_preamp = NEUTRAL_EQUALIZER_PREAMP;
        }
        let Some(ref vlc) = self.vlc else { return };
        let preset = self.equalizer_preset.clone();
        let preamp = self.equalizer_preamp;
        if let Some(ref active) = vlc.active {
            Self::apply_equalizer_to_player(vlc, &preset, &self.equalizer_bands, preamp, active.player);
        }
        if let Some(ref spare) = vlc.spare {
            Self::apply_equalizer_to_player(vlc, &preset, &self.equalizer_bands, preamp, spare.player);
        }
    }

    /// Sets an individual equalizer band gain in real time (-20 dB to +20 dB).
    pub fn set_equalizer_band(&mut self, band_index: usize, gain_db: f32) {
        if band_index >= 10 {
            return;
        }
        let mut bands = self.equalizer_bands.unwrap_or([0.0; 10]);
        bands[band_index] = gain_db.clamp(-EQUALIZER_PREAMP_LIMIT, EQUALIZER_PREAMP_LIMIT);
        self.equalizer_bands = Some(bands);

        let Some(ref vlc) = self.vlc else { return };
        let preset = self.equalizer_preset.clone();
        let preamp = self.equalizer_preamp;
        if let Some(ref active) = vlc.active {
            Self::apply_equalizer_to_player(vlc, &preset, &self.equalizer_bands, preamp, active.player);
        }
        if let Some(ref spare) = vlc.spare {
            Self::apply_equalizer_to_player(vlc, &preset, &self.equalizer_bands, preamp, spare.player);
        }
    }

    /// Port of `ApplyEqualizerCore`: always attach an equalizer, even a
    /// neutral one, so the filter exists from stream creation.
    fn apply_equalizer_to_player(
        vlc: &VlcContext,
        preset: &Option<String>,
        bands: &Option<[f32; 10]>,
        preamp: f32,
        player: *mut c_void,
    ) {
        if player.is_null() {
            return;
        }
        unsafe {
            let equalizer = match preset {
                Some(preset_name) => {
                    let count = (vlc.fn_equalizer_preset_count)();
                    if count == 0 {
                        (vlc.fn_equalizer_new)()
                    } else {
                        let index = Self::preset_index(vlc, preset_name).min(count.saturating_sub(1));
                        (vlc.fn_equalizer_new_from_preset)(index)
                    }
                }
                None => (vlc.fn_equalizer_new)(),
            };
            if equalizer.is_null() {
                return;
            }
            if let Some(bands) = bands {
                let band_count = (vlc.fn_equalizer_band_count)();
                for (index, gain) in bands.iter().enumerate() {
                    if (index as u32) < band_count {
                        (vlc.fn_equalizer_set_amp)(
                            equalizer,
                            gain.clamp(-EQUALIZER_PREAMP_LIMIT, EQUALIZER_PREAMP_LIMIT),
                            index as u32,
                        );
                    }
                }
            }
            (vlc.fn_equalizer_set_preamp)(
                equalizer,
                preamp.clamp(-EQUALIZER_PREAMP_LIMIT, EQUALIZER_PREAMP_LIMIT),
            );
            (vlc.fn_player_set_equalizer)(player, equalizer);
            (vlc.fn_equalizer_release)(equalizer);
        }
    }

    /// Port of `PresetIndex`: case-insensitive match, falling back to 0.
    fn preset_index(vlc: &VlcContext, preset_name: &str) -> u32 {
        let count = unsafe { (vlc.fn_equalizer_preset_count)() };
        for index in 0..count {
            let name_ptr = unsafe { (vlc.fn_equalizer_preset_name)(index) };
            if name_ptr.is_null() {
                continue;
            }
            let name = unsafe { CStr::from_ptr(name_ptr).to_string_lossy() };
            if name.eq_ignore_ascii_case(preset_name) {
                return index;
            }
        }
        0
    }

    // ------------------------------------------------------------------
    // Crossfade
    // ------------------------------------------------------------------

    /// Ensures freshly opened DirectSound streams pick up the configured
    /// playback volume once VLC creates the audio output after `play()`.
    pub fn reconcile_start_volume(&mut self) {
        let Some((generation, started_at)) = self.reconcile_start else {
            return;
        };
        if self.crossfade_active || generation != self.media_generation || started_at.elapsed() > Duration::from_secs(3) {
            self.reconcile_start = None;
            return;
        }
        let Some(ref vlc) = self.vlc else {
            self.reconcile_start = None;
            return;
        };
        let Some(ref active) = vlc.active else {
            self.reconcile_start = None;
            return;
        };
        let desired = self.volume as i32;
        unsafe {
            let playing = (vlc.fn_player_is_playing)(active.player) != 0;
            if !playing {
                return;
            }
            let current = (vlc.fn_audio_get_volume)(active.player);
            if current != desired {
                let _ = (vlc.fn_audio_set_volume)(active.player, desired);
            }
            let time = (vlc.fn_player_get_time)(active.player);
            if started_at.elapsed() >= Duration::from_millis(120) && time > 0 && (vlc.fn_audio_get_volume)(active.player) == desired {
                self.reconcile_start = None;
            }
        }
    }

    /// Initializes a non-blocking crossfade into `next_path` and returns
    /// `CrossfadeOutcome::InProgress` immediately. Drive it forward every
    /// ~20 ms via `poll_crossfade()`.
    pub fn start_crossfade(&mut self, next_path: &str, fade_ms: u64) -> CrossfadeOutcome {
        if fade_ms == 0 || self.video_track || self.is_video_path(next_path) {
            return CrossfadeOutcome::Rejected;
        }
        if self.vlc.is_none() || self.current_path.is_none() {
            return CrossfadeOutcome::Rejected;
        }

        self.cancel_crossfade_core();

        let remaining = self.remaining_playback_ms(fade_ms);
        let fade_window = fade_ms.min(remaining);
        if fade_window == 0 {
            return CrossfadeOutcome::Rejected;
        }

        let eq_preset = self.equalizer_preset.clone();
        let eq_bands = self.equalizer_bands;
        let eq_preamp = self.equalizer_preamp;
        {
            let Some(vlc) = self.vlc.as_mut() else {
                return CrossfadeOutcome::Rejected;
            };
            unsafe {
                vlc.spare = None;
                let media = match Self::create_media(vlc, next_path, false) {
                    Ok(media) => media,
                    Err(error) => return CrossfadeOutcome::Failed(error),
                };
                let handle = match Self::create_player_handle(vlc, media) {
                    Some(handle) => handle,
                    None => {
                        (vlc.fn_media_release)(media);
                        return CrossfadeOutcome::Failed(
                            "Failed to create LibVLC media player".into(),
                        );
                    }
                };
                self.media_generation += 1;
                handle.flags.generation.store(self.media_generation, Ordering::SeqCst);
                (vlc.fn_player_stop)(handle.player);
                Self::apply_audio_output(vlc, handle.player, &self.audio_output_device, false);
                let _ = (vlc.fn_audio_set_volume)(handle.player, 0);
                (vlc.fn_player_set_rate)(handle.player, self.playback_rate);
                Self::apply_equalizer_to_player(vlc, &eq_preset, &eq_bands, eq_preamp, handle.player);
                if (vlc.fn_player_play)(handle.player) != 0 {
                    drop(handle);
                    self.crossfade_active = false;
                    return CrossfadeOutcome::Failed(format!(
                        "LibVLC refused to play '{}'.",
                        next_path
                    ));
                }
                vlc.spare = Some(handle);
            }
        }
        self.crossfade_active = true;
        self.crossfade_progress = 0.0;
        self.reconcile_start = None;
        self.crossfade_machine = Some(CrossfadeMachine {
            next_path: next_path.to_string(),
            requested_fade_ms: fade_ms,
            fade_ms: fade_window,
            started_generation: self.media_generation,
            stage: CrossfadeStage::Opening {
                started_at: Instant::now(),
            },
        });
        CrossfadeOutcome::InProgress
    }

    /// Advances an in-progress non-blocking crossfade by one tick. Returns
    /// `None` while still progressing, or the terminal `CrossfadeOutcome` when done.
    pub fn poll_crossfade(&mut self) -> Option<CrossfadeOutcome> {
        let mut machine = self.crossfade_machine.take()?;
        if !self.crossfade_active {
            return Some(CrossfadeOutcome::Canceled);
        }

        match machine.stage {
            CrossfadeStage::Opening { started_at } => {
                if let Some(ref vlc) = self.vlc {
                    if let Some(ref spare) = vlc.spare {
                        unsafe {
                            let _ = (vlc.fn_audio_set_volume)(spare.player, 0);
                        }
                    }
                }
                let (playing, time, failed) = self.spare_playback_snapshot();
                if playing && time > 0 {
                    let remaining = self.remaining_playback_ms(machine.requested_fade_ms);
                    if remaining > machine.requested_fade_ms + 30 {
                        if let Some(vlc) = self.vlc.as_ref() {
                            if let Some(ref spare) = vlc.spare {
                                unsafe {
                                    (vlc.fn_player_set_pause)(spare.player, 1);
                                    (vlc.fn_player_set_time)(spare.player, 0);
                                    let _ = (vlc.fn_audio_set_volume)(spare.player, 0);
                                }
                            } else {
                                return Some(CrossfadeOutcome::Canceled);
                            }
                        } else {
                            return Some(CrossfadeOutcome::Canceled);
                        }
                        machine.stage = CrossfadeStage::Holding;
                    } else {
                        machine.fade_ms = if remaining > 0 {
                            machine.requested_fade_ms.min(remaining)
                        } else {
                            machine.requested_fade_ms.min(200)
                        };
                        machine.stage = CrossfadeStage::Ramping {
                            ramp_start: Instant::now(),
                        };
                    }
                    self.crossfade_machine = Some(machine);
                    None
                } else if failed || started_at.elapsed() > Duration::from_secs(5) {
                    self.cancel_crossfade_core();
                    Some(CrossfadeOutcome::Failed(
                        "The incoming crossfade track did not become ready.".into(),
                    ))
                } else {
                    self.crossfade_machine = Some(machine);
                    None
                }
            }
            CrossfadeStage::Holding => {
                if let Some(ref vlc) = self.vlc {
                    if let Some(ref spare) = vlc.spare {
                        unsafe {
                            let _ = (vlc.fn_audio_set_volume)(spare.player, 0);
                        }
                    }
                }
                let remaining = self.remaining_playback_ms(machine.requested_fade_ms);
                if remaining <= machine.requested_fade_ms {
                    if let Some(vlc) = self.vlc.as_ref() {
                        if let Some(ref spare) = vlc.spare {
                            unsafe {
                                let _ = (vlc.fn_audio_set_volume)(spare.player, 0);
                                (vlc.fn_player_set_pause)(spare.player, 0);
                            }
                        } else {
                            return Some(CrossfadeOutcome::Canceled);
                        }
                    } else {
                        return Some(CrossfadeOutcome::Canceled);
                    }
                    machine.stage = CrossfadeStage::Resuming {
                        resumed_at: Instant::now(),
                    };
                }
                self.crossfade_machine = Some(machine);
                None
            }
            CrossfadeStage::Resuming { resumed_at } => {
                if let Some(ref vlc) = self.vlc {
                    if let Some(ref spare) = vlc.spare {
                        unsafe {
                            let _ = (vlc.fn_audio_set_volume)(spare.player, 0);
                        }
                    }
                }
                let (playing, time, failed) = self.spare_playback_snapshot();
                if playing && time > 0 {
                    let remaining = self.remaining_playback_ms(machine.requested_fade_ms);
                    machine.fade_ms = if remaining > 0 {
                        machine.requested_fade_ms.min(remaining)
                    } else {
                        machine.requested_fade_ms.min(200)
                    };
                    machine.stage = CrossfadeStage::Ramping {
                        ramp_start: Instant::now(),
                    };
                    self.crossfade_machine = Some(machine);
                    None
                } else if failed || resumed_at.elapsed() > Duration::from_secs(3) {
                    self.cancel_crossfade_core();
                    Some(CrossfadeOutcome::Failed(
                        "The prepared crossfade track did not resume.".into(),
                    ))
                } else {
                    self.crossfade_machine = Some(machine);
                    None
                }
            }
            CrossfadeStage::Ramping { ramp_start } => {
                let elapsed = ramp_start.elapsed().as_secs_f64() * 1000.0;
                self.crossfade_progress = if machine.fade_ms > 0 {
                    (elapsed / machine.fade_ms as f64).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                self.apply_crossfade_volume_core();
                if self.crossfade_progress < 1.0 {
                    self.crossfade_machine = Some(machine);
                    return None;
                }

                let Some(ref mut vlc) = self.vlc else {
                    return Some(CrossfadeOutcome::Canceled);
                };
                let Some(spare) = vlc.spare.take() else {
                    return Some(CrossfadeOutcome::Canceled);
                };
                let Some(active) = vlc.active.take() else {
                    vlc.spare = Some(spare);
                    return Some(CrossfadeOutcome::Canceled);
                };
                unsafe {
                    (vlc.fn_player_stop)(active.player);
                }
                drop(active);
                unsafe {
                    let _ = (vlc.fn_audio_set_volume)(spare.player, self.volume as i32);
                }
                spare.flags.end_reached.store(false, Ordering::SeqCst);
                vlc.active = Some(spare);

                self.current_path = Some(machine.next_path);
                self.crossfade_committed_generation = machine.started_generation as i64;
                self.crossfade_active = false;
                self.crossfade_progress = 0.0;
                self.status = PlaybackStatus::Playing;
                self.reconcile_start = Some((machine.started_generation, Instant::now()));
                Some(CrossfadeOutcome::Committed)
            }
        }
    }

    /// Blocking convenience wrapper around `start_crossfade` + `poll_crossfade`
    /// preserved for headless callers and synchronous tests.
    pub fn begin_crossfade(&mut self, next_path: &str, fade_ms: u64) -> CrossfadeOutcome {
        match self.start_crossfade(next_path, fade_ms) {
            CrossfadeOutcome::InProgress => loop {
                std::thread::sleep(Duration::from_millis(20));
                if let Some(outcome) = self.poll_crossfade() {
                    return outcome;
                }
            },
            other => other,
        }
    }

    /// Polls the spare player without touching Rust-owned state transitions:
    /// (is_playing, time_ms, entered terminal error/ended state).
    fn spare_playback_snapshot(&self) -> (bool, i64, bool) {
        let Some(ref vlc) = self.vlc else {
            return (false, 0, true);
        };
        let Some(ref spare) = vlc.spare else {
            return (false, 0, true);
        };
        unsafe {
            let playing = (vlc.fn_player_is_playing)(spare.player) != 0;
            let time = (vlc.fn_player_get_time)(spare.player);
            let state = (vlc.fn_player_get_state)(spare.player);
            (
                playing,
                time,
                state == vlc_state::ERROR || state == vlc_state::ENDED,
            )
        }
    }

    fn remaining_playback_ms(&self, fallback_ms: u64) -> u64 {
        let Some(ref vlc) = self.vlc else {
            return fallback_ms;
        };
        let Some(ref active) = vlc.active else {
            return fallback_ms;
        };
        unsafe {
            // LibVLC can reset Time to zero after the outgoing stream ends;
            // treat Ended/Error as an empty tail so a prepared incoming player
            // is never held paused while the tail suddenly looks full again.
            let state = (vlc.fn_player_get_state)(active.player);
            if state == vlc_state::ENDED || state == vlc_state::ERROR {
                return 0;
            }
            let length = (vlc.fn_player_get_length)(active.player);
            if length > 0 {
                let time = (vlc.fn_player_get_time)(active.player).max(0);
                return (length - time).max(0) as u64;
            }
        }
        fallback_ms
    }

    /// Port of `CancelCrossfadeCore`: stops the spare, restores the active
    /// volume, rebinds the active flags to the current generation, and
    /// re-queues a synthetic end event when the outgoing player had ended.
    pub fn cancel_crossfade(&mut self) {
        self.cancel_crossfade_core();
    }

    fn cancel_crossfade_core(&mut self) {
        let fade_was_active = self.crossfade_active;
        self.crossfade_active = false;
        self.crossfade_progress = 0.0;
        self.crossfade_machine = None;

        let Some(ref mut vlc) = self.vlc else { return };
        let generation = self.media_generation;
        let volume = self.volume;

        unsafe {
            if let Some(spare) = vlc.spare.take() {
                if !spare.player.is_null() {
                    (vlc.fn_player_stop)(spare.player);
                    let _ = (vlc.fn_audio_set_volume)(spare.player, 0);
                }
                drop(spare);
            }
            // A canceled/failed fade keeps the outgoing media alive. Its
            // eventual end/error belongs to the current generation again.
            if fade_was_active {
                if let Some(ref active) = vlc.active {
                    active
                        .flags
                        .generation
                        .store(generation, Ordering::SeqCst);
                    if Self::player_state_is(vlc, active.player, vlc_state::ENDED) {
                        // Its native end may have arrived while the fade
                        // suppressed it.
                        active.flags.end_reached.store(true, Ordering::SeqCst);
                    }
                }
            }
            if let Some(ref active) = vlc.active {
                let _ = (vlc.fn_audio_set_volume)(active.player, volume as i32);
            }
        }
    }

    fn player_state_is(vlc: &VlcContext, player: *mut c_void, expected: i32) -> bool {
        if player.is_null() {
            return false;
        }
        unsafe { (vlc.fn_player_get_state)(player) == expected }
    }

    fn apply_crossfade_volume_core(&mut self) {
        let Some(ref vlc) = self.vlc else { return };
        let progress = if self.crossfade_active {
            self.crossfade_progress
        } else {
            0.0
        };
        let (outgoing, incoming) = crossfade_volumes(self.volume, progress);
        unsafe {
            if let Some(ref active) = vlc.active {
                let _ = (vlc.fn_audio_set_volume)(active.player, outgoing as i32);
            }
            if let Some(ref spare) = vlc.spare {
                let _ = (vlc.fn_audio_set_volume)(spare.player, incoming as i32);
            }
        }
    }

    #[cfg(windows)]
    fn get_short_path(path: &str) -> Option<String> {
        use std::ffi::{OsStr, OsString};
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        #[link(name = "kernel32")]
        extern "system" {
            fn GetShortPathNameW(
                lpszLongPath: *const u16,
                lpszShortPath: *mut u16,
                cchBuffer: u32,
            ) -> u32;
        }

        let mut wide: Vec<u16> = OsStr::new(path).encode_wide().collect();
        wide.push(0);

        let mut buf = vec![0u16; 1024];
        let len = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if len > 0 && (len as usize) < buf.len() {
            buf.truncate(len as usize);
            Some(OsString::from_wide(&buf).to_string_lossy().to_string())
        } else {
            None
        }
    }

    #[cfg(windows)]
    fn send_mci(command: &str) -> Result<String, String> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "winmm")]
        extern "system" {
            fn mciSendStringW(
                lpstr_command: *const u16,
                lpstr_return_string: *mut u16,
                u_return_length: u32,
                hwnd_callback: usize,
            ) -> u32;
        }

        let mut wide: Vec<u16> = OsStr::new(command).encode_wide().collect();
        wide.push(0);

        let mut return_buf = [0u16; 256];
        let err = unsafe {
            mciSendStringW(
                wide.as_ptr(),
                return_buf.as_mut_ptr(),
                return_buf.len() as u32,
                0,
            )
        };

        if err != 0 {
            return Err(format!("MCI error code: {}", err));
        }

        let len = return_buf.iter().position(|&c| c == 0).unwrap_or(return_buf.len());
        let res = String::from_utf16_lossy(&return_buf[..len]);
        Ok(res)
    }
}
