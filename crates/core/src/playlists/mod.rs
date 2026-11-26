//! M3U8 playlist export/import — port of `Playlists.cs`.
//!
//! Export: `#EXTM3U` header + `#EXTINF:<seconds>,<Artist> - <Title>` entries.
//! Import: paths resolved relative to the playlist file's directory; only
//! supported audio files are retained; writes take the per-path file lock.

use crate::lock::FilePathLock;
use crate::models::{Playlist, Track};
use std::fs;
use std::path::Path;

pub fn export_m3u8(playlist: &Playlist, tracks: &[Track], dest: &Path) -> std::io::Result<()> {
    let _lock = FilePathLock::acquire(dest);
    let mut body = String::from("#EXTM3U\n");
    for track in tracks {
        let seconds = (track.duration_ms / 1000) as i64;
        body.push_str(&format!(
            "#EXTINF:{},{} - {}\n{}\n",
            seconds, track.artist, track.title, track.path
        ));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).ok();
    }
    // Atomic staging + replace (mirrors Playlists.WriteM3u8).
    let staging = dest.with_extension(format!(
        "musicplayer-stage-{}.m3u8",
        crate::models::opaque_id_for(&format!(
            "{}-{}",
            playlist.id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ))
    ));
    fs::write(&staging, body)?;
    match fs::rename(&staging, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(&staging, dest)?;
            fs::remove_file(&staging).ok();
            Ok(())
        }
    }
}

/// Parses an `.m3u`/`.m3u8` file. Relative paths resolve against the file's
/// directory; non-audio entries are skipped. Returns the ordered audio paths.
pub fn import_m3u8(source: &Path, is_supported: impl Fn(&str) -> bool) -> std::io::Result<Vec<String>> {
    let _lock = FilePathLock::acquire(source);
    let text = fs::read_to_string(source)?;
    let base_dir = source.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    let mut paths: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("#") {
            continue;
        }
        let resolved = resolve_entry(line, &base_dir);
        if is_supported(&resolved) && !paths.contains(&resolved) {
            paths.push(resolved);
        }
    }
    Ok(paths)
}

fn resolve_entry(entry: &str, base_dir: &Path) -> String {
    let candidate = Path::new(entry);
    if candidate.is_absolute() {
        entry.to_string()
    } else {
        let joined = base_dir.join(candidate);
        joined.to_string_lossy().into_owned()
    }
}

/// Default playlist id generation mirrors the original's opaque ids.
pub fn new_playlist_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("pl-{:x}-{:04x}", nanos, (nanos as u16) ^ 0x5a5a)
}
