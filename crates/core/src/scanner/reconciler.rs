//! Track availability reconciliation (TrackAvailabilityService.cs port):
//! distinguishes removed files from temporarily unreachable library locations
//! using bounded paged walks, safe filesystem probes, and reparse-point
//! boundary detection. The availability flag lives in the durable `unavailable`
//! column, so `LibraryQueryService`'s in-memory overlays collapse into it.

use crate::db::LibraryStore;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const PAGE_SIZE: u32 = 800;

/// Outcome of an availability reconciliation pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvailabilityReconciliation {
    pub removed_paths: Vec<String>,
    pub unavailable_count: usize,
    pub recovered_count: usize,
}

/// Distinguishes removed files from temporarily unreachable library locations.
#[derive(Clone)]
pub struct TrackAvailabilityService {
    store: LibraryStore,
    artwork_directory: PathBuf,
}

impl TrackAvailabilityService {
    pub fn new(store: &LibraryStore) -> Self {
        Self {
            store: store.clone(),
            artwork_directory: crate::metadata::artwork_cache_dir(),
        }
    }

    /// Overrides the artwork directory pruned after confirmed removals.
    pub fn with_artwork_directory(mut self, directory: PathBuf) -> Self {
        self.artwork_directory = directory;
        self
    }

    /// Walks every indexed track in bounded pages, purging confirmed deletions
    /// and marking unreachable locations unavailable (`ReconcileIndexedTracks`).
    pub fn reconcile_indexed_tracks(
        &self,
        unavailable_roots: &[String],
        incomplete_paths: &[String],
        scan_roots: Option<&[String]>,
    ) -> AvailabilityReconciliation {
        let mut removed: Vec<String> = Vec::new();
        let mut unavailable = 0usize;
        let mut recovered = 0usize;

        let mut protected_prefixes: Vec<String> = Vec::new();
        for source in [unavailable_roots, incomplete_paths] {
            for path in source {
                if let Some(prefix) = try_normalize_prefix(path) {
                    protected_prefixes.push(prefix);
                }
            }
        }
        let excluded_prefixes: Vec<String> = self
            .scan_exclusions()
            .iter()
            .filter_map(|path| try_normalize_prefix(path))
            .collect();

        let scopes: Vec<Option<String>> = match scan_roots {
            Some(roots) if !roots.is_empty() => roots.iter().map(|r| Some(r.clone())).collect(),
            _ => vec![None],
        };
        for scope in scopes {
            let mut offset = 0u32;
            loop {
                let paths = self
                    .store
                    .get_track_paths_page(
                        scope.as_ref().map(|_| "folder"),
                        scope.as_deref(),
                        offset,
                        PAGE_SIZE,
                    )
                    .unwrap_or_default();
                if paths.is_empty() {
                    break;
                }
                let mut removed_from_page: Vec<String> = Vec::new();
                for path in &paths {
                    if excluded_prefixes
                        .iter()
                        .any(|prefix| is_ancestor_directory(prefix, path))
                    {
                        removed_from_page.push(path.clone());
                        continue;
                    }
                    if protected_prefixes
                        .iter()
                        .any(|prefix| is_ancestor_directory(prefix, path))
                    {
                        unavailable += 1;
                        continue;
                    }
                    if Path::new(path).exists() {
                        if self.store.reinstate_track(path).unwrap_or(false) {
                            recovered += 1;
                        }
                        continue;
                    }

                    if !protected_prefixes
                        .iter()
                        .any(|prefix| is_ancestor_directory(prefix, path))
                        && self.is_confirmed_missing_file(path)
                    {
                        removed_from_page.push(path.clone());
                    } else {
                        let _ = self.store.set_track_unavailable(path, true);
                        unavailable += 1;
                    }
                }

                if !removed_from_page.is_empty() {
                    let _ = self.store.purge_deleted_tracks(&removed_from_page);
                    removed.extend(removed_from_page);
                    // Deletions collapse the ordered result set; revisit this
                    // page's offset so the shifted tracks are not skipped.
                } else {
                    offset += paths.len() as u32;
                    if (paths.len() as u32) < PAGE_SIZE {
                        break;
                    }
                }
            }
        }

        // Include saved references whose index row was pruned during scanning.
        let orphaned: Vec<String> = self
            .store
            .get_referenced_paths()
            .unwrap_or_default()
            .into_iter()
            .filter(|path| {
                excluded_prefixes
                    .iter()
                    .any(|prefix| is_ancestor_directory(prefix, path))
                    || (!Path::new(path).exists()
                        && !protected_prefixes
                            .iter()
                            .any(|prefix| is_ancestor_directory(prefix, path))
                        && self.is_confirmed_missing_file(path))
            })
            .collect();
        if !orphaned.is_empty() {
            let _ = self.store.purge_deleted_tracks(&orphaned);
            removed.extend(orphaned);
        }

        if !removed.is_empty() {
            self.prune_artwork();
        }
        AvailabilityReconciliation {
            removed_paths: removed,
            unavailable_count: unavailable,
            recovered_count: recovered,
        }
    }

