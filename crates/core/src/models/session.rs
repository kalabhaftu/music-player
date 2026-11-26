use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackSession {
    pub track_path: Option<String>,
    pub position_ms: u64,
    pub queue: Vec<String>,
    pub shuffle: bool,
    pub repeat_mode: String,
    pub repeat_a_ms: Option<u64>,
    pub repeat_b_ms: Option<u64>,
    pub queue_index: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TagEdit {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub lyrics: Option<String>,
    pub artwork_path: Option<String>,
    pub custom_fields: Option<HashMap<String, String>>,
    pub additional_fields: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagBackup {
    pub original_path: String,
    pub backup_path: String,
    pub created_utc: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataEditEntry {
    pub id: i64,
    pub track_path: String,
    pub field_name: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub snapshot_json: String,
    pub backup_file_path: Option<String>,
    pub created_utc: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    pub files_found: u32,
    pub directories_visited: u32,
    pub current_path: String,
    pub indexed_tracks: i32,
}
