//! Library traversal port (LibraryScanner.cs): a stack-based walker that
//! honors user exclusions, reparse-point boundaries, built-in application
//! exclusions, and per-root completion tracking.

use super::walker::ScannerControl;
use crate::models::ScanProgress;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Why a subtree was skipped during scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanExcludedPathKind {
    UserIgnored,
    ReparsePoint,
    DefaultExcluded,
}

pub const AUDIO_EXTENSIONS: &[&str] = super::exclusions::AUDIO_EXTENSIONS;
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mkv", "avi", "mov", "wmv", "webm", "mpeg", "mpg", "mts", "m2ts", "3gp", "ogv",
];

pub fn is_video_file(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default();
    VIDEO_EXTENSIONS.contains(&ext.trim_start_matches('.'))
}

pub fn is_supported_audio_file(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    AUDIO_EXTENSIONS.contains(&ext.as_str())
}

const SYSTEM_FOLDERS: &[&str] = &[
    "windows",
    "windows.old",
    "program files",
    "program files (x86)",
    "programdata",
    "windowsapps",
    "recovery",
    "system volume information",
    "$recycle.bin",
    "perflogs",
];

const GENERATED_OR_APPLICATION_FOLDERS: &[&str] = &[
    ".git", ".hg", ".svn", ".vs", ".idea", ".vscode",
    "node_modules", "bin", "obj", "build", "dist", "target", "packages", "testresults",
    "__pycache__", ".pytest_cache", ".mypy_cache", ".tox", ".venv",
    ".cache", ".npm", ".bun", ".cargo", ".rustup", ".pnpm", ".yarn", ".docker", ".conda",
    ".gemini", ".antigravity", ".local", ".config", ".dotnet", ".nuget", ".gradle", ".obsidian",
    ".trash", ".subversion", ".metadata", "temp", "tmp", "caches", "crashdumps",
    "inetcache", "temporary internet files", "microsoftedgebackups", "package cache",
    "games", "gamelibrary", "gameslibrary", "steamlibrary", "steamapps", "xboxgames", "epic games",
];

/// A first-run scan starts with the user's music collection only.
pub fn default_roots() -> Vec<PathBuf> {
    let music = directories_next_user_music();
    match music {
        Some(dir) => vec![dir],
        None => Vec::new(),
    }
}

fn directories_next_user_music() -> Option<PathBuf> {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.audio_dir().map(|p| p.to_path_buf()))
        .or_else(|| {
            directories::UserDirs::new()
                .map(|u| u.home_dir().join("Music"))
        })
}

fn reparse_point(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        (meta.file_attributes() & 0x400) != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

/// Root-relative default exclusions (name-driven; temp folders only when they
/// sit at a drive root, 1:1 with IsDefaultIgnoredDirectory).
pub fn is_default_ignored_directory(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) if !n.trim().is_empty() => n.to_string(),
        _ => return false,
    };
    if name.starts_with("..") {
        return false;
    }
    if name.starts_with('.') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    if lower == "appdata" {
        return true;
    }
    if GENERATED_OR_APPLICATION_FOLDERS.contains(&lower.as_str()) {
        if lower == "temp" || lower == "tmp" {
            // Only exclude temp folders directly under a drive root.
            let parent = path.parent().map(|p| p.to_path_buf());
            let root = path.ancestors().last().map(|p| p.to_path_buf());
            if let (Some(parent), Some(root)) = (parent, root) {
                let parent_norm = parent.to_string_lossy().trim_end_matches(['/', '\\']).to_lowercase();
                let root_norm = root.to_string_lossy().trim_end_matches(['/', '\\']).to_lowercase();
                return parent_norm == root_norm;
            }
            return false;
        }
        return true;
    }
    if !SYSTEM_FOLDERS.contains(&lower.as_str()) {
        return false;
    }
    // System folders only apply when directly under a drive root.
    let parent = path.parent().map(|p| p.to_path_buf());
    if let Some(parent) = parent {
        let root = path.ancestors().last().unwrap_or(Path::new(""));
        let parent_norm = parent.to_string_lossy().trim_end_matches(['/', '\\']).to_lowercase();
        let root_norm = root.to_string_lossy().trim_end_matches(['/', '\\']).to_lowercase();
        return parent_norm == root_norm;
    }
    false
}

pub fn is_default_excluded_directory(path: &Path) -> bool {
    is_default_ignored_directory(path)
}

