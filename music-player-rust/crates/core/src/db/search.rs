use super::tracks::row_to_track;
use crate::models::Track;
use rusqlite::{params, Connection, Result};

pub fn search_tracks(conn: &Connection, query: &str, limit: u32) -> Result<Vec<Track>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    // Split search terms and create FTS prefix match: "word"*
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }

    let fts_query = words
        .iter()
        .map(|w| {
            let clean: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
            format!("\"{}\"*", clean)
        })
        .collect::<Vec<_>>()
        .join(" AND ");

    if fts_query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT t.*
        FROM tracks t
        JOIN tracks_fts f ON t.rowid = f.rowid
        WHERE tracks_fts MATCH ?1
        ORDER BY rank
        LIMIT ?2
        "#,
    )?;

    let mut rows = stmt.query(params![fts_query, limit as i64])?;
    let mut tracks = Vec::new();
    while let Some(row) = rows.next()? {
        tracks.push(row_to_track(row)?);
    }

    // Fallback LIKE search if FTS returned nothing (e.g. for punctuation or short tokens)
    if tracks.is_empty() {
        let pattern = format!("%{}%", trimmed);
        let mut fallback_stmt = conn.prepare(
            r#"
            SELECT * FROM tracks
            WHERE title LIKE ?1 OR artist LIKE ?1 OR album LIKE ?1
            ORDER BY title COLLATE NOCASE
            LIMIT ?2
            "#,
        )?;
        let mut fallback_rows = fallback_stmt.query(params![pattern, limit as i64])?;
        while let Some(row) = fallback_rows.next()? {
            tracks.push(row_to_track(row)?);
        }
    }

    Ok(tracks)
}
