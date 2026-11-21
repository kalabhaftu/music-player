mod http;
pub mod lrclib;
pub mod parser;
pub mod sidecar;

pub use crate::models::{LyricsDocument, LyricsLine};
pub use lrclib::{normalize_query, LrclibSearchClient, LyricsSearchResult};
pub use parser::{format_lrc, parse_lrc, plain_text};
pub use sidecar::{has_sidecar, read_sidecar, remove_sidecar, sidecar_path, write_sidecar};