/// Any folder between the file and its drive root that is a hidden cache,
/// build, or package manager directory.
pub fn is_under_default_excluded_directory(file_path: &str) -> bool {
    let path = Path::new(file_path);
    let mut directory = path.parent();
    while let Some(dir) = directory {
        if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
            if !name.trim().is_empty() {
                if name.starts_with('.') && !name.starts_with("..") {
                    return true;
                }
                let lower = name.to_ascii_lowercase();
                if lower == "node_modules"
                    || lower == "steamapps"
                    || lower == "package cache"
                    || lower == "temporary internet files"
                    || lower == "inetcache"
                    || lower == "__pycache__"
                {
                    return true;
                }
            }
        }
        directory = dir.parent();
    }
    false
}

/// True when `path` or any ancestor is one of the user's ignored directories.
pub fn is_ignored_path(path: &Path, ignored: &[PathBuf]) -> bool {
    if ignored.is_empty() {
        return false;
    }
    let mut current = normalize_path(path);
    loop {
        if ignored.iter().any(|root| paths_equal(root, &current)) {
            return true;
        }
        match current.parent() {
            Some(parent) if parent != current => current = normalize_path(parent),
            _ => break,
        }
    }
    false
}

/// Case-insensitive path equality with separators trimmed (PathsEqual).
pub fn paths_equal(left: &Path, right: &Path) -> bool {
    normalize_path(left)
        .to_string_lossy()
        .to_lowercase()
        == normalize_path(right).to_string_lossy().to_lowercase()
}

pub fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    crate::normalize_windows_path(path)
}

pub fn root_prefix(path: &Path) -> String {
    let mut s = path.to_string_lossy().to_string();
    if !s.ends_with('\\') && !s.ends_with('/') {
        s.push('\\');
    }
    s
}

pub fn is_within_root(path: &str, root: &Path) -> bool {
    let prefix = root_prefix(root);
    path.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// Callback surface for `scan_paths`.
///
/// `progress` carries a separate object-bound lifetime so a long-lived
/// callback can be reborrowed for the duration of the traversal without
/// pinning the short-lived closure references to it.
pub struct ScanCallbacks<'a, 'b> {
    pub progress: Option<&'a mut (dyn FnMut(&ScanProgress) + 'b)>,
    pub directory_visited: Option<&'a mut dyn FnMut(&Path)>,
    pub root_completed: Option<&'a mut dyn FnMut(&Path, bool)>,
    pub root_unavailable: Option<&'a mut dyn FnMut(&Path)>,
    pub path_incomplete: Option<&'a mut dyn FnMut(&Path)>,
    pub path_excluded: Option<&'a mut dyn FnMut(&Path, ScanExcludedPathKind)>,
}

impl<'a, 'b> Default for ScanCallbacks<'a, 'b> {
    fn default() -> Self {
        Self {
            progress: None,
            directory_visited: None,
            root_completed: None,
            root_unavailable: None,
            path_incomplete: None,
            path_excluded: None,
        }
    }
}

