use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

static ARTWORK_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Tracks active artwork extraction and repair tasks.
/// Pruning must NEVER run while artwork operations are in flight.
pub static ARTWORK_OPERATIONS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// RAII guard to track in-flight artwork extraction and repair operations.
pub struct ArtworkOperationGuard;

impl ArtworkOperationGuard {
    pub fn new() -> Self {
        ARTWORK_OPERATIONS_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Default for ArtworkOperationGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ArtworkOperationGuard {
    fn drop(&mut self) {
        ARTWORK_OPERATIONS_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn artwork_cache_dir() -> PathBuf {
    let base = directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("MusicPlayer").join("Artwork"))
        .unwrap_or_else(|| PathBuf::from("artwork_cache"));
    fs::create_dir_all(&base).ok();
    base
}

pub fn hash_artwork(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<String>()
}

pub fn save_artwork_to_cache(data: &[u8]) -> std::io::Result<PathBuf> {
    save_artwork_to_cache_in(&artwork_cache_dir(), data)
}

pub fn save_artwork_to_cache_in(dir: &std::path::Path, data: &[u8]) -> std::io::Result<PathBuf> {
    if data.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Artwork data cannot be empty",
        ));
    }

    let hash = hash_artwork(data);
    let ext = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if data.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        "img"
    };
    let filename = format!("{}.{}", hash, ext);
    let target = dir.join(filename);

    if target.exists() && fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(target);
    }

    let sequence = ARTWORK_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = dir.join(format!(".{}.{}.{}.tmp", hash, std::process::id(), sequence));
    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);

        match fs::rename(&temporary, &target) {
            Ok(()) => Ok(()),
            Err(e) => {
                if target.exists() && fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false) {
                    let _ = fs::remove_file(&temporary);
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    })();

    match write_result {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

pub fn resolve_cached_artwork(hash: &str) -> Option<PathBuf> {
    let dir = artwork_cache_dir();
    for extension in ["jpg", "jpeg", "png", "webp", "img"] {
        let path = dir.join(format!("{}.{}", hash, extension));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Windows path keys are case-insensitive (the C# prune query uses
/// `COLLATE NOCASE`); normalizing slashes and casing prevents mismatches
/// from deleting artwork that is still referenced.
fn path_key(path: &str) -> String {
    let s = path.replace('/', "\\");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// Deletes artwork cache files that no track references any more.
/// Never prunes while extraction/repair tasks are in flight.
/// Never prunes if the database returns an empty reference list.
/// Grace period is 30 minutes to prevent race conditions during scanning.
pub fn prune_unreferenced_artwork(cache_dir: &std::path::Path, store: &crate::db::LibraryStore) -> usize {
    if ARTWORK_OPERATIONS_IN_FLIGHT.load(Ordering::SeqCst) > 0 {
        return 0;
    }

    let refs = store.artwork_refs().unwrap_or_default();
    if refs.is_empty() {
        return 0;
    }

    const GRACE: std::time::Duration = std::time::Duration::from_secs(1800);
    let now = std::time::SystemTime::now();
    let referenced: std::collections::HashSet<String> = refs
        .iter()
        .map(|path| path_key(path))
        .collect();
    let referenced_names: std::collections::HashSet<String> = refs
        .iter()
        .filter_map(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    if cfg!(windows) {
                        n.to_lowercase()
                    } else {
                        n.to_string()
                    }
                })
        })
        .collect();
    let mut removed = 0;
    if let Ok(entries) = fs::read_dir(cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".jpg") && !name.ends_with(".png") && !name.ends_with(".jpeg")
                && !name.ends_with(".webp") && !name.ends_with(".img")
            {
                continue;
            }
            let full = path.to_string_lossy().into_owned();
            let name_key = if cfg!(windows) {
                name.to_lowercase()
            } else {
                name.to_string()
            };
            let recently_written = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map(|modified| {
                    now.duration_since(modified)
                        .map(|age| age < GRACE)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if recently_written {
                continue;
            }
            if !referenced.contains(&path_key(&full))
                && !referenced_names.contains(&name_key)
                && fs::remove_file(&path).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}
