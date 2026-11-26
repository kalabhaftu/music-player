//! Library file watcher (LibraryFileWatcher.cs port): per-root `notify` watches
//! feed the shared debounced state. The app's clock calls `pump` instead of the
//! C# timers, and the exclusion/ancestor walk mirrors `IsExcludedPath`.

use super::scan::{is_default_excluded_directory, is_within_root, normalize_path, paths_equal};
use super::watcher::DebouncedWatcherState;
use crate::scanner::AUDIO_EXTENSIONS;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use notify::event::{ModifyKind, RenameMode};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

/// Watches configured music folders for changes so the index follows the
/// filesystem. Events land in a `DebouncedWatcherState`; `pump` drains them.
pub struct LibraryFileWatcher {
    state: DebouncedWatcherState,
    /// One receiver + watcher per root so events can be attributed to the root
    /// they were raised under (FileSystemWatcher per root in the original).
    watches: Vec<RootWatch>,
    ignored_directories: Vec<PathBuf>,
    supported_extensions: HashSet<String>,
    retry_at: Option<Instant>,
    wake_notifier: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
}

struct RootWatch {
    root: PathBuf,
    receiver: Receiver<Result<Event, notify::Error>>,
    // Kept alive so the watch stays registered (dropping unregisters it).
    _watcher: notify::RecommendedWatcher,
}

impl Default for LibraryFileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl LibraryFileWatcher {
    pub fn new() -> Self {
        Self {
            state: DebouncedWatcherState::new(),
            watches: Vec::new(),
            ignored_directories: Vec::new(),
            supported_extensions: AUDIO_EXTENSIONS
                .iter()
                .map(|ext| format!(".{}", ext))
                .collect(),
            retry_at: None,
            wake_notifier: None,
        }
    }

    /// Registers a wake notifier that is called whenever new filesystem events arrive.
    pub fn with_wake_notifier(mut self, notifier: impl Fn() + Send + Sync + 'static) -> Self {
        self.wake_notifier = Some(Arc::new(notifier));
        self
    }

    /// Replaces the watched roots and exclusion state. Existing watches are
    /// dropped first, mirroring the C# `Dispose` + rebuild sequence.
    pub fn set_roots(
        &mut self,
        roots: &[PathBuf],
        ignored_directories: &[PathBuf],
        supported_extensions: Option<&HashSet<String>>,
    ) {
        self.ignored_directories = distinct_paths(
            ignored_directories
                .iter()
                .map(|path| normalize_path(path)),
        );
        self.supported_extensions = supported_extensions
            .cloned()
            .unwrap_or_else(|| AUDIO_EXTENSIONS.iter().map(|ext| format!(".{}", ext)).collect());

        self.watches.clear();
        let mut seen: Vec<PathBuf> = Vec::new();
        for root in roots {
            let full = normalize_path(root);
            if seen.iter().any(|existing| paths_equal(existing, &full)) {
                continue;
            }
            if !full.is_dir() {
                continue;
            }
            seen.push(full.clone());
            let (sender, receiver) = channel();
            let notifier = self.wake_notifier.clone();
            let mut watcher = match notify::recommended_watcher(move |res| {
                let _ = sender.send(res);
                if let Some(ref notify) = notifier {
                    notify();
                }
            }) {
                Ok(watcher) => watcher,
                // Startup and manual scans still reconcile the folder.
                Err(_) => continue,
            };
            if watcher.watch(&full, RecursiveMode::Recursive).is_err() {
                continue;
            }
            self.watches.push(RootWatch {
                root: full,
                receiver,
                _watcher: watcher,
            });
        }
    }

