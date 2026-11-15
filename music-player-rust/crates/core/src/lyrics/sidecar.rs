use std::fs;
use std::path::{Path, PathBuf};

pub fn sidecar_path<P: AsRef<Path>>(track_path: P) -> PathBuf {
    let mut p = track_path.as_ref().to_path_buf();
    p.set_extension("lrc");
    p
}

pub fn has_sidecar<P: AsRef<Path>>(track_path: P) -> bool {
    sidecar_path(track_path).exists()
}

pub fn read_sidecar<P: AsRef<Path>>(track_path: P) -> Option<String> {
    let p = sidecar_path(track_path);
    if p.exists() {
        fs::read_to_string(p).ok()
    } else {
        None
    }
}

pub fn write_sidecar<P: AsRef<Path>>(track_path: P, content: &str) -> std::io::Result<()> {
    let p = sidecar_path(track_path);
    fs::write(p, content)
}

pub fn remove_sidecar<P: AsRef<Path>>(track_path: P) -> std::io::Result<()> {
    let p = sidecar_path(track_path);
    if p.exists() {
        fs::remove_file(p)?;
    }
    Ok(())
}
