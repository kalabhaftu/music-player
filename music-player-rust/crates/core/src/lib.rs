pub mod audio;
pub mod db;
pub mod duplicates;
pub mod lyrics;
pub mod lock;
pub mod log;
pub mod metadata;
pub mod models;
pub mod playlists;
pub mod scanner;
pub mod settings;

pub use audio::{
    canonical_preset_by_name, canonical_preset_names, quadratic_volume_curve, AudioEngine,
    EqualizerState, PlaybackStatus, QueueCoordinator, RepeatMode, CANONICAL_PRESETS,
    EQUALIZER_BANDS, EQUALIZER_BAND_FREQUENCIES_HZ, EQUALIZER_BAND_LABELS, EQUALIZER_PREAMP_MAX,
    EQUALIZER_PREAMP_MIN,
};
pub use db::LibraryStore;
pub use db::tracks::TrackQuery;
pub use duplicates::{DuplicateDetector, ExactDuplicateGroup, ExactDuplicateGroupPage};
pub use lyrics::{format_lrc, has_sidecar, parse_lrc, plain_text, read_sidecar, write_sidecar};
pub use log::LocalAppLog;
pub use metadata::{
    artwork_cache_dir, clean_stale_temp_files, custom_field_format, extract_palette, hash_artwork, inspect_file, read_tags,
    read_tags_in_cache, resolve_cached_artwork, save_artwork_to_cache, ArtworkOperationGuard,
    HybridMetadataEngine, LoftyAdapter, MetadataAdapter,
    ADDITIONAL_STANDARD_FIELDS, ArtworkPalette, ExtractedTags, RgbColor, TagEditor, TrackDetails,
};
pub use models::{
    LibraryGroup, LibraryStats, LyricsDocument, LyricsLine, PlaybackSession, Playlist,
    PlaylistEntry, PlaylistSummary, ScanProgress, TagBackup, TagEdit, Track, TrackSort,
    opaque_id_for, track_id_for_path,
};
pub use playlists::{export_m3u8, import_m3u8};
pub use scanner::{
    is_default_excluded_directory, is_default_ignored_directory, is_excluded_directory,
    is_supported_audio, is_under_default_excluded_directory, scan_folder, AvailabilityReconciliation,
    DebouncedWatcherState, IndexResult, LibraryFileWatcher, LibraryIndexer, LibraryScanCoordinator,
    LibraryScanSession, LibraryWatchBatch, ScanExcludedPathKind, ScanStatus, ScannerControl,
    TrackAvailabilityService, AUDIO_EXTENSIONS,
};
pub use settings::PlayerSettings;
