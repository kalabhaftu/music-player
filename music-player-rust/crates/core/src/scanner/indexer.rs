//! Library indexing port (LibraryIndexer.cs + LibraryScanSession.cs): walks
//! roots, reads tags in bounded batches, tracks seen paths for pruning, and
//! refreshes previously indexed files.

use super::scan::{is_within_root, normalize_path, scan_paths, ScanCallbacks, ScanExcludedPathKind};
use super::session::LibraryScanSession;
use super::walker::ScannerControl;
use crate::db::tracks::TrackQuery;
use crate::db::LibraryStore;
use crate::models::{ScanProgress, Track, TrackSort};
use chrono::{DateTime, Utc};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

pub const SCAN_PAGE_SIZE: u32 = 400;

#[derive(Debug, Clone, Default)]
pub struct IndexResult {
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
    pub unavailable_roots: Vec<String>,
    pub incomplete_paths: Vec<String>,
    pub files_found: usize,
}

/// In-memory session fallback for `:memory:` databases, where a second
/// connection would not share state with the store under test.
struct MemoryScanSession {
    root_completed: Vec<(PathBuf, bool)>,
    seen: HashSet<String>,
    excluded: Vec<(String, ScanExcludedPathKind)>,
    incomplete: Vec<String>,
    initial_paths: HashSet<String>,
    completed: bool,
}

impl MemoryScanSession {
    fn begin(store: &LibraryStore) -> Self {
        let mut initial_paths = HashSet::new();
        let mut offset = 0u32;
        loop {
            let page = store
                .get_track_paths_page(None, None, offset, 800)
                .unwrap_or_default();
            let count = page.len();
            initial_paths.extend(page);
            if count < 800 {
                break;
            }
            offset += 800;
        }
        Self {
            root_completed: Vec::new(),
            seen: HashSet::new(),
            excluded: Vec::new(),
            incomplete: Vec::new(),
            initial_paths,
            completed: false,
        }
    }

    fn is_unchanged(&self, _path: &Path, _len: u64, _modified: DateTime<Utc>, _cache: &Path) -> bool {
        // The memory store has no cached prior state; re-read everything.
        false
    }

    fn mark_seen(&mut self, path: &Path) {
        self.seen.insert(path.to_string_lossy().to_lowercase());
    }

    fn mark_root_completed(&mut self, root: &Path, complete: bool) {
        self.root_completed.push((normalize_path(root), complete));
    }

    fn mark_path_incomplete(&mut self, path: &Path) {
        self.incomplete.push(normalize_path(path).to_string_lossy().to_string());
    }

    fn mark_excluded_path(&mut self, path: &Path, kind: ScanExcludedPathKind) {
        self.excluded
            .push((normalize_path(path).to_string_lossy().to_string(), kind));
    }

    fn protected_prefixes(&self) -> Vec<String> {
        let mut protected: Vec<String> = self.excluded.iter().map(|(p, _)| p.clone()).collect();
        protected.extend(self.incomplete.iter().cloned());
        protected
    }

    fn removed_paths(&self) -> Vec<String> {
        let protected = self.protected_prefixes();
        let mut out = Vec::new();
        for (root, complete) in &self.root_completed {
            if !complete {
                continue;
            }
            let prefix = root.to_string_lossy().to_lowercase();
            for path in self.initial_paths.iter() {
                let lower = path.to_lowercase();
                let is_under = lower.starts_with(&prefix) || lower == prefix.trim_end_matches(['\\', '/']);
                if !is_under
                    || self.seen.contains(&lower)
                    || protected.iter().any(|p| lower.starts_with(&p.to_lowercase()))
                {
                    continue;
                }
                out.push(path.clone());
            }
        }
        out
    }

    fn complete(&mut self) -> usize {
        let removed = self.removed_paths().len();
        self.completed = true;
        removed
    }
}

enum SessionMode {
    Database(LibraryScanSession),
    Memory(MemoryScanSession),
}

