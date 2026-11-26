use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const RECOVERY_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

/// Bounds watcher bursts and throttles recovery after lost filesystem events.
/// (1:1 Port of LibraryWatchBatch.cs)
#[derive(Debug, Default)]
pub struct LibraryWatchBatch {
    directories: HashSet<PathBuf>,
    recovery_needed: bool,
    last_recovery: Option<Instant>,
}

pub struct DrainResult {
    pub directories: Vec<PathBuf>,
    pub full_scan: bool,
    pub retry_after: Option<Duration>,
}

impl LibraryWatchBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_directory<P: AsRef<Path>>(&mut self, path: P) {
        let p = path.as_ref().to_path_buf();
        self.directories.insert(p);
        if self.directories.len() > 512 {
            self.request_recovery();
        }
    }

    pub fn request_recovery(&mut self) {
        self.recovery_needed = true;
        self.directories.clear();
    }

    pub fn drain(&mut self, now: Instant) -> DrainResult {
        if self.recovery_needed {
            if let Some(prev) = self.last_recovery {
                let elapsed = now.saturating_duration_since(prev);
                if elapsed < RECOVERY_INTERVAL {
                    return DrainResult {
                        directories: Vec::new(),
                        full_scan: false,
                        retry_after: Some(RECOVERY_INTERVAL - elapsed),
                    };
                }
            }
            self.recovery_needed = false;
            self.last_recovery = Some(now);
            self.directories.clear();
            return DrainResult {
                directories: Vec::new(),
                full_scan: true,
                retry_after: None,
            };
        }

        // Collect and sort by path length to eliminate redundant subdirectories
        let mut paths: Vec<PathBuf> = self.directories.drain().collect();
        paths.sort_by_key(|p| p.as_os_str().len());

        let mut roots: Vec<PathBuf> = Vec::new();
        for path in paths {
            let is_sub = roots.iter().any(|r| path.starts_with(r));
            if !is_sub {
                roots.push(path);
            }
        }

        DrainResult {
            directories: roots,
            full_scan: false,
            retry_after: None,
        }
    }
}

/// Debounced change aggregator for file system changes.
pub struct DebouncedWatcherState {
    pub batch: LibraryWatchBatch,
    pub pending_removed: HashSet<PathBuf>,
    pub last_remove_event: Option<Instant>,
    pub last_change_event: Option<Instant>,
    pub remove_debounce: Duration,
    pub change_debounce: Duration,
}

impl Default for DebouncedWatcherState {
    fn default() -> Self {
        Self {
            batch: LibraryWatchBatch::new(),
            pending_removed: HashSet::new(),
            last_remove_event: None,
            last_change_event: None,
            remove_debounce: Duration::from_millis(450),
            change_debounce: Duration::from_secs(30),
        }
    }
}

impl DebouncedWatcherState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_removed<P: AsRef<Path>>(&mut self, path: P) {
        self.pending_removed.insert(path.as_ref().to_path_buf());
        self.last_remove_event = Some(Instant::now());
    }

    pub fn queue_rescan<P: AsRef<Path>>(&mut self, path: Option<P>) {
        if let Some(p) = path {
            let ref_p = p.as_ref();
            let dir = if ref_p.is_dir() {
                ref_p.to_path_buf()
            } else {
                ref_p.parent().unwrap_or(ref_p).to_path_buf()
            };
            self.batch.add_directory(dir);
        } else {
            self.batch.request_recovery();
        }
        self.last_change_event = Some(Instant::now());
    }

    /// Checks if removed files debounce timeout has elapsed and drains them.
    pub fn check_removed(&mut self, now: Instant) -> Option<Vec<PathBuf>> {
        if let Some(last) = self.last_remove_event {
            if now.saturating_duration_since(last) >= self.remove_debounce {
                self.last_remove_event = None;
                if !self.pending_removed.is_empty() {
                    let removed = self.pending_removed.drain().collect();
                    return Some(removed);
                }
            }
        }
        None
    }

    /// Checks if rescan debounce timeout has elapsed and drains changes.
    pub fn check_rescan(&mut self, now: Instant) -> Option<DrainResult> {
        if let Some(last) = self.last_change_event {
            if now.saturating_duration_since(last) >= self.change_debounce {
                self.last_change_event = None;
                return Some(self.batch.drain(now));
            }
        }
        None
    }
}
