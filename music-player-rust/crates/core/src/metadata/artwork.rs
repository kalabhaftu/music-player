use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

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
    let ext = if data.starts_with(b"\x89PNG") { "png" } else { "jpg" };
    let filename = format!("{}.{}", hash, ext);
    let target = dir.join(filename);

    if !target.exists() {
        fs::write(&target, data)?;
    }

    Ok(target)
}

pub fn resolve_cached_artwork(hash: &str) -> Option<PathBuf> {
    let dir = artwork_cache_dir();
    let jpg = dir.join(format!("{}.jpg", hash));
    if jpg.exists() {
        return Some(jpg);
    }
    let png = dir.join(format!("{}.png", hash));
    if png.exists() {
        return Some(png);
    }
    None
}

/// Windows path keys are case-insensitive (the C# prune query uses
/// `COLLATE NOCASE`); normalizing both sides prevents a case-only mismatch
/// from deleting artwork that is still referenced.
fn path_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

/// Deletes artwork cache files that no track references any more
/// (mirrors `LibraryStore.PruneUnreferencedArtwork`).
pub fn prune_unreferenced_artwork(cache_dir: &std::path::Path, store: &crate::db::LibraryStore) -> usize {
    let referenced: std::collections::HashSet<String> = store
        .artwork_refs()
        .unwrap_or_default()
        .into_iter()
        .map(|path| path_key(&path))
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
            if !referenced.contains(&path_key(&full)) && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}