impl SessionMode {
    fn is_unchanged(&mut self, path: &Path, len: u64, modified: DateTime<Utc>, cache: &Path) -> bool {
        match self {
            SessionMode::Database(db) => db.is_unchanged(path, len, modified, cache),
            SessionMode::Memory(mem) => mem.is_unchanged(path, len, modified, cache),
        }
    }

    fn mark_seen(&mut self, path: &Path) {
        match self {
            SessionMode::Database(db) => db.mark_seen(path),
            SessionMode::Memory(mem) => mem.mark_seen(path),
        }
    }

    fn mark_root_completed(&mut self, root: &Path, complete: bool) {
        match self {
            SessionMode::Database(db) => db.mark_root_completed(root, complete),
            SessionMode::Memory(mem) => mem.mark_root_completed(root, complete),
        }
    }

    fn mark_path_incomplete(&mut self, path: &Path) {
        match self {
            SessionMode::Database(db) => db.mark_path_incomplete(path),
            SessionMode::Memory(mem) => mem.mark_path_incomplete(path),
        }
    }

    fn mark_excluded_path(&mut self, path: &Path, kind: ScanExcludedPathKind) {
        match self {
            SessionMode::Database(db) => db.mark_excluded_path(path, kind),
            SessionMode::Memory(mem) => mem.mark_excluded_path(path, kind),
        }
    }

    fn flush_seen(&mut self) {
        if let SessionMode::Database(db) = self {
            let _ = db.flush_seen();
        }
    }

    fn complete(&mut self) -> usize {
        match self {
            SessionMode::Database(db) => db.complete().unwrap_or(0),
            SessionMode::Memory(mem) => mem.complete(),
        }
    }

    fn removed_paths(&mut self) -> Vec<String> {
        match self {
            SessionMode::Memory(mem) => mem.removed_paths(),
            _ => Vec::new(),
        }
    }
}

/// Shared mutable state for the scan run (single-threaded; the RefCell lets
/// the traversal callbacks and the file handler share it).
struct ScanRun<'s> {
    store: &'s LibraryStore,
    artwork_cache: PathBuf,
    batch_size: usize,
    pending: Vec<Track>,
    result: IndexResult,
    since_flush: Instant,
    session: RefCell<SessionMode>,
    force_refresh: bool,
    control: Arc<ScannerControl>,
}

impl<'s> ScanRun<'s> {
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if let Ok(count) = self.store.upsert_tracks(&self.pending) {
            self.result.indexed += count;
        }
        self.pending.clear();
        self.since_flush = Instant::now();
    }
}

/// Indexes every enabled media file under the supplied roots (ScanAsync port).
pub struct LibraryIndexer<'a> {
    store: &'a LibraryStore,
    artwork_cache: PathBuf,
    batch_size: usize,
}

impl<'a> LibraryIndexer<'a> {
    pub fn new(store: &'a LibraryStore) -> Self {
        Self {
            store,
            artwork_cache: crate::metadata::artwork_cache_dir(),
            batch_size: 64,
        }
    }

    pub fn with_artwork_cache(mut self, cache: PathBuf) -> Self {
        self.artwork_cache = cache;
        self
    }

