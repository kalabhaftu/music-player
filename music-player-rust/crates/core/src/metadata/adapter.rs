use std::path::Path;
use super::tag_reader::{ExtractedTags, TrackDetails};

/// Common interface for metadata extraction engines.
/// Isolates metadata parsing libraries (like Lofty) behind a clean boundary.
pub trait MetadataAdapter: Send + Sync {
    /// Reads normalized track tags, duration, and extracts embedded artwork if a cache directory is supplied.
    fn read_metadata(&self, path: &Path, artwork_cache: Option<&Path>) -> Result<ExtractedTags, String>;

    /// Inspects audio container and technical audio stream properties.
    fn inspect_file(&self, path: &Path) -> Result<TrackDetails, String>;
}