    /// Marks a single track unavailable; purges it when the file is confirmed
    /// missing (`MarkUnavailable`). Returns the confirmed-missing verdict.
    pub fn mark_unavailable(&self, path: &str) -> bool {
        let _ = self.store.set_track_unavailable(path, true);
        let confirmed_missing = self.is_confirmed_missing_file(path);
        if confirmed_missing {
            let _ = self.store.purge_deleted_tracks(&[path.to_string()]);
            self.prune_artwork();
        }
        confirmed_missing
    }

    /// Restores an available track (`MarkAvailable`). Returns whether the
    /// durable flag actually changed.
    pub fn mark_available(&self, path: &str) -> bool {
        self.store.reinstate_track(path).unwrap_or(false)
    }

    /// Counts the indexed tracks directly inside each unavailable root
    /// (`MarkUnavailableRoots`).
    pub fn mark_unavailable_roots(&self, roots: &[String]) -> usize {
        let mut marked = 0usize;
        for root in roots {
            let mut offset = 0u32;
            loop {
                let paths = self
                    .store
                    .get_track_paths_page(Some("folder"), Some(root), offset, PAGE_SIZE)
                    .unwrap_or_default();
                marked += paths.len();
                if (paths.len() as u32) < PAGE_SIZE {
                    break;
                }
                offset += PAGE_SIZE;
            }
        }
        marked
    }

    /// Reconciles filesystem deletion events and returns the indexed/queued
    /// paths confirmed missing (`ReconcileRemovedPaths`).
    pub fn reconcile_removed_paths(&self, deleted_paths: &[String]) -> Vec<String> {
        let mut candidates: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut push_unique = |path: String| {
            if seen.insert(path.to_lowercase()) {
                candidates.push(path);
            }
        };

        for raw_path in deleted_paths {
            let Some(path) = try_normalize_prefix(raw_path) else {
                continue;
            };

            // A watcher can report an enclosing directory when a folder is
            // deleted. Resolve its indexed descendants in bounded pages
            // instead of dropping the whole root.
            if self
                .store
                .get_track(&path)
                .map_or(false, |track| track.is_some())
                || crate::scanner::is_supported_audio(Path::new(&path))
            {
                push_unique(path);
                continue;
            }

            let mut offset = 0u32;
            loop {
                let page = self
                    .store
                    .get_track_paths_page(Some("folder"), Some(&path), offset, PAGE_SIZE)
                    .unwrap_or_default();
                let count = page.len();
                for track_path in page {
                    push_unique(track_path);
                }
                if count < PAGE_SIZE as usize {
                    break;
                }
                offset += PAGE_SIZE;
            }
        }

        let mut confirmed_missing: Vec<String> = Vec::new();
        for path in &candidates {
            // Rename/delete notifications can race a replacement at the same path.
            if Path::new(path).exists() || !self.is_confirmed_missing_file(path) {
                continue;
            }
            let _ = self.store.set_track_unavailable(path, true);
            confirmed_missing.push(path.clone());
        }

        if !confirmed_missing.is_empty() {
            let _ = self.store.purge_deleted_tracks(&confirmed_missing);
            self.prune_artwork();
        }

        // Purged rows no longer appear in the library; survivors stay flagged.
        candidates
            .into_iter()
            .filter(|path| self.is_track_unavailable(path))
            .collect()
    }

