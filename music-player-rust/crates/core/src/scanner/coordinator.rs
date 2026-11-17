//! Library scan coordinator (LibraryScanCoordinator.cs port): owns the active
//! library scan, its pause and cancellation control, and the worker thread
//! running the indexer. Results are polled instead of awaited.

use super::indexer::{IndexResult, LibraryIndexer};
use super::scan::normalize_path;
use super::walker::ScannerControl;
use crate::db::LibraryStore;
use crate::models::ScanProgress;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;

/// Snapshot of the scan coordinator state (`GetStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanStatus {
    pub active: bool,
    pub paused: bool,
    pub cancelling: bool,
}

struct ActiveScan {
    control: Arc<ScannerControl>,
    receiver: Receiver<Result<IndexResult, String>>,
    paused: bool,
    cancellation_requested: bool,
}

/// Owns the active library scan and its pause, resume, and cancellation
/// control. The scan runs on a worker thread because the indexer performs
/// blocking filesystem and database work.
pub struct LibraryScanCoordinator {
    store: LibraryStore,
    artwork_cache: Option<PathBuf>,
    active: Option<ActiveScan>,
    result: Option<Result<IndexResult, String>>,
}

impl LibraryScanCoordinator {
    pub fn new(store: &LibraryStore) -> Self {
        Self {
            store: store.clone(),
            artwork_cache: None,
            active: None,
            result: None,
        }
    }

    /// Overrides the artwork cache directory used during the scan.
    pub fn with_artwork_cache(mut self, cache: PathBuf) -> Self {
        self.artwork_cache = Some(cache);
        self
    }

    /// Drops the active scan if its worker thread has finished and captures
    /// the queued result (the C# `finally` block).
    fn refresh(&mut self) {
        let outcome = self.active.as_mut().map(|active| match active.receiver.try_recv() {
            // The result is the thread's final message: receiving it means the
            // scan is complete even as the thread finishes winding down.
            Ok(result) => Some(result),
            // The worker died without sending a result (panic).
            Err(TryRecvError::Disconnected) => {
                Some(Err("The library scan failed on the worker thread.".to_string()))
            }
            Err(TryRecvError::Empty) => None,
        });
        if let Some(result) = outcome.flatten() {
            self.active = None;
            self.result = Some(result);
        }
    }

    pub fn is_running(&mut self) -> bool {
        self.refresh();
        self.active.is_some()
    }

    pub fn get_status(&mut self) -> ScanStatus {
        self.refresh();
        match &self.active {
            Some(active) => {
                let cancelling = active.cancellation_requested;
                ScanStatus {
                    active: true,
                    paused: active.paused && !cancelling,
                    cancelling,
                }
            }
            None => ScanStatus {
                active: false,
                paused: false,
                cancelling: false,
            },
        }
    }

    /// Starts a scan on the worker thread. Returns `false` when a scan is
    /// already active or there is nothing to scan (StartAsync's null result).
    pub fn start(
        &mut self,
        roots: &[PathBuf],
        ignored_directories: &[PathBuf],
        force_refresh: bool,
        supported_extensions: Option<&HashSet<String>>,
        progress: Option<Sender<ScanProgress>>,
    ) -> bool {
        let scan_roots = distinct_roots(roots.iter().map(|root| normalize_path(root)));
        if scan_roots.is_empty() && !force_refresh {
            return false;
        }
        self.refresh();
        if self.active.is_some() {
            return false;
        }
        let ignored = distinct_roots(
            ignored_directories
                .iter()
                .map(|path| normalize_path(path)),
        );

        let control = Arc::new(ScannerControl::new());
        let thread_control = control.clone();
        let (sender, receiver) = channel::<Result<IndexResult, String>>();
        let extensions = supported_extensions.cloned();
        let artwork_cache = self.artwork_cache.clone();
        let store = self.store.clone();

        let spawned = std::thread::Builder::new()
            .name("library-scan".to_string())
            .spawn(move || {
                let mut indexer = LibraryIndexer::new(&store);
                if let Some(cache) = artwork_cache {
                    indexer = indexer.with_artwork_cache(cache);
                }
                let mut progress_callback = progress
                    .map(|sender| move |progress: &ScanProgress| {
                        let _ = sender.send(progress.clone());
                    });
                let progress_ref = progress_callback
                    .as_mut()
                    .map(|callback| callback as &mut dyn FnMut(&ScanProgress));
                let result = indexer.scan_async(
                    &scan_roots,
                    &ignored,
                    thread_control,
                    progress_ref,
                    force_refresh,
                    extensions.as_ref(),
                );
                let _ = sender.send(result);
            })
            .is_ok();
        if !spawned {
            return false;
        }

        self.active = Some(ActiveScan {
            control,
            receiver,
            paused: false,
            cancellation_requested: false,
        });
        true
    }

    pub fn toggle_pause(&mut self) -> bool {
        self.refresh();
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        if active.cancellation_requested {
            return false;
        }
        if active.paused {
            active.control.resume();
        } else {
            active.control.pause();
        }
        active.paused = !active.paused;
        true
    }

    pub fn cancel(&mut self) -> bool {
        self.refresh();
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        if active.cancellation_requested {
            return false;
        }
        active.cancellation_requested = true;
        active.control.cancel();
        true
    }

    /// Returns the finished scan's result once, if any (`StartAsync`'s
    /// `Task<IndexResult?>` completion, surfaced by polling).
    pub fn poll_result(&mut self) -> Option<Result<IndexResult, String>> {
        self.refresh();
        if self.active.is_none() && self.result.is_some() {
            return self.result.take();
        }
        // A finished-but-unread result: the channel holds it until polled.
        if let Some(active) = &self.active {
            match active.receiver.try_recv() {
                Ok(result) => {
                    self.active = None;
                    self.result = Some(result);
                    return self.result.take();
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {}
            }
        }
        None
    }
}

fn distinct_roots<I: Iterator<Item = PathBuf>>(paths: I) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !out
            .iter()
            .any(|existing| same_path(existing, &path))
        {
            out.push(path);
        }
    }
    out
}

/// Case-insensitive path equality over normalized strings (PathComparer).
fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left)
        .to_string_lossy()
        .to_lowercase()
        == normalize_path(right).to_string_lossy().to_lowercase()
}
