use super::exclusions::{is_excluded_directory, is_supported_audio};
use crate::models::ScanProgress;
use crossbeam_channel::Sender;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use walkdir::WalkDir;

pub struct ScannerControl {
    pub is_cancelled: Arc<AtomicBool>,
    pub is_paused: Arc<AtomicBool>,
}

impl ScannerControl {
    pub fn new() -> Self {
        Self {
            is_cancelled: Arc::new(AtomicBool::new(false)),
            is_paused: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::Relaxed)
    }

    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn resume(&self) {
        self.is_paused.store(false, Ordering::SeqCst);
    }
}

impl Default for ScannerControl {
    fn default() -> Self {
        Self::new()
    }
}

pub fn scan_folder<P: AsRef<Path>>(
    root: P,
    control: Arc<ScannerControl>,
    progress_tx: Option<Sender<ScanProgress>>,
) -> Vec<PathBuf> {
    let mut discovered = Vec::new();
    let mut files_found: u32 = 0;
    let mut dirs_visited: u32 = 0;

    let walker = WalkDir::new(root).follow_links(false).into_iter();

    for entry_res in walker.filter_entry(|entry| {
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                return !is_excluded_directory(name);
            }
        }
        true
    }) {
        if control.is_cancelled.load(Ordering::SeqCst) {
            break;
        }

        while control.is_paused.load(Ordering::SeqCst) {
            if control.is_cancelled.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        if let Ok(entry) = entry_res {
            let path = entry.path();
            if entry.file_type().is_dir() {
                dirs_visited += 1;
            } else if entry.file_type().is_file() && is_supported_audio(path) {
                files_found += 1;
                discovered.push(path.to_path_buf());

                if let Some(ref tx) = progress_tx {
                    if files_found % 25 == 0 {
                        let _ = tx.send(ScanProgress {
                            files_found,
                            directories_visited: dirs_visited,
                            current_path: path.to_string_lossy().to_string(),
                            indexed_tracks: -1,
                        });
                    }
                }
            }
        }
    }

    if let Some(ref tx) = progress_tx {
        let _ = tx.send(ScanProgress {
            files_found,
            directories_visited: dirs_visited,
            current_path: String::new(),
            indexed_tracks: -1,
        });
    }

    discovered
}
