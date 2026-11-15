use rusqlite::{Connection, Result};
use std::path::Path;

pub fn filter_available_paths(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut available = Vec::new();
    let mut unavailable = Vec::new();

    for p in paths {
        if Path::new(p).exists() {
            available.push(p.clone());
        } else {
            unavailable.push(p.clone());
        }
    }

    (available, unavailable)
}

pub fn purge_deleted_tracks(conn: &Connection, paths: &[String]) -> Result<usize> {
    if paths.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for chunk in paths.chunks(200) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM tracks WHERE path IN ({})", placeholders);
        let mut stmt = conn.prepare(&sql)?;
        count += stmt.execute(rusqlite::params_from_iter(chunk.iter()))?;
    }

    Ok(count)
}

pub fn purge_tracks_under_directory(conn: &Connection, dir_prefix: &str) -> Result<usize> {
    let pattern = format!("{}%", dir_prefix.trim_end_matches(['/', '\\']));
    conn.execute(
        "DELETE FROM tracks WHERE path LIKE ?1",
        rusqlite::params![pattern],
    )
}
