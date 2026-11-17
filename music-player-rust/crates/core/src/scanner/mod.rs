pub mod coordinator;
pub mod exclusions;
pub mod indexer;
pub mod reconciler;
pub mod scan;
pub mod session;
pub mod walker;
pub mod watcher;
pub mod watch_source;

pub use coordinator::{LibraryScanCoordinator, ScanStatus};
pub use exclusions::{is_excluded_directory, is_supported_audio, AUDIO_EXTENSIONS};
pub use indexer::{IndexResult, LibraryIndexer};
pub use reconciler::{AvailabilityReconciliation, TrackAvailabilityService};
pub use scan::{
    is_default_excluded_directory, is_default_ignored_directory, is_under_default_excluded_directory,
    normalize_path, scan_paths, paths_equal, is_within_root, ScanExcludedPathKind,
};
pub use session::LibraryScanSession;
pub use walker::{scan_folder, ScannerControl};
pub use watcher::{DebouncedWatcherState, DrainResult, LibraryWatchBatch};
pub use watch_source::LibraryFileWatcher;
