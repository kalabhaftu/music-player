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
    let hash = hash_artwork(data);
    let ext = if data.starts_with(b"\x89PNG") { "png" } else { "jpg" };
    let filename = format!("{}.{}", hash, ext);
    let target = artwork_cache_dir().join(filename);

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
