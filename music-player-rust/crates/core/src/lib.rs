pub mod audio;
pub mod db;
pub mod duplicates;
pub mod lyrics;
pub mod metadata;
pub mod models;
pub mod scanner;

pub use audio::{AudioEngine, EqualizerState, PlaybackStatus, QueueCoordinator, RepeatMode, EQUALIZER_BANDS};
pub use db::LibraryStore;
pub use duplicates::{DuplicateDetector, ExactDuplicateGroup, ExactDuplicateGroupPage};
pub use lyrics::{format_lrc, has_sidecar, parse_lrc, plain_text, read_sidecar, write_sidecar};
pub use metadata::{artwork_cache_dir, extract_palette, hash_artwork, read_tags, resolve_cached_artwork, save_artwork_to_cache, ArtworkPalette, ExtractedTags, RgbColor};
pub use models::{
    LibraryGroup, LibraryStats, LyricsDocument, LyricsLine, PlaybackSession, Playlist,
    PlaylistEntry, PlaylistSummary, ScanProgress, TagBackup, TagEdit, Track, TrackSort,
};
pub use scanner::{is_excluded_directory, is_supported_audio, scan_folder, IndexResult, LibraryIndexer, ScannerControl, AUDIO_EXTENSIONS};
