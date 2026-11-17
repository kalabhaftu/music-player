//! Database-backed scan state (LibraryScanSession.cs). Unseen tracks under a
//! completed root are removed only when the root is committed, protecting
//! blocked subtrees and legacy exclusion paths.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::scan::{normalize_path, root_prefix, ScanExcludedPathKind};

const SEEN_BATCH_SIZE: usize = 256;

struct CachedTrackState {
    size: i64,
    modified: DateTime<Utc>,
    has_lyrics: i64,
    artwork: Option<String>,
}

/// One indexing run. Opens its own connection against the library database so
/// scans can run on a worker thread while the UI keeps querying.
pub struct LibraryScanSession {
    conn: Connection,
    run_id: i64,
    pending_seen: Vec<PathBuf>,
    pending_seen_set: std::collections::HashSet<String>,
    pending_protected: Vec<(PathBuf, Option<ScanExcludedPathKind>)>,
    cached_state: HashMap<String, CachedTrackState>,
    completed: bool,
}

fn parse_stamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn stamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

impl LibraryScanSession {
    pub fn begin(db_path: &Path, roots: &[PathBuf]) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        crate::db::schema::initialize_schema(&conn)?;

        let stale = stamp(Utc::now() - chrono::Duration::days(1));
        conn.execute_batch(&format!(
            r#"
            BEGIN;
            DELETE FROM scan_seen WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < '{stale}');
            DELETE FROM scan_excluded WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < '{stale}');
            DELETE FROM scan_user_ignored WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < '{stale}');
            DELETE FROM scan_default_excluded WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < '{stale}');
            DELETE FROM scan_incomplete_paths WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < '{stale}');
            DELETE FROM scan_roots WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < '{stale}');
            DELETE FROM scan_runs WHERE started_utc < '{stale}';
            COMMIT;
            "#,
            stale = stale
        ))?;

        conn.execute("INSERT INTO scan_runs(started_utc) VALUES (?1)", params![stamp(Utc::now())])?;
        let run_id = conn.last_insert_rowid();
        for root in roots {
            conn.execute(
                "INSERT OR IGNORE INTO scan_roots(run_id, root_path, root_prefix) VALUES (?1, ?2, ?3)",
                params![run_id, normalize_path(root).to_string_lossy(), root_prefix(root)],
            )?;
        }

        // Load cached track state for unchanged-detection.
        let mut cached_state = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT path, file_size, modified_utc, has_lyrics, artwork_path FROM tracks",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let size: i64 = row.get(1)?;
                let modified_raw: String = row.get(2)?;
                let has_lyrics: i64 = row.get(3)?;
                let artwork: Option<String> = row.get(4)?;
                if let Some(modified) = parse_stamp(&modified_raw) {
                    cached_state.insert(
                        path.to_lowercase(),
                        CachedTrackState { size, modified, has_lyrics, artwork },
                    );
                }
            }
        }

        Ok(Self {
            conn,
            run_id,
            pending_seen: Vec::new(),
            pending_seen_set: std::collections::HashSet::new(),
            pending_protected: Vec::new(),
            cached_state,
            completed: false,
        })
    }

    /// True when the file's size, mtime, lyrics flag, and cached artwork are
    /// unchanged since the last index (IsUnchanged port).
    pub fn is_unchanged(
        &mut self,
        path: &Path,
        length: u64,
        modified_utc: DateTime<Utc>,
        artwork_cache: &Path,
    ) -> bool {
        let full = path.to_string_lossy().to_string();
        let key = full.to_lowercase();
        let cache_root = artwork_cache.to_string_lossy().trim_end_matches(['/', '\\']).to_lowercase();

        if let Some(cached) = self.cached_state.get(&key) {
            if cached.size != length as i64 || cached.modified != modified_utc || cached.has_lyrics < 0 {
                return false;
            }
            match &cached.artwork {
                None => true,
                Some(art) => {
                    let art = art.to_lowercase();
                    (art.starts_with(&format!("{}\\", cache_root))
                        || art.starts_with(&format!("{}/", cache_root)))
                        && Path::new(&art).exists()
                }
            }
        } else {
            false
        }
    }

    pub fn mark_seen(&mut self, path: &Path) {
        let full = path.to_string_lossy().to_string();
        if self.pending_seen_set.insert(full.to_lowercase()) {
            self.pending_seen.push(PathBuf::from(&full));
        }
        if self.pending_seen.len() >= SEEN_BATCH_SIZE {
            let _ = self.flush_seen();
        }
    }

    pub fn mark_root_completed(&mut self, root: &Path, complete: bool) {
        let _ = self.conn.execute(
            "UPDATE scan_roots SET completed = ?1 WHERE run_id = ?2 AND root_path = ?3",
            params![if complete { 1 } else { 0 }, self.run_id, normalize_path(root).to_string_lossy()],
        );
    }

    /// Protects a subtree whose contents could not be fully inspected.
    pub fn mark_path_incomplete(&mut self, path: &Path) {
        self.pending_protected.push((normalize_path(path), None));
        if self.pending_protected.len() >= SEEN_BATCH_SIZE {
            let _ = self.flush_protected();
        }
    }

    pub fn mark_excluded_path(&mut self, path: &Path, kind: ScanExcludedPathKind) {
        self.pending_protected.push((normalize_path(path), Some(kind)));
        if self.pending_protected.len() >= SEEN_BATCH_SIZE {
            let _ = self.flush_protected();
        }
    }

    fn flush_protected(&mut self) -> Result<()> {
        if self.pending_protected.is_empty() {
            return Ok(());
        }
        self.conn.execute_batch("BEGIN")?;
        for (path, kind) in self.pending_protected.drain(..) {
            let prefix = root_prefix(&path);
            let table = match kind {
                None => "scan_incomplete_paths",
                Some(ScanExcludedPathKind::DefaultExcluded) => "scan_default_excluded",
                Some(ScanExcludedPathKind::UserIgnored) => "scan_user_ignored",
                Some(ScanExcludedPathKind::ReparsePoint) => "scan_excluded",
            };
            self.conn.execute(
                &format!("INSERT OR IGNORE INTO {}(run_id, path, prefix) VALUES (?1, ?2, ?3)", table),
                params![self.run_id, path.to_string_lossy(), prefix],
            )?;
        }
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn flush_seen(&mut self) -> Result<()> {
        if self.pending_seen.is_empty() {
            return Ok(());
        }
        self.conn.execute_batch("BEGIN")?;
        for path in self.pending_seen.drain(..) {
            self.conn.execute(
                "INSERT OR IGNORE INTO scan_seen(run_id, path) VALUES (?1, ?2)",
                params![self.run_id, path.to_string_lossy()],
            )?;
        }
        self.conn.execute_batch("COMMIT")?;
        self.pending_seen_set.clear();
        Ok(())
    }

    /// Removes indexed tracks under completed roots that were never seen,
    /// except protected excluded/incomplete subtrees. Returns the removed count.
    pub fn complete(&mut self) -> Result<usize> {
        self.flush_seen()?;
        self.flush_protected()?;
        let run = self.run_id;
        self.conn.execute_batch("BEGIN")?;
        let mut removed = self
            .conn
            .execute(
                r#"
                DELETE FROM tracks
                WHERE EXISTS (
                  SELECT 1 FROM scan_roots r
                  WHERE r.run_id = ?1 AND r.completed = 1
                    AND (tracks.path COLLATE NOCASE = r.root_path OR
                         substr(tracks.path, 1, length(r.root_prefix)) COLLATE NOCASE = r.root_prefix)
                )
                AND NOT EXISTS (
                  SELECT 1 FROM scan_seen s
                  WHERE s.run_id = ?1 AND tracks.path COLLATE NOCASE = s.path
                )
                AND NOT EXISTS (
                  SELECT 1 FROM scan_excluded e
                  WHERE e.run_id = ?1 AND
                    (tracks.path COLLATE NOCASE = e.path OR
                     substr(tracks.path, 1, length(e.prefix)) COLLATE NOCASE = e.prefix)
                )
                AND NOT EXISTS (
                  SELECT 1 FROM scan_roots incomplete
                  WHERE incomplete.run_id = ?1 AND incomplete.completed = 0
                    AND (tracks.path COLLATE NOCASE = incomplete.root_path OR
                         substr(tracks.path, 1, length(incomplete.root_prefix)) COLLATE NOCASE = incomplete.root_prefix)
                )
                AND NOT EXISTS (
                  SELECT 1 FROM scan_incomplete_paths incomplete
                  WHERE incomplete.run_id = ?1 AND
                    (tracks.path COLLATE NOCASE = incomplete.path OR
                     substr(tracks.path, 1, length(incomplete.prefix)) COLLATE NOCASE = incomplete.prefix)
                )
                AND NOT EXISTS (
                  SELECT 1 FROM scan_default_excluded e
                  WHERE e.run_id = ?1 AND
                    (tracks.path COLLATE NOCASE = e.path OR
                     substr(tracks.path, 1, length(e.prefix)) COLLATE NOCASE = e.prefix)
                    AND EXISTS (
                      SELECT 1 FROM scan_roots overlap
                      WHERE overlap.run_id = ?1 AND
                        (overlap.root_path COLLATE NOCASE = e.path OR
                         substr(overlap.root_path, 1, length(e.prefix)) COLLATE NOCASE = e.prefix)
                        AND (tracks.path COLLATE NOCASE = overlap.root_path OR
                             substr(tracks.path, 1, length(overlap.root_prefix)) COLLATE NOCASE = overlap.root_prefix)
                    )
                )
                "#,
                params![run],
            )?;
        removed += self.conn.execute(
            r#"
            DELETE FROM tracks
            WHERE EXISTS (
              SELECT 1 FROM scan_user_ignored u
              WHERE u.run_id = ?1 AND
                (tracks.path COLLATE NOCASE = u.path OR
                 substr(tracks.path, 1, length(u.prefix)) COLLATE NOCASE = u.prefix)
            )
            "#,
            params![run],
        )?;
        for table in [
            "scan_seen",
            "scan_user_ignored",
            "scan_excluded",
            "scan_default_excluded",
            "scan_incomplete_paths",
            "scan_roots",
            "scan_runs",
        ] {
            self.conn.execute(&format!("DELETE FROM {} WHERE run_id = ?1", table), params![run])?;
        }
        self.conn.execute_batch("COMMIT")?;
        self.completed = true;
        Ok(removed)
    }
}

impl Drop for LibraryScanSession {
    fn drop(&mut self) {
        if !self.completed {
            // An abandoned generation is cleaned up by the next scan's stale purge.
            let _ = self.conn.execute_batch("ROLLBACK");
            let run = self.run_id;
            let _ = self.conn.execute_batch("BEGIN");
            for table in [
                "scan_seen",
                "scan_user_ignored",
                "scan_excluded",
                "scan_default_excluded",
                "scan_incomplete_paths",
                "scan_roots",
                "scan_runs",
            ] {
                if table == "scan_runs" {
                    let _ = self.conn.execute(&format!("DELETE FROM {} WHERE id = ?1", table), params![run]);
                } else {
                    let _ = self.conn.execute(&format!("DELETE FROM {} WHERE run_id = ?1", table), params![run]);
                }
            }
            let _ = self.conn.execute_batch("COMMIT");
        }
    }
}
