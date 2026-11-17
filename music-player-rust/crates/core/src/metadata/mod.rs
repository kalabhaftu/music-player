pub mod artwork;
pub mod dff;
pub mod palette;
pub mod tag_editor;
pub mod tag_reader;

use std::path::PathBuf;

pub use artwork::{
    artwork_cache_dir, hash_artwork, prune_unreferenced_artwork, resolve_cached_artwork,
    save_artwork_to_cache, save_artwork_to_cache_in,
};
pub use dff::{dff_id3_payload, read_dff_duration_ms};
pub use palette::{extract_palette, ArtworkPalette, RgbColor};
pub use tag_editor::TagEditor;
pub use tag_reader::{
    custom_field_format, inspect_file, read_additional_standard_fields, read_custom_fields,
    read_tags, read_tags_in_cache, set_artwork_extraction_enabled, ExtractedTags, TrackDetails,
    ADDITIONAL_STANDARD_FIELDS,
};

/// `%LOCALAPPDATA%\MusicPlayer` — the single app-data root (AppDataPaths.Root).
pub fn app_data_root() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("MusicPlayer"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Tag backup folder: `%LOCALAPPDATA%\MusicPlayer\TagBackups`.
pub fn tag_backups_dir() -> PathBuf {
    let dir = app_data_root().join("TagBackups");
    std::fs::create_dir_all(&dir).ok();
    dir
}
