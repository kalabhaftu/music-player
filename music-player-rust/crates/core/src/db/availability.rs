use crate::models::PlaybackSession;
use rusqlite::{params, Connection, Result};
use std::collections::HashSet;
use std::path::Path;

/// True when a path points at an existing file.
fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Deletes index rows plus every user-facing reference (playlist entries with
/// resequencing, playback queue, saved session state) — a 1:1 port of
/// `LibraryStore.PurgeDeletedTracks`.
pub fn purge_deleted_tracks(conn: &Connection, paths: &[String]) -> Result<usize> {
    let mut normalized: Vec<String> = paths.iter().filter(|p| !p.trim().is_empty()).cloned().collect();
    if normalized.is_empty() {
        return Ok(0);
    }
    normalized.sort();
    normalized.dedup();

    let deleted: HashSet<String> = normalized.iter().map(|p| p.to_lowercase()).collect();
    let mut removed = 0usize;

    conn.execute_batch("BEGIN")?;

    // Playlists that reference purged entries (captured before deletion), then
    // delete the entries and compact surviving rows.
    let mut affected: Vec<String> = Vec::new();
    for path in &normalized {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_path = ?1 COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![path], |r| r.get::<_, String>(0))?;
        affected.extend(rows.flatten());
    }
    affected.sort();
    affected.dedup();

    for path in &normalized {
        conn.execute(
            "DELETE FROM playlist_tracks WHERE track_path = ?1 COLLATE NOCASE",
            params![path],
        )?;
        removed += conn
            .execute("DELETE FROM tracks WHERE path = ?1 COLLATE NOCASE", params![path])
            .unwrap_or(0);
        let _ = conn.execute(
            "DELETE FROM track_handles WHERE path = ?1 COLLATE NOCASE",
            params![path],
        );
        let _ = conn.execute(
            "DELETE FROM track_fingerprints WHERE path = ?1 COLLATE NOCASE",
            params![path],
        );
    }

    // Drop orphaned playlist entries and compact positions for affected playlists.
    conn.execute_batch(
        "DELETE FROM playlist_tracks WHERE track_path NOT IN (SELECT path FROM tracks)",
    )?;
    for playlist_id in &affected {
        let paths: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT track_path FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let rows = stmt.query_map(params![playlist_id], |r| r.get::<_, String>(0))?;
            rows.flatten().collect()
        };
        conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![playlist_id])?;
        let mut stmt = conn.prepare(
            "INSERT INTO playlist_tracks (playlist_id, position, track_path) VALUES (?1, ?2, ?3)",
        )?;
        for (position, path) in paths.iter().enumerate() {
            stmt.execute(params![playlist_id, position as i64, path])?;
        }
    }

    // Session + queue pruning (RemoveSessionPaths port).
    let queue: Vec<String> = {
        let mut stmt = conn.prepare("SELECT track_path FROM playback_queue ORDER BY position")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.flatten().collect()
    };
    let queue: Vec<String> = queue
        .into_iter()
        .filter(|p| !deleted.contains(&p.to_lowercase()))
        .collect();

    let session: Option<PlaybackSession> = {
        let mut stmt = conn.prepare("SELECT payload FROM playback_session WHERE id = 1")?;
        let mut rows = stmt.query([])?;
        let payload: Option<String> = rows.next()?.and_then(|r| r.get(0).ok());
        payload.and_then(|payload| serde_json::from_str::<PlaybackSession>(&payload).ok())
    };

    if let Some(session) = session {
        let old_queue = session.queue.clone();
        let current_deleted = session
            .track_path
            .as_ref()
            .map(|p| deleted.contains(&p.to_lowercase()))
            .unwrap_or(false);
        let bound = (session.queue_index + 1).clamp(0, old_queue.len() as i32) as usize;
        let kept_before = old_queue
            .iter()
            .take(bound)
            .filter(|p| !deleted.contains(&p.to_lowercase()))
            .count() as i32;
        let pruned = PlaybackSession {
            track_path: if current_deleted {
                None
            } else {
                session.track_path.clone()
            },
            position_ms: if current_deleted { 0 } else { session.position_ms },
            queue: queue.clone(),
            shuffle: session.shuffle,
            repeat_mode: session.repeat_mode.clone(),
            repeat_a_ms: if current_deleted { None } else { session.repeat_a_ms },
            repeat_b_ms: if current_deleted { None } else { session.repeat_b_ms },
            queue_index: kept_before - 1,
        };
        let payload = serde_json::to_string(&pruned).unwrap_or_default();
        conn.execute(
            "INSERT INTO playback_session (id, payload) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            params![payload],
        )?;
        conn.execute("DELETE FROM playback_queue", [])?;
        let mut stmt = conn.prepare("INSERT INTO playback_queue (position, track_path) VALUES (?1, ?2)")?;
        for (pos, path) in queue.iter().enumerate() {
            stmt.execute(params![pos as i64, path])?;
        }
    } else {
        conn.execute("DELETE FROM playback_queue", [])?;
        let mut stmt = conn.prepare("INSERT INTO playback_queue (position, track_path) VALUES (?1, ?2)")?;
        for (pos, path) in queue.iter().enumerate() {
            stmt.execute(params![pos as i64, path])?;
        }
    }

    conn.execute_batch("COMMIT")?;
    Ok(removed)
}



