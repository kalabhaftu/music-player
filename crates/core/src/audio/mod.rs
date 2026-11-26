pub mod coordinator;
pub mod engine;
pub mod equalizer;
pub mod listening;
pub mod queue;
pub mod volume;

pub use coordinator::{
    Notice, NoticeSeverity, PlaybackCommandKind, PlaybackCommandResult, PlaybackCoordinator,
    PlaybackCrossfadeRequest,
};
pub use engine::{
    crossfade_volumes, vlc_event, vlc_state, AudioEngine, CrossfadeOutcome, EventFlags,
    PlaybackStatus, PreparedVlc, VideoSubtitleTrack,
};
pub use equalizer::{
    canonical_preset_by_name, canonical_preset_names, EqualizerState, CANONICAL_PRESETS,
    EQUALIZER_BANDS, EQUALIZER_BAND_FREQUENCIES_HZ, EQUALIZER_BAND_LABELS, EQUALIZER_PREAMP_LIMIT,
    EQUALIZER_PREAMP_MAX, EQUALIZER_PREAMP_MIN, NEUTRAL_EQUALIZER_PREAMP,
};
pub use listening::{PlayCompletion, PlaybackEventPolicy, PlaybackListeningTracker};
pub use queue::{
    build_external_open_queue, next_index, previous_index, shuffle_upcoming, QueueCoordinator,
    RepeatMode,
};
pub use volume::{
    amplitude_to_perceived_loudness, linear_to_perceived_volume, perceived_loudness_to_amplitude,
    perceived_to_linear_volume, quadratic_volume_curve,
};
