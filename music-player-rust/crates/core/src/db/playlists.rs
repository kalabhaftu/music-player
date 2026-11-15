use crate::models::{Playlist, PlaylistEntry, PlaylistSummary};
use super::tracks::row_to_track;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result};

pub fn create_playlist(conn: &Connection, id: &str, name: &str) -> Result<Playlist> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    conn.execute(
        "INSERT INTO playlists (id, name, created_utc) VALUES (?1, ?2, ?3)",
        params![id, name, now_str],
    )?;

    Ok(Playlist {
        id: id.to_string(),
        name: name.to_string(),
        paths: Vec::new(),
        created_utc: now,
    })
}

pub fn get_playlists(conn: &Connection) -> Result<Vec<PlaylistSummary>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT p.id, p.name, p.created_utc, COUNT(pt.track_path) as cnt
        FROM playlists p
        LEFT JOIN playlist_tracks pt ON p.id = pt.playlist_id
        GROUP BY p.id
        ORDER BY p.name COLLATE NOCASE ASC
        "#,
    )?;
    let mut rows = stmt.query([])?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next()? {
        let created_str: String = row.get(2)?;
        let created_utc = DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        summaries.push(PlaylistSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            created_utc,
            track_count: row.get::<_, i64>(3)? as u32,
        });
    }
    Ok(summaries)
}

pub fn get_playlist(conn: &Connection, playlist_id: &str) -> Result<Option<Playlist>> {
    let mut stmt = conn.prepare("SELECT id, name, created_utc FROM playlists WHERE id = ?1")?;
    let mut rows = stmt.query(params![playlist_id])?;
    if let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let created_str: String = row.get(2)?;
        let created_utc = DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let mut tracks_stmt = conn.prepare(
            "SELECT track_path FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position ASC",
        )?;
        let mut track_rows = tracks_stmt.query(params![playlist_id])?;
        let mut paths = Vec::new();
        while let Some(tr) = track_rows.next()? {
            paths.push(tr.get(0)?);
        }

        Ok(Some(Playlist {
            id,
            name,
            paths,
            created_utc,
        }))
    } else {
        Ok(None)
    }
}

pub fn get_playlist_entries(conn: &Connection, playlist_id: &str) -> Result<Vec<PlaylistEntry>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT pt.position, pt.track_path, t.path, t.title, t.artist, t.album, t.album_artist,
               t.genre, t.year, t.track_number, t.duration_ms, t.file_size, t.modified_utc,
               t.added_utc, t.favorite, t.rating, t.play_count, t.last_played_utc, t.artwork_path, t.has_lyrics
        FROM playlist_tracks pt
        LEFT JOIN tracks t ON pt.track_path = t.path
        WHERE pt.playlist_id = ?1
        ORDER BY pt.position ASC
        "#,
    )?;

    let mut rows = stmt.query(params![playlist_id])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        let position: u32 = row.get::<_, i64>(0)? as u32;
        let track_path: String = row.get(1)?;
        let track = if row.get::<_, Option<String>>(2)?.is_some() {
            Some(row_to_track(row)?)
        } else {
            None
        };

        entries.push(PlaylistEntry {
            position,
            path: track_path,
            track,
        });
    }
    Ok(entries)
}

pub fn add_tracks_to_playlist(conn: &Connection, playlist_id: &str, paths: &[String]) -> Result<()> {
    let max_pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
        |r| r.get(0),
    )?;

    let mut next_pos = max_pos + 1;
    let mut stmt = conn.prepare(
        "INSERT INTO playlist_tracks (playlist_id, position, track_path) VALUES (?1, ?2, ?3)",
    )?;

    for path in paths {
        stmt.execute(params![playlist_id, next_pos, path])?;
        next_pos += 1;
    }

    Ok(())
}

pub fn delete_playlist(conn: &Connection, playlist_id: &str) -> Result<()> {
    conn.execute("DELETE FROM playlists WHERE id = ?1", params![playlist_id])?;
    Ok(())
}
