use std::path::Path;
use super::adapter::MetadataAdapter;
use super::lofty_adapter::LoftyAdapter;
use super::tag_reader::{legacy_read_tags_in_cache, legacy_inspect_file, ExtractedTags, TrackDetails};

/// Primary metadata engine combining Lofty streaming reader with custom parser fallback.
pub struct HybridMetadataEngine {
    lofty: LoftyAdapter,
}

impl HybridMetadataEngine {
    pub fn new() -> Self {
        Self {
            lofty: LoftyAdapter::new(),
        }
    }

    pub fn read_metadata(&self, path: &Path, artwork_cache: Option<&Path>) -> Result<ExtractedTags, String> {
        <Self as MetadataAdapter>::read_metadata(self, path, artwork_cache)
    }

    pub fn inspect_file(&self, path: &Path) -> Result<TrackDetails, String> {
        <Self as MetadataAdapter>::inspect_file(self, path)
    }
}

impl Default for HybridMetadataEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataAdapter for HybridMetadataEngine {
    fn read_metadata(&self, path: &Path, artwork_cache: Option<&Path>) -> Result<ExtractedTags, String> {
        // Try Lofty streaming parser first (does not load full file into memory)
        match self.lofty.read_metadata(path, artwork_cache) {
            Ok(mut tags) => {
                if artwork_cache.is_some() && tags.artwork_path.is_none() {
                    let legacy = legacy_read_tags_in_cache(path, artwork_cache);
                    if legacy.artwork_path.is_some() {
                        tags.artwork_path = legacy.artwork_path;
                    }
                }
                Ok(tags)
            }
            Err(_) => {
                // Fallback to legacy parser if Lofty encountered an unhandled container variant
                let tags = legacy_read_tags_in_cache(path, artwork_cache);
                Ok(tags)
            }
        }
    }

    fn inspect_file(&self, path: &Path) -> Result<TrackDetails, String> {
        match self.lofty.inspect_file(path) {
            Ok(details) => Ok(details),
            Err(_) => {
                legacy_inspect_file(path).ok_or_else(|| format!("Could not inspect file: {}", path.display()))
            }
        }
    }
}
