use super::tracks::row_to_track;
use crate::models::Track;
use rusqlite::{params, Connection, Result};

/// Builds the FTS5 MATCH expression for a free-text query: whitespace-split
/// words, alphanumeric-filtered, quoted-prefix form joined with AND. Empty
/// when the query carries no searchable words.
pub fn fts_query_string(query: &str) -> String {
    query
        .split_whitespace()
        .map(|w| {
            let clean: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
            format!("\"{}\"*", clean)
        })
        .filter(|w| w != "\"\"*")
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Appends case-insensitive per-word metadata predicates using
/// `musicplayer_contains` (full Unicode case folding across title, artist,
/// album, album_artist, genre, and path).
pub fn push_unicode_search_clauses(
    query: &str,
    alias: &str,
    clauses: &mut Vec<String>,
    params: &mut Vec<String>,
) {
    for term in query.split_whitespace().map(str::trim).filter(|t| !t.is_empty()) {
        let col_prefix = if alias.is_empty() {
            String::new()
        } else {
            format!("{alias}.")
        };
        clauses.push(format!(
            "(musicplayer_contains({col_prefix}title, ?) OR musicplayer_contains({col_prefix}artist, ?) OR musicplayer_contains({col_prefix}album, ?) OR musicplayer_contains({col_prefix}album_artist, ?) OR musicplayer_contains({col_prefix}genre, ?) OR musicplayer_contains({col_prefix}path, ?))"
        ));
        let lower = term.to_lowercase();
        for _ in 0..6 {
            params.push(lower.clone());
        }
    }
}

pub fn search_tracks(conn: &Connection, query: &str, limit: u32) -> Result<Vec<Track>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut tracks = Vec::new();
    let fts_query = fts_query_string(trimmed);
    if !fts_query.trim().is_empty() {
        if let Ok(mut stmt) = conn.prepare(
            r#"
            SELECT t.*
            FROM tracks t
            JOIN tracks_fts f ON t.rowid = f.rowid
            WHERE tracks_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
            "#,
        ) {
            if let Ok(mut rows) = stmt.query(params![fts_query, limit as i64]) {
                while let Some(row) = rows.next()? {
                    tracks.push(row_to_track(row)?);
                }
            }
        }
    }

    // Fallback case-insensitive search when FTS yields nothing (short tokens,
    // punctuation, or a C#-created case-sensitive trigram FTS index).
    if tracks.is_empty() {
        let mut clauses = Vec::new();
        let mut search_params = Vec::new();
        push_unicode_search_clauses(trimmed, "", &mut clauses, &mut search_params);
        if !clauses.is_empty() {
            let sql = format!(
                "SELECT * FROM tracks WHERE {} ORDER BY title COLLATE NOCASE LIMIT ?",
                clauses.join(" AND ")
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut bound: Vec<Box<dyn rusqlite::ToSql>> = search_params
                .into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>)
                .collect();
            bound.push(Box::new(limit as i64));
            let mut rows = stmt.query(rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())))?;
            while let Some(row) = rows.next()? {
                tracks.push(row_to_track(row)?);
            }
        }
    }

    Ok(tracks)
}