    pub fn scan_async(
        &self,
        roots: &[PathBuf],
        ignored_directories: &[PathBuf],
        control: Arc<ScannerControl>,
        mut progress: Option<&mut dyn FnMut(&ScanProgress)>,
        force_refresh: bool,
        supported_extensions: Option<&HashSet<String>>,
    ) -> Result<IndexResult, String> {
        let session = if self.store.db_path() == Path::new(":memory:") {
            SessionMode::Memory(MemoryScanSession::begin(self.store))
        } else {
            SessionMode::Database(
                LibraryScanSession::begin(self.store.db_path(), roots)
                    .map_err(|e| format!("Could not begin a library scan session: {}", e))?,
            )
        };

        // A scan reconciles the current filesystem: purge rows under default
        // and user excluded directories before walking.
        let mut excluded_purged = 0usize;
        if let Ok(count) = self.store.purge_tracks_in_default_excluded_directories() {
            excluded_purged += count;
        }
        if !ignored_directories.is_empty() {
            let prefixes: Vec<String> = ignored_directories
                .iter()
                .map(|p| normalize_path(p).to_string_lossy().to_string())
                .collect();
            if let Ok(count) = self.store.purge_tracks_under_directories(&prefixes) {
                excluded_purged += count;
            }
        }

        let run = Rc::new(RefCell::new(ScanRun {
            store: self.store,
            artwork_cache: self.artwork_cache.clone(),
            batch_size: self.batch_size,
            pending: Vec::with_capacity(self.batch_size),
            result: IndexResult::default(),
            since_flush: Instant::now(),
            session: RefCell::new(session),
            force_refresh,
            control: control.clone(),
        }));

        {
            let run_for_files = run.clone();
            let mut on_audio_file = move |path: &Path| {
                let mut run = run_for_files.borrow_mut();
                if run.control.is_cancelled() {
                    return;
                }
                run.result.files_found += 1;
                match std::fs::metadata(path) {
                    Err(_) => {
                        run.session.borrow_mut().mark_seen(path);
                        run.result.skipped += 1;
                    }
                    Ok(info) => {
                        let modified: DateTime<Utc> = info
                            .modified()
                            .map(DateTime::from)
                            .unwrap_or_else(|_| Utc::now());
                        let cache = run.artwork_cache.clone();
                        let unchanged =
                            run.session
                                .borrow_mut()
                                .is_unchanged(path, info.len(), modified, &cache);
                        if !run.force_refresh && unchanged {
                            run.session.borrow_mut().mark_seen(path);
                            return;
                        }
                        let track = build_track(path, info.len(), modified, &run.artwork_cache);
                        run.session.borrow_mut().mark_seen(path);
                        run.pending.push(track);
                        if run.pending.len() >= run.batch_size {
                            run.flush();
                        }
                    }
                }
            };

            let run_for_dirs = run.clone();
            let mut dir_visited = move |_: &Path| {
                let mut run = run_for_dirs.borrow_mut();
                // Make a small newly discovered library usable immediately, then
                // bound tail writes to once per second.
                if run.result.indexed == 0 || run.since_flush.elapsed() >= std::time::Duration::from_secs(1) {
                    run.flush();
                }
            };

            let run_for_roots = run.clone();
            let mut root_completed = move |root: &Path, complete: bool| {
                let mut run = run_for_roots.borrow_mut();
                run.flush();
                run.session.borrow_mut().mark_root_completed(root, complete);
            };

            let run_for_unavail = run.clone();
            let mut root_unavailable = move |root: &Path| {
                run_for_unavail
                    .borrow_mut()
                    .result
                    .unavailable_roots
                    .push(root.to_string_lossy().to_string());
            };

            let run_for_incomplete = run.clone();
            let mut path_incomplete = move |path: &Path| {
                let mut run = run_for_incomplete.borrow_mut();
                run.session.borrow_mut().mark_path_incomplete(path);
                run.result.incomplete_paths.push(path.to_string_lossy().to_string());
            };

            let run_for_excluded = run.clone();
            let mut path_excluded = move |path: &Path, kind: ScanExcludedPathKind| {
                run_for_excluded
                    .borrow_mut()
                    .session
                    .borrow_mut()
                    .mark_excluded_path(path, kind);
            };

            let mut callbacks = ScanCallbacks {
                progress: progress.as_mut().map(|f| &mut **f),
                directory_visited: Some(&mut dir_visited),
                root_completed: Some(&mut root_completed),
                root_unavailable: Some(&mut root_unavailable),
                path_incomplete: Some(&mut path_incomplete),
                path_excluded: Some(&mut path_excluded),
            };
            scan_paths(
                roots,
                ignored_directories,
                control.clone(),
                &mut on_audio_file,
                &mut callbacks,
                supported_extensions,
            );

            {
                let mut run = run.borrow_mut();
                if run.control.is_cancelled() {
                    // Preserve completed metadata reads even on cancellation.
                    run.flush();
                    run.session.borrow_mut().flush_seen();
                    return Err("cancelled".to_string());
                }
                run.flush();
            }
        }

        // Refresh already-indexed files: explicit rebuilds repair files that no
        // longer sit under today's roots; normal scans cheaply revisit only
        // broken artwork references.
        let repaired = self.refresh_indexed_files(
            force_refresh,
            roots,
            ignored_directories,
            control.clone(),
            progress.as_mut().map(|f| &mut **f),
        );
        run.borrow_mut().result.indexed += repaired;

        let removed_from_session = {
            let run = run.borrow_mut();
            let removed_paths = run.session.borrow_mut().removed_paths();
            let count = run.session.borrow_mut().complete();
            if !removed_paths.is_empty() {
                let _ = self.store.purge_deleted_tracks(&removed_paths);
            }
            count
        };
        let removed = removed_from_session + excluded_purged;
        run.borrow_mut().result.removed = removed;

        {
            let mut run = run.borrow_mut();
            if run.result.indexed > 0 || removed > 0 || force_refresh {
                let _ = crate::metadata::prune_unreferenced_artwork(&self.artwork_cache, self.store);
            }
            let mut result = std::mem::take(&mut run.result);
            result.unavailable_roots.dedup();
            result.incomplete_paths.dedup();
            Ok(result)
        }
    }

