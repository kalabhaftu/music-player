use std::path::Path;
use rusqlite::{params, Connection, Result as SqlResult};
use crate::models::Track;
use crate::db::tracks::row_to_track;
use super::fingerprint::{get_file_state, hash_file};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExactDuplicateGroup {
    pub sha256: String,
    pub file_size: i64,
    pub copy_count: usize,
    pub representative: Track,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExactDuplicateGroupPage {
    pub groups: Vec<ExactDuplicateGroup>,
    pub total_count: usize,
}

pub struct DuplicateDetector;

impl DuplicateDetector {
    /// Prepares exact content fingerprints using a two-pass algorithm:
    /// 1. Finds all files with identical file sizes.
    /// 2. Only hashes files that share a size with at least one other file.
    /// 3. Saves verified fingerprints to `track_fingerprints`.
    pub fn prepare_fingerprints(conn: &Connection) -> SqlResult<usize> {
        // Create temporary table for current file states
        conn.execute_batch(
            "DROP TABLE IF EXISTS temp.current_fingerprint_state;
             CREATE TEMP TABLE current_fingerprint_state(
                 path TEXT PRIMARY KEY,
                 file_size INTEGER NULL,
                 modified_utc TEXT NULL,
                 available INTEGER NOT NULL,
                 cached_file_size INTEGER NULL,
                 cached_modified_utc TEXT NULL,
                 cached_sha256 TEXT NULL
             ) WITHOUT ROWID;

             DROP TABLE IF EXISTS temp.current_repeated_sizes;
             CREATE TEMP TABLE current_repeated_sizes(file_size INTEGER PRIMARY KEY) WITHOUT ROWID;"
        )?;

        // Fetch tracks and their existing cached fingerprints
        let mut stmt = conn.prepare(
            "SELECT t.path, t.file_size, t.modified_utc, f.file_size, f.modified_utc, f.sha256
             FROM tracks t
             LEFT JOIN track_fingerprints f ON f.path = t.path"
        )?;

        let mut insert_state = conn.prepare(
            "INSERT INTO current_fingerprint_state(
                 path, file_size, modified_utc, available, cached_file_size, cached_modified_utc, cached_sha256
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        )?;

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let path_str: String = row.get(0)?;
            let cached_size: Option<i64> = row.get(3)?;
            let cached_modified: Option<String> = row.get(4)?;
            let cached_sha256: Option<String> = row.get(5)?;

            let path = Path::new(&path_str);
            if let Ok((live_size, live_modified)) = get_file_state(path) {
                insert_state.execute(params![
                    path_str,
                    live_size,
                    live_modified,
                    1,
                    cached_size,
                    cached_modified,
                    cached_sha256
                ])?;
            } else {
                insert_state.execute(params![
                    path_str,
                    Option::<i64>::None,
                    Option::<String>::None,
                    0,
                    cached_size,
                    cached_modified,
                    cached_sha256
                ])?;
            }
        }
        drop(rows);
        drop(stmt);
        drop(insert_state);

        // Populate repeated sizes: only sizes with COUNT(*) > 1 can possibly be duplicates!
        conn.execute(
            "INSERT INTO current_repeated_sizes(file_size)
             SELECT file_size FROM current_fingerprint_state
             WHERE available = 1 AND file_size IS NOT NULL
             GROUP BY file_size HAVING COUNT(*) > 1",
            [],
        )?;

        // Query work items that need hashing
        let mut work_stmt = conn.prepare(
            "SELECT c.path, c.file_size, c.modified_utc, c.cached_file_size, c.cached_modified_utc, c.cached_sha256
             FROM current_fingerprint_state c
             WHERE c.available = 1
               AND EXISTS (SELECT 1 FROM current_repeated_sizes s WHERE s.file_size = c.file_size)
               AND (c.cached_sha256 IS NULL 
                    OR c.cached_file_size != c.file_size 
                    OR c.cached_modified_utc != c.modified_utc)"
        )?;

        let mut to_hash = Vec::new();
        let mut work_rows = work_stmt.query([])?;
        while let Some(row) = work_rows.next()? {
            let path: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let modified: String = row.get(2)?;
            to_hash.push((path, size, modified));
        }
        drop(work_rows);
        drop(work_stmt);

        let mut upsert_fp = conn.prepare(
            "INSERT OR REPLACE INTO track_fingerprints(path, file_size, modified_utc, sha256)
             VALUES (?1, ?2, ?3, ?4)"
        )?;

        let count = to_hash.len();
        for (path_str, size, modified) in to_hash {
            if let Ok(hash) = hash_file(&path_str) {
                upsert_fp.execute(params![path_str, size, modified, hash])?;
            }
        }

        Ok(count)
    }

    /// Queries duplicate groups where copy count > 1.
    pub fn get_duplicate_groups(conn: &Connection, offset: usize, page_size: usize) -> SqlResult<Vec<ExactDuplicateGroup>> {
        let mut stmt = conn.prepare(
            "SELECT f.sha256, f.file_size, COUNT(*) as copy_count
             FROM track_fingerprints f
             JOIN tracks t ON t.path = f.path
             WHERE f.file_size = t.file_size AND f.modified_utc = t.modified_utc
             GROUP BY f.sha256, f.file_size
             HAVING COUNT(*) > 1
             ORDER BY copy_count DESC, f.file_size DESC
             LIMIT ?1 OFFSET ?2"
        )?;

        let mut groups = Vec::new();
        let mut rows = stmt.query(params![page_size as i64, offset as i64])?;

        while let Some(row) = rows.next()? {
            let sha256: String = row.get(0)?;
            let file_size: i64 = row.get(1)?;
            let copy_count: i64 = row.get(2)?;

            // Grab the first representative track
            let mut rep_stmt = conn.prepare(
                "SELECT t.*
                 FROM tracks t
                 JOIN track_fingerprints f ON f.path = t.path
                 WHERE f.sha256 = ?1
                 ORDER BY t.path
                 LIMIT 1"
            )?;

            let representative = rep_stmt.query_row(params![sha256], |r| row_to_track(r))?;

            groups.push(ExactDuplicateGroup {
                sha256,
                file_size,
                copy_count: copy_count as usize,
                representative,
            });
        }

        Ok(groups)
    }

    /// Returns every indexed file that belongs to an exact-content group.
    pub fn get_duplicate_tracks(conn: &Connection, sha256: &str) -> SqlResult<Vec<Track>> {
        let mut stmt = conn.prepare(
            "SELECT t.* FROM tracks t
             JOIN track_fingerprints f ON f.path = t.path
             WHERE f.sha256 = ?1 AND f.file_size = t.file_size AND f.modified_utc = t.modified_utc
             ORDER BY t.path",
        )?;
        let rows = stmt.query_map(params![sha256], row_to_track)?;
        rows.collect()
    }
}
