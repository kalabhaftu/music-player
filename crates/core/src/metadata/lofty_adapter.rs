use std::path::Path;
use chrono::{DateTime, Utc};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::picture::PictureType;
use lofty::tag::ItemKey;

use super::adapter::MetadataAdapter;
use super::tag_reader::{ExtractedTags, TrackDetails};
use super::artwork::save_artwork_to_cache_in;

/// Metadata reader implemented using the Lofty audio metadata library.
/// Uses seekable file streaming rather than loading entire files into memory.
pub struct LoftyAdapter;

impl LoftyAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LoftyAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataAdapter for LoftyAdapter {
    fn read_metadata(&self, path: &Path, artwork_cache: Option<&Path>) -> Result<ExtractedTags, String> {
        let probe = Probe::open(path).map_err(|e| e.to_string())?;
        let tagged_file = probe.read().map_err(|e| e.to_string())?;

        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());
        let properties = tagged_file.properties();

        let mut tags = ExtractedTags::default();
        tags.duration_ms = properties.duration().as_millis() as u64;

        if let Some(tag) = tag {
            tags.title = tag.title().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            tags.artist = tag.artist().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            tags.album = tag.album().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            tags.genre = tag.genre().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            tags.track_number = tag.track().unwrap_or(0);

            // Year / Date parsing
            tags.year = tag.get_string(ItemKey::Year)
                .or_else(|| tag.get_string(ItemKey::RecordingDate))
                .or_else(|| tag.get_string(ItemKey::ReleaseDate))
                .and_then(|date_str| {
                    let digits: String = date_str.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
                    digits.parse::<u32>().ok()
                })
                .unwrap_or(0);

            // Album artist
            tags.album_artist = tag.get_string(ItemKey::AlbumArtist)
                .or_else(|| tag.get_string(ItemKey::AlbumArtists))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            // Lyrics
            if let Some(lyrics_text) = tag.get_string(ItemKey::Lyrics) {
                let trimmed = lyrics_text.trim();
                if !trimmed.is_empty() {
                    tags.has_lyrics = true;
                    tags.lyrics = Some(trimmed.to_string());
                }
            }

            // Embedded artwork extraction (check primary tag first, then all tags on container)
            if let Some(cache_dir) = artwork_cache {
                let mut front_cover = tag
                    .pictures()
                    .iter()
                    .find(|p| p.pic_type() == PictureType::CoverFront)
                    .or_else(|| tag.pictures().first());

                if front_cover.is_none() {
                    for extra_tag in tagged_file.tags() {
                        let pics = extra_tag.pictures();
                        if let Some(pic) = pics
                            .iter()
                            .find(|p| p.pic_type() == PictureType::CoverFront)
                            .or_else(|| pics.first())
                        {
                            front_cover = Some(pic);
                            break;
                        }
                    }
                }

                if let Some(pic) = front_cover {
                    let data = pic.data();
                    if !data.is_empty() {
                        if let Ok(saved) = save_artwork_to_cache_in(cache_dir, data) {
                            tags.artwork_path = Some(saved.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        // If title is missing, infer from filename
        if tags.title.is_none() {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !stem.trim().is_empty() {
                    tags.title = Some(stem.to_string());
                }
            }
        }

        // Check for sidecar LRC lyrics if embedded lyrics were not present
        if !tags.has_lyrics {
            let lrc_path = path.with_extension("lrc");
            if lrc_path.is_file() {
                tags.has_lyrics = true;
            }
        }

        Ok(tags)
    }

    fn inspect_file(&self, path: &Path) -> Result<TrackDetails, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let file_size = meta.len();
        let modified_utc: DateTime<Utc> = meta.modified()
            .map(DateTime::from)
            .unwrap_or_else(|_| Utc::now());

        let probe = Probe::open(path).map_err(|e| e.to_string())?;
        let tagged_file = probe.read().map_err(|e| e.to_string())?;
        let props = tagged_file.properties();

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let container = match ext.as_str() {
            "mp3" => "taglib/mp3",
            "flac" => "taglib/flac",
            "m4a" | "mp4" | "aac" => "taglib/m4a",
            "ogg" | "oga" => "taglib/ogg",
            "opus" => "taglib/opus",
            "wav" => "taglib/wav",
            "wma" => "taglib/wma",
            "ape" => "taglib/ape",
            "aiff" | "aif" => "taglib/aiff",
            _ => "taglib/unknown",
        }.to_string();

        Ok(TrackDetails {
            path: path.to_string_lossy().to_string(),
            container,
            duration_ms: props.duration().as_millis() as u64,
            bitrate_kbps: props.audio_bitrate().unwrap_or(0),
            sample_rate_hz: props.sample_rate().unwrap_or(0),
            bits_per_sample: props.bit_depth().unwrap_or(0) as u32,
            file_size,
            modified_utc,
        })
    }
}