    /// Drives the debounced flushes: drains OS events, then emits removed
    /// paths (450ms debounce) and directory rescans (30s debounce / 5-minute
    /// recovery retry) through the supplied callbacks.
    pub fn pump(
        &mut self,
        now: Instant,
        on_removed: Option<&mut dyn FnMut(&[PathBuf])>,
        on_rescan: Option<&mut dyn FnMut(&[PathBuf], bool)>,
    ) {
        // 1. Collect whatever the OS delivered since the last pump.
        let mut events: Vec<(PathBuf, Result<Event, notify::Error>)> = Vec::new();
        for watch in &mut self.watches {
            loop {
                match watch.receiver.try_recv() {
                    Ok(message) => events.push((watch.root.clone(), message)),
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                }
            }
        }
        for (root, message) in events {
            match message {
                Ok(event) => self.handle_event(&root, &event),
                // Watcher error: a library folder change could not be observed.
                Err(_) => self.state.queue_rescan(None::<&Path>),
            }
        }

        // 2. Flush removed paths after the remove debounce elapses.
        if let Some(removed) = self.state.check_removed(now) {
            if let Some(callback) = on_removed {
                callback(&removed);
            }
        }

        // 3. Flush directory rescans after the change debounce, honouring the
        // recovery retry delay returned by the batch drain.
        let mut drained = self.state.check_rescan(now);
        if drained.is_none() && self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
            drained = Some(self.state.batch.drain(now));
        }
        if let Some(result) = drained {
            if let Some(delay) = result.retry_after {
                self.retry_at = Some(now + delay);
            } else {
                self.retry_at = None;
                if let Some(callback) = on_rescan {
                    if result.full_scan || !result.directories.is_empty() {
                        callback(&result.directories, result.full_scan);
                    }
                }
            }
        }
    }

    fn handle_event(&mut self, root: &Path, event: &Event) {
        match &event.kind {
            EventKind::Remove(_) => {
                for path in &event.paths {
                    if !self.is_excluded_path(path, root) {
                        self.state.queue_removed(path);
                    }
                }
            }
            EventKind::Create(_) => {
                for path in &event.paths {
                    // Moving or copying a populated directory does not emit a
                    // Created event for every file in its subtree.
                    if !self.is_excluded_path(path, root)
                        && (self.is_supported_file(path) || path.is_dir())
                    {
                        self.state.queue_rescan(Some(path));
                    }
                }
            }
            EventKind::Modify(kind) => match kind {
                ModifyKind::Name(RenameMode::From) => {
                    for path in &event.paths {
                        if !self.is_excluded_path(path, root) {
                            self.state.queue_removed(path);
                        }
                    }
                }
                ModifyKind::Name(RenameMode::Both) => {
                    if let Some(old_path) = event.paths.first() {
                        if !self.is_excluded_path(old_path, root) {
                            self.state.queue_removed(old_path);
                        }
                    }
                    if let Some(new_path) = event.paths.get(1) {
                        if !self.is_excluded_path(new_path, root)
                            && (self.is_supported_file(new_path) || new_path.is_dir())
                        {
                            self.state.queue_rescan(Some(new_path));
                        }
                    }
                }
                ModifyKind::Name(_) => {
                    // RenameMode::To / Any: treat the reported path as the new name.
                    for path in &event.paths {
                        if !self.is_excluded_path(path, root)
                            && (self.is_supported_file(path) || path.is_dir())
                        {
                            self.state.queue_rescan(Some(path));
                        }
                    }
                }
                _ => {
                    // Data / Metadata / Other changes on an observed file.
                    for path in &event.paths {
                        if !self.is_excluded_path(path, root) && self.is_supported_file(path) {
                            self.state.queue_rescan(Some(path));
                        }
                    }
                }
            },
            EventKind::Access(_) | EventKind::Other | EventKind::Any => {}
        }
    }

    fn is_supported_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| self.supported_extensions.contains(&format!(".{}", ext.to_ascii_lowercase())))
            .unwrap_or(false)
    }

    fn is_ignored(&self, path: &Path) -> bool {
        self.ignored_directories
            .iter()
            .any(|ignored| paths_equal(ignored, path))
    }

    /// Ancestor walk that ignores events inside excluded or default-excluded
    /// directories (IsExcludedPath port).
    fn is_excluded_path(&self, path: &Path, watch_root: &Path) -> bool {
        if path.as_os_str().is_empty() {
            return true;
        }
        let full = normalize_path(path);
        let mut current = if full.is_dir() {
            full.clone()
        } else {
            match full.parent() {
                Some(parent) if parent != full => normalize_path(parent),
                _ => return false,
            }
        };
        let root = normalize_path(watch_root);
        loop {
            if self.is_ignored(&current) {
                return true;
            }
            let is_root = paths_equal(&current, &root);
            if !is_root
                && is_within_root(current.to_string_lossy().as_ref(), &root)
                && is_default_excluded_directory(&current)
            {
                return true;
            }
            if is_root {
                break;
            }
            match current.parent() {
                Some(parent) if parent != current => current = normalize_path(parent),
                _ => break,
            }
        }
        false
    }
}

fn distinct_paths<I: Iterator<Item = PathBuf>>(paths: I) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !out.iter().any(|existing| paths_equal(existing, &path)) {
            out.push(path);
        }
    }
    out
}
