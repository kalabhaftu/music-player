pub mod artwork;
pub mod palette;
pub mod tag_editor;
pub mod tag_reader;

pub use artwork::{artwork_cache_dir, hash_artwork, resolve_cached_artwork, save_artwork_to_cache};
pub use palette::{extract_palette, ArtworkPalette, RgbColor};
pub use tag_editor::TagEditor;
pub use tag_reader::{read_tags, ExtractedTags};