fn wait_if_paused(control: &ScannerControl) {
    while control.is_paused() && !control.is_cancelled() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Traverses the supplied roots, invoking `on_audio_file` for each enabled
/// media file. Returns `(files_found, directories_visited)` — 1:1 with
/// `LibraryScanner.ScanAsync` (per-root completion, reparse/user/default
/// exclusion reporting, incomplete-subtree tracking, throttled progress).
pub fn scan_paths<F>(
    roots: &[PathBuf],
    ignored: &[PathBuf],
    control: Arc<ScannerControl>,
    on_audio_file: &mut F,
    callbacks: &mut ScanCallbacks,
    supported_extensions: Option<&HashSet<String>>,
) -> (u32, u32)
where
    F: FnMut(&Path),
{
    let extensions: HashSet<String> = supported_extensions
        .cloned()
        .unwrap_or_else(|| {
            AUDIO_EXTENSIONS
                .iter()
                .map(|e| format!(".{}", e))
                .collect()
        });
    let extensions: HashSet<String> = extensions
        .into_iter()
        .map(|e| {
            let e = e.to_ascii_lowercase();
            if e.starts_with('.') {
                e
            } else {
                format!(".{}", e)
            }
        })
        .collect();

    let mut files_found: u32 = 0;
    let mut directories_visited: u32 = 0;
    let mut next_progress = std::time::Instant::now();

    for supplied_root in roots {
        if control.is_cancelled() {
            break;
        }
        wait_if_paused(&control);
        let root = normalize_path(supplied_root);
        let mut complete_root = true;
        let mut root_unavailable_reported = false;
        let report_root_unavailable = |root: &Path,
                                           complete_root: &mut bool,
                                           reported: &mut bool,
                                           cb: &mut ScanCallbacks| {
            if !*reported {
                *reported = true;
                *complete_root = false;
                if let Some(cb) = cb.root_unavailable.as_mut() {
                    cb(root);
                }
            }
        };

        if !root.exists() {
            if let Some(cb) = callbacks.root_completed.as_mut() {
                cb(&root, false);
            }
            report_root_unavailable(&root, &mut complete_root, &mut root_unavailable_reported, callbacks);
            continue;
        }

        let mut pending: Vec<PathBuf> = vec![root.clone()];
        while let Some(directory) = pending.pop() {
            if control.is_cancelled() {
                break;
            }
            wait_if_paused(&control);

            let meta = match std::fs::metadata(&directory) {
                Ok(meta) => Some(meta),
                Err(_) => None,
            };
            let Some(meta) = meta else {
                if paths_equal(&directory, &root) {
                    report_root_unavailable(&root, &mut complete_root, &mut root_unavailable_reported, callbacks);
                } else if let Some(cb) = callbacks.path_incomplete.as_mut() {
                    cb(&directory);
                }
                continue;
            };
            if reparse_point(&meta) {
                if let Some(cb) = callbacks.path_excluded.as_mut() {
                    cb(&directory, ScanExcludedPathKind::ReparsePoint);
                }
                continue;
            }
            if is_ignored_path(&directory, ignored) {
                if let Some(cb) = callbacks.path_excluded.as_mut() {
                    cb(&directory, ScanExcludedPathKind::UserIgnored);
                }
                continue;
            }
            if !paths_equal(&directory, &root) && is_default_ignored_directory(&directory) {
                if let Some(cb) = callbacks.path_excluded.as_mut() {
                    cb(&directory, ScanExcludedPathKind::DefaultExcluded);
                }
                continue;
            }

            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(_) => {
                    if paths_equal(&directory, &root) {
                        report_root_unavailable(&root, &mut complete_root, &mut root_unavailable_reported, callbacks);
                    } else if let Some(cb) = callbacks.path_incomplete.as_mut() {
                        cb(&directory);
                    }
                    continue;
                }
            };

            let mut complete_directory = true;
            let mut collected: Vec<(PathBuf, bool)> = Vec::new();
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => {
                        complete_directory = false;
                        break;
                    }
                };
                if control.is_cancelled() {
                    break;
                }
                wait_if_paused(&control);
                let entry_path = entry.path();
                let entry_meta = match std::fs::metadata(&entry_path) {
                    Ok(meta) => Some(meta),
                    Err(_) => None,
                };
                let Some(entry_meta) = entry_meta else {
                    complete_directory = false;
                    continue;
                };
                if reparse_point(&entry_meta) {
                    if let Some(cb) = callbacks.path_excluded.as_mut() {
                        cb(&entry_path, ScanExcludedPathKind::ReparsePoint);
                    }
                    continue;
                }
                if entry_meta.is_dir() {
                    let normalized = normalize_path(&entry_path);
                    if is_ignored_path(&normalized, ignored) {
                        if let Some(cb) = callbacks.path_excluded.as_mut() {
                            cb(&normalized, ScanExcludedPathKind::UserIgnored);
                        }
                    } else if is_default_ignored_directory(&normalized) {
                        if let Some(cb) = callbacks.path_excluded.as_mut() {
                            cb(&normalized, ScanExcludedPathKind::DefaultExcluded);
                        }
                    } else {
                        collected.push((normalized, true));
                    }
                } else {
                    let ext = entry_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| format!(".{}", e.to_ascii_lowercase()))
                        .unwrap_or_default();
                    if extensions.contains(&ext) {
                        on_audio_file(&entry_path);
                        files_found += 1;
                        if next_progress.elapsed() >= std::time::Duration::from_millis(250) {
                            next_progress = std::time::Instant::now();
                            if let Some(cb) = callbacks.progress.as_mut() {
                                cb(&ScanProgress {
                                    files_found,
                                    directories_visited,
                                    current_path: entry_path.to_string_lossy().to_string(),
                                    indexed_tracks: -1,
                                });
                            }
                        }
                    }
                }
            }
            for (dir, _) in collected {
                pending.push(dir);
            }

            if !complete_directory {
                if paths_equal(&directory, &root) {
                    report_root_unavailable(&root, &mut complete_root, &mut root_unavailable_reported, callbacks);
                } else if let Some(cb) = callbacks.path_incomplete.as_mut() {
                    cb(&directory);
                }
                continue;
            }
            directories_visited += 1;
            if let Some(cb) = callbacks.directory_visited.as_mut() {
                cb(&directory);
            }
        }

        if control.is_cancelled() {
            // Cancellation leaves root completion unrecorded; the scan session
            // then retains indexed tracks of unfinished roots.
            break;
        }
        if let Some(cb) = callbacks.root_completed.as_mut() {
            cb(&root, complete_root);
        }
    }

    if let Some(cb) = callbacks.progress.as_mut() {
        cb(&ScanProgress {
            files_found,
            directories_visited,
            current_path: String::new(),
            indexed_tracks: -1,
        });
    }

    (files_found, directories_visited)
}
