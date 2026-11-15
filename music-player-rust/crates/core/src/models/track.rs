use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackSort {
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
}

impl Track {
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