    /// Clears the unavailable flag for tracks whose files exist again,
    /// returning the recovered paths (`ClearUnavailableTracksThatExist`).
    pub fn clear_unavailable_tracks_that_exist(&self) -> Vec<String> {
        self.store
            .clear_unavailable_tracks_that_exist()
            .unwrap_or_default()
    }

    fn is_track_unavailable(&self, path: &str) -> bool {
        match self.store.get_track(path) {
            Ok(Some(track)) => track.unavailable,
            // A purged row is definitionally outside the available library.
            Ok(None) => true,
            Err(_) => false,
        }
    }

    fn is_confirmed_missing_file(&self, path: &str) -> bool {
        if path.trim().is_empty() {
            return false;
        }
        let full = normalized_full_path(path);

        // A ready host volume is not proof that a path below a mounted folder
        // or junction is reachable. Preserve indexed rows behind a broken
        // reparse boundary until its target is available again.
        if Self::has_unavailable_reparse_boundary(&full) {
            return false;
        }

        let parent = full.parent();
        let parent_available = parent.is_some_and(|p| p.is_dir());
        let accessible_configured_root_contains_path = !parent_available
            && self
                .library_roots()
                .iter()
                .any(|root| is_ancestor_directory(root, &full.to_string_lossy()) && Path::new(root).is_dir());
        let storage_volume_available = Self::is_storage_volume_available(&full);
        if !parent_available && !accessible_configured_root_contains_path && !storage_volume_available
        {
            return false;
        }

        // GetAttributes success → present. NotFound covers both the file and
        // its parent directory missing (the guard above already established
        // the location is supposed to be reachable). Permission and other I/O
        // errors keep the row.
        match std::fs::metadata(&full) {
            Ok(_) => false,
            Err(err) => err.kind() == std::io::ErrorKind::NotFound,
        }
    }

    fn is_storage_volume_available(path: &Path) -> bool {
        let Some(root) = path.ancestors().last() else {
            return false;
        };
        if root.as_os_str().is_empty() {
            return false;
        }
        std::fs::metadata(root).is_ok()
    }

    fn has_unavailable_reparse_boundary(full_path: &Path) -> bool {
        let mut directory = full_path.parent();
        while let Some(dir) = directory {
            if dir.as_os_str().is_empty() {
                break;
            }
            match std::fs::symlink_metadata(dir) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        // ResolveLinkTarget(returnFinalTarget: true): the final
                        // destination must exist for the boundary to count as
                        // available.
                        match std::fs::read_link(dir) {
                            Ok(target) => {
                                if !target.is_dir() {
                                    return true;
                                }
                            }
                            Err(_) => return true,
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    directory = dir.parent();
                    continue;
                }
                Err(_) => return true,
            }
            match dir.parent() {
                Some(parent) if parent != dir => directory = Some(parent),
                _ => break,
            }
        }
        false
    }

    fn read_string_array_setting(&self, key: &str) -> Vec<String> {
        self.store
            .get_setting(key)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
            .unwrap_or_default()
    }

    fn library_roots(&self) -> Vec<String> {
        self.read_string_array_setting("library-roots")
    }

    fn scan_exclusions(&self) -> Vec<String> {
        self.read_string_array_setting("ignored-directories")
    }

    fn prune_artwork(&self) {
        crate::metadata::prune_unreferenced_artwork(&self.artwork_directory, &self.store);
    }
}

/// Trimmed full path that keeps drive roots intact (TryNormalizePrefix).
fn try_normalize_prefix(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Trimmed path that keeps drive roots intact (Path.GetFullPath normalization).
fn normalized_full_path(path: &str) -> PathBuf {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' {
        PathBuf::from(format!("{}\\", trimmed))
    } else {
        PathBuf::from(trimmed)
    }
}

/// Windows-style ancestor check: `path` strictly under `root` (IsAncestorDirectory).
fn is_ancestor_directory(root: &str, path: &str) -> bool {
    crate::db::availability::is_within_root(root, path)
}