    fn refresh_indexed_files(
        &self,
        force_refresh: bool,
        scan_roots: &[PathBuf],
        ignored_directories: &[PathBuf],
        control: Arc<ScannerControl>,
        mut progress: Option<&mut (dyn FnMut(&ScanProgress) + '_)>,
    ) -> usize {
        let mut changed = 0usize;
        let mut batch: Vec<Track> = Vec::with_capacity(32);

        if force_refresh {
            let mut offset = 0u32;
            loop {
                if control.is_cancelled() {
                    break;
                }
                wait_if_paused(&control);
                let query = TrackQuery {
                    sort: TrackSort::Path,
                    offset,
                    page_size: SCAN_PAGE_SIZE,
                    include_unavailable: true,
                    ..Default::default()
                };
                let page = match self.store.get_tracks_page(&query) {
                    Ok(page) => page,
                    Err(_) => break,
                };
                for track in &page {
                    if control.is_cancelled() {
                        return changed;
                    }
                    wait_if_paused(&control);
                    if ignored_directories.iter().any(|ign| is_within_root(&track.path, ign)) {
                        continue;
                    }
                    if scan_roots.iter().any(|root| is_within_root(&track.path, root)) {
                        continue;
                    }
                    if !Path::new(&track.path).exists() {
                        continue;
                    }
                    batch.push(self.rebuild_track(track));
                    if batch.len() >= 32 {
                        if let Ok(count) = self.store.upsert_tracks(&batch) {
                            changed += count;
                        }
                        batch.clear();
                        if let Some(cb) = progress.as_mut() {
                            cb(&ScanProgress {
                                files_found: 0,
                                directories_visited: 0,
                                current_path: track.path.clone(),
                                indexed_tracks: changed as i32,
                            });
                        }
                    }
                }
                if (page.len() as u32) < SCAN_PAGE_SIZE {
                    break;
                }
                offset += SCAN_PAGE_SIZE;
            }
        } else {
            for root in scan_roots {
                let mut offset = 0u32;
                loop {
                    if control.is_cancelled() {
                        break;
                    }
                    wait_if_paused(&control);
                    let query = TrackQuery {
                        group_column: Some("folder".to_string()),
                        group_value: Some(root.to_string_lossy().to_string()),
                        sort: TrackSort::Path,
                        offset,
                        page_size: SCAN_PAGE_SIZE,
                        ..Default::default()
                    };
                    let page = match self.store.get_tracks_page(&query) {
                        Ok(page) => page,
                        Err(_) => break,
                    };
                    for track in &page {
                        if control.is_cancelled() {
                            return changed;
                        }
                        wait_if_paused(&control);
                        if ignored_directories.iter().any(|ign| is_within_root(&track.path, ign)) {
                            continue;
                        }
                        // 1:1 with LibraryIndexer.RefreshIndexedFilesAsync:
                        // only non-empty references whose cache file vanished are
                        // re-read here. Rows with no artwork are left alone (the
                        // startup migration handles legacy NULL rows).
                        let artwork_missing = track
                            .artwork_path
                            .as_deref()
                            .map(|path| {
                                !path.trim().is_empty() && !Path::new(path).exists()
                            })
                            .unwrap_or(false);
                        if !artwork_missing || !Path::new(&track.path).exists() {
                            continue;
                        }
                        batch.push(self.rebuild_track(track));
                        if batch.len() >= 32 {
                            if let Ok(count) = self.store.upsert_tracks(&batch) {
                                changed += count;
                            }
                            batch.clear();
                            if let Some(cb) = progress.as_mut() {
                                cb(&ScanProgress {
                                    files_found: 0,
                                    directories_visited: 0,
                                    current_path: track.path.clone(),
                                    indexed_tracks: changed as i32,
                                });
                            }
                        }
                    }
                    if (page.len() as u32) < SCAN_PAGE_SIZE {
                        break;
                    }
                    offset += SCAN_PAGE_SIZE;
                }
            }
        }
        if !batch.is_empty() {
            if let Ok(count) = self.store.upsert_tracks(&batch) {
                changed += count;
            }
            batch.clear();
        }
        changed
    }

    fn rebuild_track(&self, track: &Track) -> Track {
        let modified = std::fs::metadata(Path::new(&track.path))
            .ok()
            .and_then(|m| m.modified().ok())
            .map(DateTime::from)
            .unwrap_or_else(Utc::now);
        build_track(Path::new(&track.path), track.file_size, modified, &self.artwork_cache)
    }
}

fn wait_if_paused(control: &ScannerControl) {
    while control.is_paused() && !control.is_cancelled() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Reads tags for one file into a Track row (TrackReader.Read port with the
/// filename fallbacks used by the indexer).
fn build_track(path: &Path, file_size: u64, modified_utc: DateTime<Utc>, artwork_cache: &Path) -> Track {
    let full_path = path.to_string_lossy().to_string();
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown Track")
        .to_string();
    let added_utc = Utc::now();

    let tags = crate::metadata::read_tags_in_cache(path, artwork_cache);

    let (stem_artist, stem_title) = match file_stem.split_once(" - ") {
        Some((a, t)) => (Some(a.trim().to_string()), Some(t.trim().to_string())),
        None => (None, None),
    };

    let title = tags
        .title
        .filter(|t| !t.trim().is_empty())
        .or(stem_title)
        .unwrap_or_else(|| file_stem.clone());
    let artist = tags
        .artist
        .filter(|a| !a.trim().is_empty())
        .or(stem_artist)
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album_artist = tags
        .album_artist
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| artist.clone());
    let album = tags
        .album
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| "Unknown Album".to_string());

    let has_sidecar = path.with_extension("lrc").exists();

    Track {
        path: full_path,
        title,
        artist,
        album,
        album_artist,
        genre: tags.genre.unwrap_or_default(),
        year: tags.year,
        track_number: tags.track_number,
        duration_ms: tags.duration_ms,
        file_size,
        modified_utc,
        added_utc,
        favorite: false,
        rating: 0,
        play_count: 0,
        last_played_utc: None,
        artwork_path: tags.artwork_path,
        has_lyrics: has_sidecar || tags.has_lyrics,
        unavailable: false,
    }
}
