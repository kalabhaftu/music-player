//! Database-backed scan state (LibraryScanSession.cs). Unseen tracks under a
//! completed root are removed only when the root is committed, protecting
//! blocked subtrees and legacy exclusion paths.

use chrono::{DateTime, Utc};
use rusqlite::{params, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::scan::{normalize_path, root_prefix, ScanExcludedPathKind};
use crate::db::LibraryStore;

const SEEN_BATCH_SIZE: usize = 1024;

struct CachedTrackState {
    size: i64,
    modified: DateTime<Utc>,
    has_lyrics: i64,
    artwork: Option<String>,
}

/// One indexing run. Routes all mutations through the single library database writer
/// while reading track state concurrently via independent WAL readers.
pub struct LibraryScanSession {
    store: LibraryStore,
    run_id: i64,
    pending_seen: Vec<PathBuf>,
    pending_seen_set: std::collections::HashSet<String>,
    pending_protected: Vec<(PathBuf, Option<ScanExcludedPathKind>)>,
    cached_state: HashMap<String, CachedTrackState>,
    artwork_cache_prefix_backslash: Option<String>,
    artwork_cache_prefix_slash: Option<String>,
    existing_artwork_paths: Option<std::collections::HashSet<String>>,
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
    pub fn begin(store: &LibraryStore, roots: &[PathBuf]) -> Result<Self> {
        let stale = stamp(Utc::now() - chrono::Duration::days(1));
        let roots_owned = roots.to_vec();

        let run_id = store.write(move |conn| {
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

            conn.execute(
                "INSERT INTO scan_runs(started_utc) VALUES (?1)",
                params![stamp(Utc::now())],
            )?;
            let run_id = conn.last_insert_rowid();
            for root in &roots_owned {
                conn.execute(
                    "INSERT OR IGNORE INTO scan_roots(run_id, root_path, root_prefix) VALUES (?1, ?2, ?3)",
                    params![run_id, normalize_path(root).to_string_lossy(), root_prefix(root)],
                )?;
            }
            Ok(run_id)
        })?;

        // Load cached track state for unchanged-detection via reader connection.
        let mut cached_state = HashMap::new();
        {
            let conn = store.read_conn()?;
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
                        CachedTrackState {
                            size,
                            modified,
                            has_lyrics,
                            artwork,
                        },
                    );
                }
            }
        }

        Ok(Self {
            store: store.clone(),
            run_id,
            pending_seen: Vec::new(),
            pending_seen_set: std::collections::HashSet::new(),
            pending_protected: Vec::new(),
            cached_state,
            artwork_cache_prefix_backslash: None,
            artwork_cache_prefix_slash: None,
            existing_artwork_paths: None,
            completed: false,
        })
    }

    fn ensure_artwork_snapshot(&mut self, artwork_cache: &Path) {
        if self.existing_artwork_paths.is_some() {
            return;
        }
        let cache_root = artwork_cache
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_lowercase();
        self.artwork_cache_prefix_backslash = Some(format!("{}\\", cache_root));
        self.artwork_cache_prefix_slash = Some(format!("{}/", cache_root));
        let mut set = std::collections::HashSet::new();
        if let Ok(entries) = std::fs::read_dir(artwork_cache) {
            for entry in entries.flatten() {
                set.insert(entry.path().to_string_lossy().to_lowercase());
            }
        }
        self.existing_artwork_paths = Some(set);
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
        self.ensure_artwork_snapshot(artwork_cache);
        let full = path.to_string_lossy().to_lowercase();

        if let Some(cached) = self.cached_state.get(&full) {
            if cached.size != length as i64
                || cached.modified != modified_utc
                || cached.has_lyrics < 0
            {
                return false;
            }
            match &cached.artwork {
                None => true,
                Some(art) => {
                    let art = art.to_lowercase();
                    let under_cache = self
                        .artwork_cache_prefix_backslash
                        .as_deref()
                        .map(|p| art.starts_with(p))
                        .unwrap_or(false)
                        || self
                            .artwork_cache_prefix_slash
                            .as_deref()
                            .map(|p| art.starts_with(p))
                            .unwrap_or(false);
                    under_cache
                        && self
                            .existing_artwork_paths
                            .as_ref()
                            .map(|set| set.contains(&art))
                            .unwrap_or(false)
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
        let root_path = normalize_path(root).to_string_lossy().to_string();
        let run_id = self.run_id;
        let _ = self.store.write(move |conn| {
            conn.execute(
                "UPDATE scan_roots SET completed = ?1 WHERE run_id = ?2 AND root_path = ?3",
                params![if complete { 1 } else { 0 }, run_id, root_path],
            )?;
            Ok(())
        });
    }

    /// Protects a subtree whose contents could not be fully inspected.
    pub fn mark_path_incomplete(&mut self, path: &Path) {
        self.pending_protected.push((normalize_path(path), None));
        if self.pending_protected.len() >= SEEN_BATCH_SIZE {
            let _ = self.flush_protected();
        }
    }

    pub fn mark_excluded_path(&mut self, path: &Path, kind: ScanExcludedPathKind) {
        self.pending_protected
            .push((normalize_path(path), Some(kind)));
        if self.pending_protected.len() >= SEEN_BATCH_SIZE {
            let _ = self.flush_protected();
        }
    }

    fn flush_protected(&mut self) -> Result<()> {
        if self.pending_protected.is_empty() {
            return Ok(());
        }
        let batch: Vec<_> = self.pending_protected.drain(..).collect();
        let run_id = self.run_id;
        self.store.write(move |conn| {
            let tx = conn.transaction()?;
            for (path, kind) in batch {
                let prefix = root_prefix(&path);
                let table = match kind {
                    None => "scan_incomplete_paths",
                    Some(ScanExcludedPathKind::DefaultExcluded) => "scan_default_excluded",
                    Some(ScanExcludedPathKind::UserIgnored) => "scan_user_ignored",
                    Some(ScanExcludedPathKind::ReparsePoint) => "scan_excluded",
                };
                tx.execute(
                    &format!(
                        "INSERT OR IGNORE INTO {}(run_id, path, prefix) VALUES (?1, ?2, ?3)",
                        table
                    ),
                    params![run_id, path.to_string_lossy(), prefix],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn flush_seen(&mut self) -> Result<()> {
        if self.pending_seen.is_empty() {
            return Ok(());
        }
        let batch: Vec<_> = self.pending_seen.drain(..).collect();
        let run_id = self.run_id;
        self.store.write(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt =
                    tx.prepare_cached("INSERT OR IGNORE INTO scan_seen(run_id, path) VALUES (?1, ?2)")?;
                for path in batch {
                    stmt.execute(params![run_id, path.to_string_lossy()])?;
                }
            }
            tx.commit()?;
            Ok(())
        })?;
        self.pending_seen_set.clear();
        Ok(())
    }

    /// Removes indexed tracks under completed roots that were never seen,
    /// except protected excluded/incomplete subtrees. Returns the removed count.
    pub fn complete(&mut self) -> Result<usize> {
        self.flush_seen()?;
        self.flush_protected()?;
        let run = self.run_id;
        let removed = self.store.write(move |conn| {
            let tx = conn.transaction()?;
            let mut removed = tx.execute(
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
            removed += tx.execute(
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
                let col = if table == "scan_runs" { "id" } else { "run_id" };
                tx.execute(
                    &format!("DELETE FROM {} WHERE {} = ?1", table, col),
                    params![run],
                )?;
            }
            tx.commit()?;
            Ok(removed)
        })?;
        self.completed = true;
        Ok(removed)
    }
}

impl Drop for LibraryScanSession {
    fn drop(&mut self) {
        if !self.completed {
            let run = self.run_id;
            let _ = self.store.write(move |conn| {
                let tx = conn.transaction()?;
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
                        let _ = tx.execute(
                            &format!("DELETE FROM {} WHERE id = ?1", table),
                            params![run],
                        );
                    } else {
                        let _ = tx.execute(
                            &format!("DELETE FROM {} WHERE run_id = ?1", table),
                            params![run],
                        );
                    }
                }
                tx.commit()?;
                Ok(())
            });
        }
    }
}
