use rusqlite::{Connection, functions::FunctionFlags};
use std::path::Path;

/// Registers deterministic scalar helpers used by view queries
/// (mirrors LibraryStore.Open's `musicplayer_directory` function).
pub fn register_query_functions(conn: &Connection) {
    conn.create_scalar_function(
        "musicplayer_directory",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let path: String = ctx.get(0)?;
            Ok(directory_of(&path))
        },
    )
    .ok();
    conn.create_scalar_function(
        "musicplayer_contains",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let haystack: String = ctx.get::<Option<String>>(0)?.unwrap_or_default();
            let needle: String = ctx.get::<Option<String>>(1)?.unwrap_or_default();
            if needle.is_empty() {
                return Ok(true);
            }
            Ok(haystack.to_lowercase().contains(&needle.to_lowercase()))
        },
    )
    .ok();
}

/// Trimmed parent directory of a track path ("" when none).
pub fn directory_of(path: &str) -> String {
    if path.trim().is_empty() {
        return String::new();
    }
    match Path::new(path).parent() {
        Some(parent) => {
            let s = parent.to_string_lossy().to_string();
            s.trim_end_matches(['/', '\\']).to_string()
        }
        None => String::new(),
    }
}

/// SQL fragment testing that a path is a direct child of the bound folder.
/// Used by the Folders group detail (parent-directory equality, 1:1 with
/// `musicplayer_directory(tracks.path) = $folderPrefix`).
pub const FOLDER_CHILD_PREDICATE: &str = "musicplayer_directory(t.path) = ? COLLATE NOCASE";
