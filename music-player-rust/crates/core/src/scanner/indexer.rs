use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use chrono::{DateTime, Utc};
use crate::db::LibraryStore;
use crate::metadata::read_tags;
use crate::models::{ScanProgress, Track};
use super::exclusions::is_supported_audio;
use super::walker::{scan_folder, ScannerControl};

#[derive(Debug, Clone, Default)]
pub struct IndexResult {
    pub indexed: usize,
    pub skipped: usize,
    pub files_found: usize,
    pub directories_visited: usize,
}

pub struct LibraryIndexer<'a> {
    store: &'a LibraryStore,
    batch_size: usize,
}

impl<'a> LibraryIndexer<'a> {
    pub fn new(store: &'a LibraryStore) -> Self {
        Self {
            store,
            batch_size: 64,
        }
    }

    /// Indexes all audio files in the specified root folder.
    pub fn index_folder<F>(
        &self,
        root: &Path,
        control: Arc<ScannerControl>,
        mut on_progress: F,
    ) -> Result<IndexResult, String>
    where
        F: FnMut(&ScanProgress),
    {
        let paths = scan_folder(root, control.clone(), None);
        let mut pending: Vec<Track> = Vec::with_capacity(self.batch_size);
        let mut result = IndexResult::default();
        let mut last_flush = Instant::now();

        let flush = |pending: &mut Vec<Track>, result: &mut IndexResult, last_flush: &mut Instant, on_progress: &mut F| {
            if pending.is_empty() {
                return;
            }
            if let Ok(count) = self.store.upsert_tracks(pending) {
                result.indexed += count;
            }
            let last_path = pending.last().map(|t| t.path.clone()).unwrap_or_default();
            pending.clear();
            *last_flush = Instant::now();

            on_progress(&ScanProgress {
                files_found: result.files_found as u32,
                directories_visited: result.directories_visited as u32,
                current_path: last_path,
                indexed_tracks: result.indexed as i32,
            });
        };

        for path in paths {
            if control.is_cancelled() {
                break;
            }

            if path.is_dir() {
                result.directories_visited += 1;
                if result.indexed == 0 || last_flush.elapsed().as_millis() >= 1000 {
                    flush(&mut pending, &mut result, &mut last_flush, &mut on_progress);
                }
                continue;
            }

            if !is_supported_audio(&path) {
                result.skipped += 1;
                continue;
            }

            result.files_found += 1;

            if let Ok(track) = read_track_with_metadata(&path) {
                pending.push(track);
                if pending.len() >= self.batch_size {
                    flush(&mut pending, &mut result, &mut last_flush, &mut on_progress);
                }
            } else {
                result.skipped += 1;
            }
        }

        // Flush remaining tracks
        flush(&mut pending, &mut result, &mut last_flush, &mut on_progress);

        Ok(result)
    }
}

fn read_track_with_metadata(path: &Path) -> Result<Track, std::io::Error> {
    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();
    let modified_utc: DateTime<Utc> = metadata.modified()?.into();
    let added_utc = Utc::now();

    let full_path = path.to_string_lossy().to_string();
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown Track")
        .to_string();

    // Check for sidecar lyrics (.lrc)
    let lrc_path = path.with_extension("lrc");
    let has_lyrics = lrc_path.exists();

    // Extract tags (ID3v2, FLAC, M4A) and embedded artwork
    let tags = read_tags(path);

    // Title fallback
    let title = tags.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        if let Some((_, t)) = file_stem.split_once(" - ") {
            t.trim().to_string()
        } else {
            file_stem.clone()
        }
    });

    // Artist fallback
    let artist = tags.artist.filter(|a| !a.trim().is_empty()).unwrap_or_else(|| {
        if let Some((a, _)) = file_stem.split_once(" - ") {
            a.trim().to_string()
        } else {
            "Unknown Artist".to_string()
        }
    });

    let album_artist = tags
        .album_artist
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| artist.clone());

    let album = tags
        .album
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| "Unknown Album".to_string());

    let genre = tags.genre.unwrap_or_default();

    Ok(Track {
        path: full_path,
        title,
        artist,
        album,
        album_artist,
        genre,
        year: tags.year,
        track_number: tags.track_number,
        duration_ms: tags.duration_ms,
        file_size,
        modified_utc,
        added_utc,
        favorite: false,
        rating: 0,
        play_count: 0,
        last_played_utc: None,
        artwork_path: tags.artwork_path,
        has_lyrics,
    })
}
