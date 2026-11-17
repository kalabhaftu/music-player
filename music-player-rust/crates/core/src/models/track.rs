use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Stable public identity for a track: the first 24 hex characters of the
/// SHA-256 of the full, normalized path (mirrors `LibraryStore.TrackHandleId`).
/// Survives purges and library refilters, unlike positional indexes.
pub fn track_id_for_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let digest = hasher.finalize();
    let mut id = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        id.push_str(&format!("{:02x}", byte));
    }
    id
}

/// Opaque 24-hex identifier used for tag backups and artwork tokens.
pub fn opaque_id_for(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut id = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        id.push_str(&format!("{:02x}", byte));
    }
    id
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackSort {
    #[default]
    Title,
    Artist,
    Album,
    Genre,
    Year,
    Added,
    Duration,
    PlayCount,
    LastPlayed,
    Path,
    Rating,
    TrackNumber,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: u32,
    pub track_number: u32,
    pub duration_ms: u64,
    pub file_size: u64,
    pub modified_utc: DateTime<Utc>,
    pub added_utc: DateTime<Utc>,
    pub favorite: bool,
    pub rating: u8,
    pub play_count: u32,
    pub last_played_utc: Option<DateTime<Utc>>,
    pub artwork_path: Option<String>,
    pub has_lyrics: bool,
    /// Soft availability: the row is retained while the file's location is
    /// unreachable, but it is excluded from visible library queries.
    #[serde(default)]
    pub unavailable: bool,
}

impl Track {
    /// Stable library id for this track (24-hex SHA-256 path prefix).
    pub fn id(&self) -> String {
        track_id_for_path(&self.path)
    }

    /// Placeholder DTO shape used by playlist/queue pages when the underlying
    /// library row is gone but the entry must still render (mirrors the
    /// original `unavailable` TrackDto): title is the file stem and the
    /// artist column reads "Unavailable".
    pub fn unavailable_placeholder(path: &str) -> Track {
        let stem = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        Track {
            path: path.to_string(),
            title: stem,
            artist: "Unavailable".to_string(),
            album: String::new(),
            album_artist: String::new(),
            genre: String::new(),
            year: 0,
            track_number: 0,
            duration_ms: 0,
            file_size: 0,
            modified_utc: Utc::now(),
            added_utc: Utc::now(),
            favorite: false,
            rating: 0,
            play_count: 0,
            last_played_utc: None,
            artwork_path: None,
            has_lyrics: false,
            unavailable: true,
        }
    }
    pub fn duration_display(&self) -> String {
        let total_seconds = self.duration_ms / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        if minutes >= 60 {
            let hours = minutes / 60;
            let rem_minutes = minutes % 60;
            format!("{}:{:02}:{:02}", hours, rem_minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        }
    }

    pub fn year_display(&self) -> String {
        if self.year == 0 {
            String::new()
        } else {
            self.year.to_string()
        }
    }

    pub fn rating_display(&self) -> String {
        if self.rating == 0 {
            "☆".to_string()
        } else {
            "★".repeat(self.rating.min(5) as usize)
        }
    }

    pub fn favorite_glyph(&self) -> &'static str {
        if self.favorite {
            "♥"
        } else {
            "♡"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryGroup {
    pub name: String,
    pub track_count: u32,
    pub artwork_path: Option<String>,
    pub artist: Option<String>,
    pub year: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryStats {
    pub total_tracks: u32,
    pub total_bytes: u64,
}
