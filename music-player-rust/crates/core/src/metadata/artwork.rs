use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static ARTWORK_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

    if target.exists() {
        return Ok(target);
    }

    let sequence = ARTWORK_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = dir.join(format!(".{}.{}.{}.tmp", hash, std::process::id(), sequence));
    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(data)?;
        fs::rename(&temporary, &target)
    })();

    match write_result {
        Ok(()) => Ok(target),
        Err(_error) if target.exists() => {
            let _ = fs::remove_file(&temporary);
            Ok(target)
        }
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

/// Deletes artwork cache files that no track references any more
/// (mirrors `LibraryStore.PruneUnreferencedArtwork`). Files younger than the
/// grace period are skipped: the startup artwork repair writes cache files on a
/// worker thread and only then commits their database references, so a scan
/// completing in between must not delete them.
pub fn prune_unreferenced_artwork(cache_dir: &std::path::Path, store: &crate::db::LibraryStore) -> usize {
    const GRACE: std::time::Duration = std::time::Duration::from_secs(120);
    let now = std::time::SystemTime::now();
    let refs = store.artwork_refs().unwrap_or_default();
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