/// Purges every track under the supplied directory prefixes (scan exclusions
/// and removed library roots) through the full purge path.
pub fn purge_tracks_under_directories(conn: &Connection, prefixes: &[String]) -> Result<usize> {
    let mut to_purge: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT path FROM tracks")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for path in rows.flatten() {
            if prefixes.iter().any(|prefix| is_within_root(prefix, &path)) {
                to_purge.push(path);
            }
        }
    }
    if to_purge.is_empty() {
        return Ok(0);
    }
    purge_deleted_tracks(conn, &to_purge)
}

pub fn purge_tracks_under_directory(conn: &Connection, dir_prefix: &str) -> Result<usize> {
    purge_tracks_under_directories(conn, std::slice::from_ref(&dir_prefix.to_string()))
}

/// Removes rows indexed by older builds under cache, build, or hidden
/// application folders (`PurgeTracksInDefaultExcludedDirectories`).
pub fn purge_tracks_in_default_excluded_directories(conn: &Connection) -> Result<usize> {
    let mut to_purge: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT path FROM tracks")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for path in rows.flatten() {
            if crate::scanner::is_under_default_excluded_directory(&path) {
                to_purge.push(path);
            }
        }
    }
    if to_purge.is_empty() {
        return Ok(0);
    }
    purge_deleted_tracks(conn, &to_purge)
}

/// Saved references (playlists, queue, handles) whose index row no longer
/// exists (`GetReferencedPaths`).
pub fn get_referenced_paths(conn: &Connection) -> Result<Vec<String>> {
    let sql = r#"
        SELECT DISTINCT path FROM (
            SELECT track_path AS path FROM playlist_tracks
            UNION SELECT track_path AS path FROM playback_queue
            UNION SELECT path AS path FROM track_handles
        ) refs
        WHERE NOT EXISTS(SELECT 1 FROM tracks WHERE tracks.path = refs.path COLLATE NOCASE)
    "#;
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.flatten().collect())
}

/// Clears the soft-unavailable flag for tracks whose files exist on disk again,
/// returning the recovered paths (`ClearUnavailableTracksThatExist`).
pub fn clear_unavailable_tracks_that_exist(conn: &Connection) -> Result<Vec<String>> {
    let mut unavailable: Vec<String> = {
        let mut stmt = conn.prepare("SELECT path FROM tracks WHERE unavailable = 1")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.flatten().collect()
    };
    let recovered: Vec<String> = unavailable.iter().filter(|p| exists(p)).cloned().collect();
    for path in &recovered {
        conn.execute("UPDATE tracks SET unavailable = 0 WHERE path = ?1", params![path])?;
    }
    let recovered_lower: HashSet<String> = recovered.iter().map(|p| p.to_lowercase()).collect();
    unavailable.retain(|p| !recovered_lower.contains(&p.to_lowercase()));
    Ok(recovered)
}

/// Restores an available track, reporting whether the durable flag actually
/// changed (`ReinstateTrack` + `SetTrackUnavailable(path, false)` change
/// detection combined — the store keeps no separate purged-path overlay).
pub fn reinstate_track(conn: &Connection, path: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE tracks SET unavailable = 0 WHERE path = ?1 AND unavailable <> 0",
        params![path],
    )?;
    Ok(changed > 0)
}

/// Windows-style ancestor check: `path` strictly under `root` (case-insensitive
/// when the root ends without a separator, 1:1 with IsAncestorDirectory).
pub fn is_within_root(root: &str, path: &str) -> bool {
    let root_full = normalize_path(root);
    let prefix = root_full.trim_end_matches(['/', '\\']);
    if prefix.is_empty() {
        return false;
    }
    let path_lower = path.to_lowercase();
    let prefix_lower = prefix.to_lowercase();
    // A root drive ("c:\") matches everything beneath it.
    if prefix_lower.len() <= 3 && prefix_lower.ends_with(':') {
        return path_lower.starts_with(&format!("{}\\", prefix_lower));
    }
    match path_lower.strip_prefix(&prefix_lower) {
        Some(rest) => rest.starts_with('\\') || rest.starts_with('/'),
        None => false,
    }
}

pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    let mut out = trimmed.trim_end_matches(['/', '\\']).to_string();
    // Preserve drive roots like "C:\"
    if out.len() == 2 && out.as_bytes()[1] == b':' {
        out.push('\\');
    }
    out
}
