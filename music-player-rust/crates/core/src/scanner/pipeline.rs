use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use chrono::{DateTime, Utc};
use crossbeam_channel::{bounded, Sender};

use crate::db::LibraryStore;
use crate::metadata::HybridMetadataEngine;
use crate::models::{ScanProgress, Track};
use super::walker::ScannerControl;

pub struct CandidateFile {
    pub path: PathBuf,
    pub file_size: u64,
    pub modified_utc: DateTime<Utc>,
}

/// Bounded multi-stage scanning pipeline.
/// Stage 1: Directory discovery pushes candidate files into bounded queue.
/// Stage 2: Parallel metadata workers parse tags with Lofty streaming.
/// Stage 3: Dedicated database writer commits batches within transactions.
pub struct ScanPipeline {
    file_tx: Sender<CandidateFile>,
    db_tx: Sender<Track>,
    workers: Vec<thread::JoinHandle<()>>,
    writer_handle: Option<thread::JoinHandle<usize>>,
    control: Arc<ScannerControl>,
}

impl ScanPipeline {
    pub fn new(
        store: LibraryStore,
        artwork_cache: PathBuf,
        control: Arc<ScannerControl>,
        progress_tx: Option<Sender<ScanProgress>>,
        batch_size: usize,
    ) -> Self {
        // Queue capacities provide bounded backpressure to limit RAM usage
        let (file_tx, file_rx) = bounded::<CandidateFile>(128);
        let (db_tx, db_rx) = bounded::<Track>(128);

        // Stage 2: Metadata extraction workers (2 to 4 based on CPU)
        let num_workers = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 4))
            .unwrap_or(2);

        let mut workers = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            let rx = file_rx.clone();
            let tx = db_tx.clone();
            let cache = artwork_cache.clone();
            let ctl = control.clone();

            let handle = thread::Builder::new()
                .name(format!("scanner-meta-worker-{}", i))
                .spawn(move || {
                    let engine = HybridMetadataEngine::new();
                    while let Ok(candidate) = rx.recv() {
                        if ctl.is_cancelled() {
                            break;
                        }
                        let track = Self::process_file(&engine, &candidate, &cache);
                        if tx.send(track).is_err() || ctl.is_cancelled() {
                            break;
                        }
                    }
                })
                .expect("Failed to spawn scanner metadata worker");
            workers.push(handle);
        }

        // Stage 3: Dedicated SQLite Batch Writer
        let ctl_writer = control.clone();
        let writer_handle = thread::Builder::new()
            .name("scanner-db-writer".into())
            .spawn(move || {
                let mut total_indexed = 0usize;
                let mut batch = Vec::with_capacity(batch_size);

                while let Ok(track) = db_rx.recv() {
                    let current_path = track.path.clone();
                    batch.push(track);

                    if batch.len() >= batch_size {
                        if let Ok(count) = store.upsert_tracks(&batch) {
                            total_indexed += count;
                            if let Some(ref p_tx) = progress_tx {
                                let _ = p_tx.send(ScanProgress {
                                    files_found: 0,
                                    directories_visited: 0,
                                    current_path,
                                    indexed_tracks: total_indexed as i32,
                                });
                            }
                        }
                        batch.clear();
                    }

                    if ctl_writer.is_cancelled() {
                        break;
                    }
                }

                if !batch.is_empty() && !ctl_writer.is_cancelled() {
                    if let Ok(count) = store.upsert_tracks(&batch) {
                        total_indexed += count;
                    }
                }
                total_indexed
            })
            .expect("Failed to spawn scanner db writer");

        Self {
            file_tx,
            db_tx,
            workers,
            writer_handle: Some(writer_handle),
            control,
        }
    }

    /// Submits a changed file for metadata parsing and persistence.
    /// Blocks if the bounded queue is full (applying backpressure to the walker).
    pub fn submit(&self, file: CandidateFile) -> Result<(), String> {
        if self.control.is_cancelled() {
            return Err("Scan cancelled".into());
        }
        self.file_tx.send(file).map_err(|e| e.to_string())
    }

    /// Closes input channels, waits for in-flight jobs to complete, and returns total indexed count.
    pub fn finish(mut self) -> usize {
        drop(self.file_tx);
        for w in self.workers {
            let _ = w.join();
        }
        drop(self.db_tx);
        if let Some(handle) = self.writer_handle.take() {
            handle.join().unwrap_or(0)
        } else {
            0
        }
    }

    fn process_file(
        engine: &HybridMetadataEngine,
        candidate: &CandidateFile,
        artwork_cache: &Path,
    ) -> Track {
        let full_path = candidate.path.to_string_lossy().to_string();
        let file_stem = candidate.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Track")
            .to_string();
        let added_utc = Utc::now();

        let tags = engine.read_metadata(&candidate.path, Some(artwork_cache)).unwrap_or_default();

        let (stem_artist, stem_title) = match file_stem.split_once(" - ") {
            Some((a, t)) => (Some(a.trim().to_string()), Some(t.trim().to_string())),
            None => (None, None),
        };

        let title = tags
            .title
            .filter(|t| !t.trim().is_empty())
            .or(stem_title)
            .unwrap_or_else(|| file_stem.clone());
        let artist = tags
            .artist
            .filter(|a| !a.trim().is_empty())
            .or(stem_artist)
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let album_artist = tags
            .album_artist
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| artist.clone());
        let album = tags
            .album
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| "Unknown Album".to_string());

        let has_sidecar = candidate.path.with_extension("lrc").is_file();

        Track {
            path: full_path,
            title,
            artist,
            album,
            album_artist,
            genre: tags.genre.unwrap_or_default(),
            year: tags.year,
            track_number: tags.track_number,
            duration_ms: tags.duration_ms,
            file_size: candidate.file_size,
            modified_utc: candidate.modified_utc,
            added_utc,
            favorite: false,
            rating: 0,
            play_count: 0,
            last_played_utc: None,
            artwork_path: tags.artwork_path,
            has_lyrics: has_sidecar || tags.has_lyrics,
            unavailable: false,
        }
    }
}
