pub mod coordinator;
pub mod engine;
pub mod equalizer;
pub mod listening;
pub mod queue;

pub use coordinator::{
    Notice, NoticeSeverity, PlaybackCommandKind, PlaybackCommandResult, PlaybackCoordinator,
    PlaybackCrossfadeRequest,
};
pub use engine::{
    crossfade_volumes, vlc_event, vlc_state, AudioEngine, CrossfadeOutcome, EventFlags,
    PlaybackStatus,
};
pub use equalizer::{
    EqualizerState, EQUALIZER_BANDS, EQUALIZER_PREAMP_LIMIT, NEUTRAL_EQUALIZER_PREAMP,
};
pub use listening::{PlayCompletion, PlaybackEventPolicy, PlaybackListeningTracker};
pub use queue::{
    build_external_open_queue, next_index, previous_index, shuffle_upcoming, QueueCoordinator,
    RepeatMode,
};
