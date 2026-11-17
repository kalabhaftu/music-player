//! `TagEditor` port: backup history plus per-container tag writers.
//!
//! The C# version leans on TagLib#'s unified `Tag` abstraction; here every
//! container carries its own writer that mirrors the read-side mappings in
//! `tag_reader.rs` so a save → read round trip is stable:
//!
//! - MP3/AAC: surgical ID3v2 head replacement that preserves unknown and
//!   binary frames verbatim.
//! - WAV/AIFF: the `ID3` chunk body is replaced inside the container.
//! - DSF: the ID3 payload is appended after the audio stream and the metadata
//!   pointer/total size in the DSD chunk are updated.
//! - FLAC: metadata blocks are rebuilt (VORBIS_COMMENT / PICTURE).
//! - APE (ape/wv/mpc): the APEv2 tag range is replaced, keeping any trailing
//!   ID3v1 block.
//! - ASF (wma): the header objects are rebuilt; stream data stays raw.
//! - MP4 (m4a/m4b/mp4): the `moov/udta/meta/ilst` chain is rebuilt with
//!   size-chain awareness; audio atoms are untouched.
//! - OGG/OGA/OPUS: tag writing is not implemented; a save succeeds by staging
//!   an untouched copy so the tags stay unchanged.
//! - DFF/TTA: saving is not supported and fails with "unsupported format",
//!   leaving the file bytes intact (TagLib parity).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use sha2::{Digest, Sha256};

use crate::lock::FilePathLock;
use crate::metadata::tag_reader::{
    decode_id3_strings, decode_ilst_name, is_common_field, mp4_data_payloads,
    mp4_freeform_key, normalize_v22_frame_id, parse_vorbis_comments, split_id3_string,
    ADDITIONAL_STANDARD_FIELDS,
};
use crate::models::{TagBackup, TagEdit};

const BACKUP_RETENTION_PER_TRACK: usize = 5;

// ======================================================================
// Container dispatch
// ======================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveContainer {
    /// ID3v2 at the head of an MPEG audio stream (.mp3).
    Id3Head,
    /// ID3v2 at the head of an ADTS stream (.aac).
    Id3HeadAac,
    /// RIFF/WAVE container with an `ID3 ` chunk.
    Wav,
    /// FORM/AIFF container with an `ID3 ` chunk.
    Aiff,
    /// DSD stream with a metadata pointer to an ID3v2 tail tag.
    Dsf,
    /// FLAC metadata blocks.
    Flac,
    /// ASF header objects (.wma).
    Asf,
    /// APEv2 item collections (.ape/.wv/.mpc).
    Ape,
    /// QuickTime atoms (.m4a/.m4b/.mp4).
    Mp4,
    /// Copy the file unchanged (tag writing not implemented for .ogg family).
    CopyOnly,
    /// Saving this container is not supported.
    FailUnsupported,
}

impl SaveContainer {
    fn resolve(extension: &str) -> Self {
        match extension {
            "mp3" => SaveContainer::Id3Head,
            "aac" => SaveContainer::Id3HeadAac,
            "wav" | "wave" => SaveContainer::Wav,
            "aif" | "aiff" => SaveContainer::Aiff,
            "dsf" => SaveContainer::Dsf,
            "flac" => SaveContainer::Flac,
            "wma" => SaveContainer::Asf,
            "ape" | "wv" | "mpc" => SaveContainer::Ape,
            "m4a" | "m4b" | "mp4" => SaveContainer::Mp4,
            "ogg" | "oga" | "opus" => SaveContainer::CopyOnly,
            // .dff and .tta plus anything unrecognized: TagLib reports
            // "unsupported format" for these extensions.
            _ => SaveContainer::FailUnsupported,
        }
    }

    /// `TagEditor.FormatFor`: which custom-field storage a file format uses.
    fn custom_format(extension: &str) -> Option<&'static str> {
        match extension {
            "flac" | "ogg" | "oga" | "opus" => Some("xiph"),
            "wma" => Some("asf"),
            "ape" | "wv" | "mpc" => Some("ape"),
            "mp3" | "wav" | "wave" | "aif" | "aiff" | "aac" | "tta" | "dsf" | "dff" => Some("id3"),
            _ => None,
        }
    }
}

// ======================================================================
// TagEditor
// ======================================================================

pub struct TagEditor {
    backup_directory: PathBuf,
}

impl TagEditor {
    pub fn new<P: AsRef<Path>>(backup_directory: P) -> Self {
        Self {
            backup_directory: backup_directory.as_ref().to_path_buf(),
        }
    }

    pub fn backup_directory(&self) -> &Path {
        &self.backup_directory
    }

    /// Computes the 10-character hex suffix of the SHA-256 hash of the canonical full file path.
    pub fn backup_suffix<P: AsRef<Path>>(path: P) -> String {
        let full_path = fs::canonicalize(path.as_ref())
            .unwrap_or_else(|_| path.as_ref().to_path_buf())
            .to_string_lossy()
            .to_string();
        let mut hasher = Sha256::new();
        hasher.update(full_path.as_bytes());
        let hash = hasher.finalize();
        let hex_str: String = hash.iter().map(|b| format!("{:02X}", b)).collect();
        hex_str[..10].to_string()
    }

    /// Lists up to 5 of the most recent backups for a given track file, ordered newest first.
    pub fn list_backups<P: AsRef<Path>>(&self, path: P) -> Vec<TagBackup> {
        let original = path.as_ref();
        let suffix = Self::backup_suffix(original);
        let orig_str = original.to_string_lossy().to_string();

        if !self.backup_directory.exists() {
            return Vec::new();
        }

        let mut backups = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.backup_directory) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
                    if file_name.contains(&format!("-{}-", suffix)) && file_name.ends_with(".bak") {
                        let created = Self::parse_backup_stamp(&p);
                        backups.push(TagBackup {
                            original_path: orig_str.clone(),
                            backup_path: p.to_string_lossy().to_string(),
                            created_utc: created,
                        });
                    }
                }
            }
        }

        backups.sort_by(|a, b| b.created_utc.cmp(&a.created_utc));
        backups.truncate(BACKUP_RETENTION_PER_TRACK);
        backups
    }

    /// Saves updated tags to an audio file: stage a copy, back up the
    /// original, rewrite the staged copy, then replace the original.
    pub fn save<P: AsRef<Path>>(
        &self,
        path: P,
        edit: &TagEdit,
    ) -> Result<TagBackup, Box<dyn std::error::Error>> {
        let original = full_path(path.as_ref());
        let _path_mutex = FilePathLock::acquire(&original);
        fs::create_dir_all(&self.backup_directory)?;

        let extension = file_extension(&original);
        let container = SaveContainer::resolve(&extension);

        // TagLib's AudioTags.Open fails for unsupported containers before any
        // field validation runs, so mirror that order here.
        if container == SaveContainer::FailUnsupported {
            return Err("unsupported format".into());
        }

        let plan = build_edit_plan(edit)?;
        let custom = validated_custom_fields(
            edit.custom_fields.as_ref(),
            SaveContainer::custom_format(&extension),
        )?;
        let mut plan = plan;
        plan.custom = custom;

        let backup_path = self.new_backup_path(&original);
        let backup_stage = backup_stage_path(&backup_path);
        let staged = temporary_path(&original, "stage");

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            fs::copy(&original, &staged)?;
            fs::copy(&original, &backup_stage)?;
            let data = fs::read(&staged)?;
            let rewritten = rewrite_staged(container, &plan, data)?;
            if let Some(bytes) = rewritten {
                fs::write(&staged, bytes)?;
            }
            fs::rename(&backup_stage, &backup_path)?;
            fs::rename(&staged, &original)?;
            Ok(())
        })();

        // The original is untouched whenever the rewrite or rename failed.
        let _ = fs::remove_file(&staged);
        let _ = fs::remove_file(&backup_stage);
        result?;
        self.prune_backup_files();
        Ok(TagBackup {
            original_path: original.to_string_lossy().to_string(),
            backup_path: backup_path.to_string_lossy().to_string(),
            created_utc: Self::parse_backup_stamp(&backup_path),
        })
    }

    /// Restores a backup snapshot, first preserving the current file as a new
    /// undo snapshot (RestoreCore port).
    pub fn restore(&self, backup: &TagBackup) -> Result<TagBackup, Box<dyn std::error::Error>> {
        let original = full_path(Path::new(&backup.original_path));
        let source = full_path(Path::new(&backup.backup_path));
        if !self.is_managed_backup(&original, &source) {
            return Err("The selected backup does not belong to this track's Music Player backup history.".into());
        }
        if !source.exists() {
            return Err("The saved tag backup is missing.".into());
        }

        let _path_mutex = FilePathLock::acquire(&original);
        fs::create_dir_all(&self.backup_directory)?;
        let undo_backup = self.new_backup_path(&original);
        let backup_stage = backup_stage_path(&undo_backup);
        let restore_stage = temporary_path(&original, "restore");

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            fs::copy(&original, &backup_stage)?;
            fs::copy(&source, &restore_stage)?;
            fs::rename(&backup_stage, &undo_backup)?;
            fs::rename(&restore_stage, &original)?;
            Ok(())
        })();

        let _ = fs::remove_file(&restore_stage);
        let _ = fs::remove_file(&backup_stage);
        result?;
        self.prune_backup_files();
        Ok(TagBackup {
            original_path: original.to_string_lossy().to_string(),
            backup_path: undo_backup.to_string_lossy().to_string(),
            created_utc: Self::parse_backup_stamp(&undo_backup),
        })
    }

    /// True when the candidate lives in this editor's backup directory and
    /// carries the original's path suffix (IsManagedBackup port).
    fn is_managed_backup(&self, original: &Path, candidate: &Path) -> bool {
        let candidate_dir = candidate.parent().map(|p| full_path(p));
        let Some(directory) = candidate_dir else {
            return false;
        };
        let same_dir = directory
            .to_string_lossy()
            .eq_ignore_ascii_case(self.backup_directory.to_string_lossy().as_ref());
        if !same_dir {
            return false;
        }
        let Some(name) = candidate.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        name.contains(&format!("-{}-", Self::backup_suffix(original)))
    }

    fn new_backup_path(&self, original: &Path) -> PathBuf {
        let stamp = backup_stamp_now();
        let suffix = Self::backup_suffix(original);
        self.backup_directory
            .join(format!("{}-{}-{}.bak", stamp, suffix, unique_token()))
    }

    fn parse_backup_stamp(path: &Path) -> DateTime<Utc> {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return file_modified_utc(path);
        };
        parse_backup_stamp_text(name).unwrap_or_else(|| file_modified_utc(path))
    }

    /// Deletes the oldest backups beyond the per-track retention window
    /// (PruneBackupFiles port).
    fn prune_backup_files(&self) {
        let Ok(entries) = fs::read_dir(&self.backup_directory) else {
            return;
        };
        let mut groups: BTreeMap<String, Vec<(String, String, String, PathBuf)>> = BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".bak") {
                continue;
            }
            let parts: Vec<&str> = name.split('-').collect();
            if parts.len() != 4 || !parts[3].to_ascii_lowercase().ends_with(".bak") {
                continue;
            }
            groups.entry(parts[2].to_string()).or_default().push((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[3].to_string(),
                path.clone(),
            ));
        }
        for mut group in groups.into_values() {
            group.sort_by(|a, b| (&b.0, &b.1, &b.2).cmp(&(&a.0, &a.1, &a.2)));
            for (_, _, _, path) in group.into_iter().skip(BACKUP_RETENTION_PER_TRACK) {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn backup_stage_path(backup_path: &Path) -> PathBuf {
    let name = backup_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut stage = backup_path.to_path_buf();
    stage.set_file_name(format!("{}.partial", name));
    stage
}

/// `.musicplayer-{purpose}-{token}{extension}` in the file's own directory.
fn temporary_path(original: &Path, purpose: &str) -> PathBuf {
    let extension = original
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    original.with_file_name(format!(
        ".musicplayer-{}-{}{}",
        purpose,
        unique_token(),
        extension
    ))
}

fn backup_stamp_now() -> String {
    let now = Utc::now();
    format!(
        "{}-{}{:03}",
        now.format("%Y%m%d-%H%M%S"),
        "",
        now.timestamp_subsec_millis()
    )
}

/// Parses the leading `yyyyMMdd-HHmmssfff` stamp of a backup file name.
fn parse_backup_stamp_text(name: &str) -> Option<DateTime<Utc>> {
    if name.len() < 18 {
        return None;
    }
    let text = &name[..18];
    let date = NaiveDate::parse_from_str(&text[..8], "%Y%m%d").ok()?;
    let hour: u32 = text[9..11].parse().ok()?;
    let minute: u32 = text[11..13].parse().ok()?;
    let second: u32 = text[13..15].parse().ok()?;
    let millis: u32 = text[15..18].parse().ok()?;
    if hour > 23 || minute > 59 || second > 59 || millis > 999 {
        return None;
    }
    Some(
        NaiveDateTime::new(date, chrono::NaiveTime::from_hms_milli_opt(hour, minute, second, millis)?)
            .and_utc(),
    )
}

fn file_modified_utc(path: &Path) -> DateTime<Utc> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| DateTime::<Utc>::from(t))
        .unwrap_or_else(|_| Utc::now())
}

static UNIQUE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Replaces the C# `Guid.NewGuid()` token in file names: a hex string without
/// separators, unique enough for same-second operations.
fn unique_token() -> String {
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(Utc::now().timestamp_nanos_opt().unwrap_or_default().to_le_bytes());
    hasher.update(counter.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(backup_stamp_now().as_bytes());
    let hash = hasher.finalize();
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..32].to_string()
}

/// Absolute, normalized path (Path.GetFullPath without symlink resolution).
fn full_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let text = absolute.to_string_lossy();
    let trimmed = text.trim_end_matches(['/', '\\']);
    let trimmed = if trimmed.len() == 2 && trimmed.as_bytes()[1] == b':' {
        format!("{}\\", trimmed)
    } else {
        trimmed.to_string()
    };
    PathBuf::from(trimmed)
}

fn file_extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

// ======================================================================
// Validation (ApplyAdditionalFields / ApplyCustomFields ports)
// ======================================================================

/// The resolved, validated edit payload consumed by the container writers.
#[derive(Debug, Default)]
struct EditPlan {
    title: Option<String>,
    artists: Option<Vec<String>>,
    album: Option<String>,
    album_artists: Option<Vec<String>>,
    genres: Option<Vec<String>>,
    year: Option<u32>,
    track: Option<u32>,
    /// Raw lyrics text; empty means "clear".
    lyrics: Option<String>,
    /// Decoded artwork bytes with its MIME type.
    artwork: Option<(Vec<u8>, String)>,
    // Additional fields, keyed by catalog key (upper case).
    track_count: Option<u32>,
    disc: Option<u32>,
    disc_count: Option<u32>,
    bpm: Option<u32>,
    date_tagged: Option<String>,
    strings: BTreeMap<String, String>,
    arrays: BTreeMap<String, Vec<String>>,
    doubles: BTreeMap<String, f64>,
    /// Validated custom fields (key casing preserved from the input).
    custom: BTreeMap<String, String>,
}

fn split_values(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_uint_field(value: &str, field: &str) -> Result<u32, String> {
    if value.trim().is_empty() {
        return Ok(0);
    }
    if !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("'{}' must be a whole number.", field));
    }
    value
        .parse::<u32>()
        .map_err(|_| format!("'{}' must be a whole number.", field))
}

fn parse_double_field(value: &str, field: &str) -> Result<f64, String> {
    if value.trim().is_empty() {
        return Ok(f64::NAN);
    }
    match value.parse::<f64>() {
        Ok(parsed) if parsed.is_finite() => Ok(parsed),
        _ => Err(format!("'{}' must be a finite number.", field)),
    }
}

/// Accepts the ISO 8601 shapes `DateTime.TryParse(..., RoundtripKind)` does.
fn parse_iso_datetime(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
        "%Y/%m/%d",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, format) {
            return Some(naive.and_utc());
        }
        if let Ok(date) = NaiveDate::parse_from_str(value, format) {
            return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    None
}

/// Normalizes a validated date to the ID3 TDRC timestamp shape.
fn format_iso_datetime(value: &str) -> String {
    match parse_iso_datetime(value) {
        Some(parsed) => {
            if parsed.timestamp_subsec_millis() > 0 {
                parsed.format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
            } else {
                parsed.format("%Y-%m-%dT%H:%M:%S").to_string()
            }
        }
        None => value.to_string(),
    }
}

fn is_uint_field(key: &str) -> bool {
    matches!(key, "TRACK_COUNT" | "DISC" | "DISC_COUNT" | "BPM")
}

fn is_double_field(key: &str) -> bool {
    key.starts_with("REPLAYGAIN_")
}

fn is_array_field(key: &str) -> bool {
    matches!(
        key,
        "ARTIST_SORT" | "ARTIST_ROLE" | "ALBUM_ARTIST_SORT" | "COMPOSERS" | "COMPOSER_SORT"
    )
}

/// Validates and resolves the standard + additional fields (the first half of
/// the C# SaveCore body plus ApplyAdditionalFields).
fn build_edit_plan(edit: &TagEdit) -> Result<EditPlan, String> {
    let mut plan = EditPlan {
        title: edit.title.clone().map(|v| v.trim().to_string()),
        artists: edit.artist.as_ref().map(|v| split_values(v)),
        album: edit.album.clone().map(|v| v.trim().to_string()),
        album_artists: edit.album_artist.as_ref().map(|v| split_values(v)),
        genres: edit.genre.as_ref().map(|v| split_values(v)),
        year: edit.year,
        track: edit.track_number,
        lyrics: edit.lyrics.clone(),
        ..Default::default()
    };

    if let Some(path) = edit.artwork_path.as_ref() {
        let artwork_path = Path::new(path);
        let bytes = fs::read(artwork_path)
            .map_err(|_| format!("Artwork file not found: {}", path))?;
        let mime = sniff_image_mime(&bytes).ok_or_else(|| {
            format!("Artwork file not found: {}", path)
        })?;
        plan.artwork = Some((bytes, mime.to_string()));
    }

    let Some(fields) = edit.additional_fields.as_ref() else {
        return Ok(plan);
    };

    for (raw_key, value) in fields {
        let key = raw_key.trim().to_ascii_uppercase();
        if !ADDITIONAL_STANDARD_FIELDS.iter().any(|(k, _)| *k == key) {
            return Err(format!("Unknown standard tag field '{}'.", raw_key));
        }
        if is_uint_field(&key) {
            let parsed = parse_uint_field(value, raw_key)?;
            match key.as_str() {
                "TRACK_COUNT" => plan.track_count = Some(parsed),
                "DISC" => plan.disc = Some(parsed),
                "DISC_COUNT" => plan.disc_count = Some(parsed),
                "BPM" => plan.bpm = Some(parsed),
                _ => unreachable!(),
            }
        } else if is_double_field(&key) {
            plan.doubles.insert(key, parse_double_field(value, raw_key)?);
        } else if key == "DATE_TAGGED" {
            if value.trim().is_empty() {
                plan.date_tagged = Some(String::new());
            } else if parse_iso_datetime(value).is_some() {
                plan.date_tagged = Some(format_iso_datetime(value));
            } else {
                return Err("'Date tagged' must use an ISO 8601 date and time.".to_string());
            }
        } else if is_array_field(&key) {
            plan.arrays.insert(key, split_values(value));
        } else {
            plan.strings.insert(key, value.trim().to_string());
        }
    }
    Ok(plan)
}

fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        return Some("image/jpeg");
    }
    if bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some("image/png");
    }
    if bytes.len() >= 12 && (&bytes[0..4] == b"RIFF" || &bytes[4..8] == b"WAVE") {
        return None;
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"GIF8" {
        return Some("image/gif");
    }
    None
}

/// Validates the custom field set (ApplyCustomFields' loop) and returns the
/// desired mapping with the caller's key casing.
fn validated_custom_fields(
    fields: Option<&std::collections::HashMap<String, String>>,
    format: Option<&'static str>,
) -> Result<BTreeMap<String, String>, String> {
    let Some(fields) = fields else {
        return Ok(BTreeMap::new());
    };
    let Some(format) = format else {
        return Err("Custom tags are not supported for this file format.".to_string());
    };
    let mut desired: BTreeMap<String, String> = BTreeMap::new();
    for (raw_key, value) in fields {
        let key = raw_key.trim();
        if key.is_empty() || key.contains('=') || key.contains('\n') || key.contains('\r') {
            return Err("Custom tag names cannot be empty or contain '=', or line breaks.".to_string());
        }
        if format == "xiph"
            && !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("Xiph field names can contain only ASCII letters, digits, and underscores.".to_string());
        }
        if format != "id3" && is_common_field(key) {
            return Err(format!("'{}' is already edited in a standard tag field.", key));
        }
        if format == "id3" {
            if let Some(frame_id) = key
                .strip_prefix("ID3:")
                .or_else(|| key.strip_prefix("id3:"))
            {
                let upper = frame_id.to_ascii_uppercase();
                if upper.len() != 4
                    || !upper.chars().all(|c| c.is_ascii_alphanumeric())
                    || is_common_id3_frame(&upper)
                    || upper == "TXXX"
                {
                    return Err(
                        "Use ID3: followed by a non-standard four-character text frame identifier.".to_string(),
                    );
                }
            }
        }
        desired.insert(key.to_string(), value.clone());
    }
    Ok(desired)
}

fn is_common_id3_frame(id: &str) -> bool {
    const COMMON_ID3_FRAMES: &[&str] = &[
        "TIT2", "TPE1", "TALB", "TPE2", "TCON", "TYER", "TDRC", "TRCK", "TSOT", "TIT3", "TIT1",
        "TSOP", "TMCL", "TIPL", "TCOM", "TSOC", "TSO2", "TSOA", "TPOS", "TBPM", "TPE3", "TCOP",
        "TKEY", "TPE4", "TPUB", "TSRC",
    ];
    COMMON_ID3_FRAMES.iter().any(|f| f.eq_ignore_ascii_case(id))
}

// ======================================================================
// Dispatch into container writers
// ======================================================================

/// Rewrites `data` in place for `container`, returning the new bytes.
/// `Ok(None)` means "leave the staged copy untouched" (OGG family).
fn rewrite_staged(
    container: SaveContainer,
    plan: &EditPlan,
    data: Vec<u8>,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    match container {
        SaveContainer::Id3Head => Ok(Some(rewrite_id3_head(&data, plan, false)?)),
        SaveContainer::Id3HeadAac => Ok(Some(rewrite_id3_head(&data, plan, true)?)),
        SaveContainer::Wav => Ok(Some(rewrite_riff_chunk(&data, plan, b"RIFF", false)?)),
        SaveContainer::Aiff => Ok(Some(rewrite_riff_chunk(&data, plan, b"FORM", true)?)),
        SaveContainer::Dsf => Ok(Some(rewrite_dsf(&data, plan)?)),
        SaveContainer::Flac => Ok(Some(rewrite_flac(&data, plan)?)),
        SaveContainer::Asf => Ok(Some(rewrite_asf(&data, plan)?)),
        SaveContainer::Ape => Ok(Some(rewrite_ape_tag(&data, plan)?)),
        SaveContainer::Mp4 => Ok(Some(rewrite_mp4(&data, plan)?)),
        // Tag writing is not implemented for the OGG family: the save
        // succeeds by staging an untouched copy (tags stay unchanged).
        SaveContainer::CopyOnly => Ok(None),
        // Mirrors TagLib's UnsupportedFormatException for these extensions.
        SaveContainer::FailUnsupported => Err("unsupported format".into()),
    }
}

// ======================================================================
// ID3v2 writer (MP3 / AAC / WAV / AIFF / DSF)
// ======================================================================

/// A raw ID3v2 frame: normalized 4-character id + untouched content bytes.
#[derive(Debug, Clone)]
struct Id3Frame {
    id: String,
    flags: [u8; 2],
    content: Vec<u8>,
}

/// Parses an ID3v2 tag into raw frames, preserving every frame byte (the
/// writer re-renders unknown and binary frames verbatim).
fn parse_id3_tag(data: &[u8]) -> Option<(Vec<Id3Frame>, u8, usize)> {
    if data.len() < 10 || &data[0..3] != b"ID3" {
        return None;
    }
    let version = data[3];
    let tag_size = synchsafe_u32(&data[6..10]) as usize;
    let total = 10 + tag_size;
    if total > data.len() {
        return None;
    }
    let mut payload = &data[10..total];
    // Extended header (flag bit 6): v4 stores a synchsafe length, v3 a plain u32.
    if data[5] & 0x40 != 0 && payload.len() >= 4 {
        let ext_len = if version == 4 {
            synchsafe_u32(&payload[0..4]) as usize
        } else {
            u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize
        };
        payload = &payload[ext_len.min(payload.len())..];
    }

    let v22 = version == 2;
    let header_len = if v22 { 6 } else { 10 };
    let mut frames = Vec::new();
    let mut cursor = 0usize;
    while cursor + header_len <= payload.len() {
        if payload[cursor] == 0 {
            break; // padding
        }
        let id_raw = String::from_utf8_lossy(
            &payload[cursor..cursor + if v22 { 3 } else { 4 }],
        )
        .to_string();
        let (size, flags): (usize, [u8; 2]) = if v22 {
            let s = ((payload[cursor + 3] as usize) << 16)
                | ((payload[cursor + 4] as usize) << 8)
                | (payload[cursor + 5] as usize);
            (s, [0, 0])
        } else if version == 4 {
            (
                synchsafe_u32(&payload[cursor + 4..cursor + 8]) as usize,
                [payload[cursor + 8], payload[cursor + 9]],
            )
        } else {
            (
                u32::from_be_bytes([
                    payload[cursor + 4],
                    payload[cursor + 5],
                    payload[cursor + 6],
                    payload[cursor + 7],
                ]) as usize,
                [payload[cursor + 8], payload[cursor + 9]],
            )
        };
        cursor += header_len;
        if size == 0 || cursor + size > payload.len() {
            break;
        }
        let id = normalize_v22_frame_id(&id_raw);
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            break; // implausible frame id: treat the rest as audio
        }
        frames.push(Id3Frame {
            id,
            flags,
            content: payload[cursor..cursor + size].to_vec(),
        });
        cursor += size;
    }
    Some((frames, version, total))
}

fn synchsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32 & 0x7F) << 21)
        | ((bytes[1] as u32 & 0x7F) << 14)
        | ((bytes[2] as u32 & 0x7F) << 7)
        | (bytes[3] as u32 & 0x7F)
}

fn synchsafe_bytes(value: usize) -> [u8; 4] {
    let v = value as u32;
    [
        ((v >> 21) & 0x7F) as u8,
        ((v >> 14) & 0x7F) as u8,
        ((v >> 7) & 0x7F) as u8,
        (v & 0x7F) as u8,
    ]
}

/// UTF-8 text-frame content: encoding byte 3 + NUL-separated values.
fn id3_text_content(values: &[String]) -> Vec<u8> {
    let mut content = vec![3u8];
    for value in values {
        content.extend_from_slice(value.as_bytes());
        content.push(0);
    }
    content
}

/// Splits a text frame's content into its values (drops empties).
fn id3_text_values(content: &[u8]) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    let encoding = content[0];
    decode_id3_strings(&content[1..], encoding)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .collect()
}

/// Rebuilds a COMM frame: encoding + original language + empty description +
/// the comment text (mirrors the reader's [lang, desc, text] split).
fn rebuild_comm_frame(existing: Option<&Id3Frame>, text: &str) -> Option<Id3Frame> {
    let language: &[u8] = match existing {
        Some(frame) if frame.content.len() >= 4 => &frame.content[1..4],
        _ => b"XXX",
    };
    let mut content = vec![3u8];
    content.extend_from_slice(language);
    content.push(0); // empty description
    content.extend_from_slice(text.as_bytes());
    content.push(0);
    Some(Id3Frame {
        id: "COMM".to_string(),
        flags: [0, 0],
        content,
    })
}

/// Rebuilds a USLT frame: encoding + language + empty description + text.
fn rebuild_uslt_frame(existing: Option<&Id3Frame>, text: &str) -> Option<Id3Frame> {
    let language: &[u8] = match existing {
        Some(frame) if frame.content.len() >= 4 => &frame.content[1..4],
        _ => b"eng",
    };
    let mut content = vec![3u8];
    content.extend_from_slice(language);
    content.push(0);
    content.extend_from_slice(text.as_bytes());
    content.push(0);
    Some(Id3Frame {
        id: "USLT".to_string(),
        flags: [0, 0],
        content,
    })
}

/// Rebuilds an APIC frame with the new artwork: encoding + mime + picture
/// type (3 = front cover) + empty description + image bytes.
fn build_apic_frame(image: &[u8], mime: &str) -> Id3Frame {
    let mut content = vec![3u8];
    content.extend_from_slice(mime.as_bytes());
    content.push(0);
    content.push(0x03);
    content.push(0); // empty description
    content.extend_from_slice(image);
    Id3Frame {
        id: "APIC".to_string(),
        flags: [0, 0],
        content,
    }
}

/// Builds a TXXX frame: encoding + description + text values.
fn build_txxx_frame(description: &str, values: &[String]) -> Id3Frame {
    let mut content = vec![3u8];
    content.extend_from_slice(description.as_bytes());
    content.push(0);
    for value in values {
        content.extend_from_slice(value.as_bytes());
        content.push(0);
    }
    Id3Frame {
        id: "TXXX".to_string(),
        flags: [0, 0],
        content,
    }
}

/// TXXX frame description (None when the frame is not parseable user text).
fn txxx_description(frame: &Id3Frame) -> Option<String> {
    let encoding = *frame.content.first()?;
    let (description, _) = split_id3_string(&frame.content[1..], encoding)?;
    Some(description)
}

fn remove_frames<F: Fn(&Id3Frame) -> bool>(frames: &mut Vec<Id3Frame>, predicate: F) {
    frames.retain(|frame| !predicate(frame));
}

fn set_text_frame(frames: &mut Vec<Id3Frame>, id: &str, values: &[String]) {
    let upper = id.to_ascii_uppercase();
    remove_frames(frames, |frame| frame.id.eq_ignore_ascii_case(&upper));
    if !values.is_empty() {
        frames.push(Id3Frame {
            id: upper,
            flags: [0, 0],
            content: id3_text_content(values),
        });
    }
}

fn set_txxx_frame(frames: &mut Vec<Id3Frame>, description: &str, values: &[String]) {
    remove_frames(frames, |frame| {
        frame.id.eq_ignore_ascii_case("TXXX")
            && txxx_description(frame)
                .map(|d| d.eq_ignore_ascii_case(description))
                .unwrap_or(false)
    });
    if !values.is_empty() {
        frames.push(build_txxx_frame(description, values));
    }
}

/// The TRCK/TPOS composite read from the current frames ("n" or "n/total").
fn composite_parts(frames: &[Id3Frame], id: &str) -> (Option<u32>, Option<u32>) {
    let value = frames
        .iter()
        .find(|frame| frame.id.eq_ignore_ascii_case(id))
        .and_then(|frame| id3_text_values(&frame.content).first().cloned());
    let Some(value) = value else {
        return (None, None);
    };
    let mut parts = value.split('/');
    let number = parts
        .next()
        .and_then(|p| p.trim().parse::<u32>().ok())
        .filter(|n| *n > 0);
    let total = parts
        .next()
        .and_then(|p| p.trim().parse::<u32>().ok())
        .filter(|n| *n > 0);
    (number, total)
}

/// Renders a "n/total" composite; None removes the frame.
fn render_composite(number: Option<u32>, total: Option<u32>) -> Option<Vec<String>> {
    match (number, total) {
        (Some(n), Some(t)) => Some(vec![format!("{}/{}", n, t)]),
        (Some(n), None) => Some(vec![n.to_string()]),
        (None, Some(t)) => Some(vec![format!("/{}", t)]),
        (None, None) => None,
    }
}

/// Replaces an ID3v2 tag at the head of `data` (MP3/AAC). The rest of the
/// file is preserved verbatim; a trailing ID3v1 tag is never touched.
fn rewrite_id3_head(data: &[u8], plan: &EditPlan, is_aac: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let _ = is_aac;
    if data.len() >= 3 && &data[0..3] == b"ID3" {
        if parse_id3_tag(data).is_none() {
            return Err("Could not parse the existing ID3v2 tag.".into());
        }
    }
    let (existing_frames, _, tag_len) = parse_id3_tag(data).unwrap_or_default();
    let audio = if tag_len > 0 { &data[tag_len..] } else { data };
    let frames = apply_id3_edits(existing_frames, plan);
    if frames.is_empty() {
        return Ok(audio.to_vec());
    }
    let tag = render_id3v2_tag(&frames)?;
    let mut out = Vec::with_capacity(tag.len() + audio.len());
    out.extend_from_slice(&tag);
    out.extend_from_slice(audio);
    Ok(out)
}

/// Builds the new frame list from the existing raw frames plus the edit plan.
fn apply_id3_edits(mut frames: Vec<Id3Frame>, plan: &EditPlan) -> Vec<Id3Frame> {
    // Standard fields: only assigned values are written; untouched frames stay raw.
    if let Some(title) = plan.title.as_ref() {
        set_text_frame(&mut frames, "TIT2", std::slice::from_ref(title));
    }
    if let Some(artists) = plan.artists.as_ref() {
        set_text_frame(&mut frames, "TPE1", artists);
    }
    if let Some(album) = plan.album.as_ref() {
        set_text_frame(&mut frames, "TALB", std::slice::from_ref(album));
    }
    if let Some(album_artists) = plan.album_artists.as_ref() {
        set_text_frame(&mut frames, "TPE2", album_artists);
    }
    if let Some(genres) = plan.genres.as_ref() {
        set_text_frame(&mut frames, "TCON", genres);
    }

    // Year / Date tagged: TDRC wins, TYER stays alongside for v2.3 readers.
    if plan.year.is_some() {
        remove_frames(&mut frames, |frame| {
            matches!(frame.id.to_ascii_uppercase().as_str(), "TYER" | "TDAT" | "TDRC")
        });
        if let Some(year) = plan.year.filter(|y| *y > 0) {
            set_text_frame(&mut frames, "TDRC", &[year.to_string()]);
            set_text_frame(&mut frames, "TYER", &[year.to_string()]);
        }
    }
    if let Some(date_tagged) = plan.date_tagged.as_ref() {
        set_text_frame(&mut frames, "TDRC", &[]);
        if !date_tagged.trim().is_empty() {
            set_text_frame(&mut frames, "TDRC", std::slice::from_ref(date_tagged));
        }
    }

    // Track / disc composites ("n/total").
    if plan.track.is_some() || plan.track_count.is_some() {
        let (number, total) = composite_parts(&frames, "TRCK");
        let number = plan.track.filter(|t| *t > 0).or(number);
        let total = plan
            .track_count
            .filter(|t| *t > 0)
            .or(total.filter(|_| plan.track_count.is_none() || plan.track_count.unwrap() > 0));
        set_text_frame(&mut frames, "TRCK", &render_composite(number, total).unwrap_or_default());
    }
    if plan.disc.is_some() || plan.disc_count.is_some() {
        let (number, total) = composite_parts(&frames, "TPOS");
        let number = plan.disc.filter(|d| *d > 0).or(number);
        let total = plan
            .disc_count
            .filter(|d| *d > 0)
            .or(total.filter(|_| plan.disc_count.is_none() || plan.disc_count.unwrap() > 0));
        set_text_frame(&mut frames, "TPOS", &render_composite(number, total).unwrap_or_default());
    }

    // Lyrics: USLT carries the text; SYLT is removed so the plain edit wins.
    if let Some(lyrics) = plan.lyrics.as_ref() {
        remove_frames(&mut frames, |frame| {
            matches!(frame.id.to_ascii_uppercase().as_str(), "USLT" | "SYLT")
        });
        if !lyrics.trim().is_empty() {
            let existing = frames.iter().find(|frame| frame.id.eq_ignore_ascii_case("USLT"));
            let _ = existing; // language falls back to "eng" once removed
            frames.push(rebuild_uslt_frame(None, lyrics).unwrap());
        }
    }

    // Artwork: replace every picture frame with the new one.
    if let Some((image, mime)) = plan.artwork.as_ref() {
        remove_frames(&mut frames, |frame| {
            matches!(frame.id.to_ascii_uppercase().as_str(), "APIC" | "PIC")
        });
        frames.push(build_apic_frame(image, mime));
    }

    // Additional standard fields: text frames, TXXX descriptors, composites.
    for (key, frame_id) in ID3_FIELD_FRAMES {
        if let Some(values) = id3_field_values(plan, key) {
            set_text_frame(&mut frames, frame_id, &values);
        }
    }
    for (key, description) in ID3_FIELD_TXXX {
        if let Some(values) = id3_field_values(plan, key) {
            set_txxx_frame(&mut frames, description, &values);
        }
    }
    if let Some(values) = id3_field_values(plan, "COMMENT") {
        let existing = frames
            .iter()
            .find(|frame| frame.id.eq_ignore_ascii_case("COMM"));
        let rebuilt = rebuild_comm_frame(existing, &values.join("; "));
        if let Some(rebuilt) = rebuilt {
            remove_frames(&mut frames, |frame| frame.id.eq_ignore_ascii_case("COMM"));
            frames.push(rebuilt);
        }
    }

    apply_id3_custom_fields(&mut frames, plan);
    frames
}

/// Resolves an additional field's rendered value list (None = leave untouched;
/// empty list = clear the field).
fn id3_field_values(plan: &EditPlan, key: &str) -> Option<Vec<String>> {
    let Some(key) = ADDITIONAL_STANDARD_FIELDS.iter().find(|(k, _)| *k == key) else {
        return None;
    };
    let key = key.0;
    if let Some(value) = plan.strings.get(key) {
        return Some(if value.is_empty() {
            Vec::new()
        } else {
            vec![value.clone()]
        });
    }
    if let Some(values) = plan.arrays.get(key) {
        return Some(values.clone());
    }
    if let Some(number) = plan_uint(plan, key) {
        return Some(if number == 0 {
            Vec::new()
        } else {
            vec![number.to_string()]
        });
    }
    if let Some(value) = plan.doubles.get(key) {
        return Some(if value.is_nan() {
            Vec::new()
        } else {
            vec![format_double(*value)]
        });
    }
    None
}

/// The additional-field storage tables (canonical first, aliases follow).
const ID3_FIELD_FRAMES: &[(&str, &str)] = &[
    ("TITLE_SORT", "TSOT"),
    ("SUBTITLE", "TIT3"),
    ("DESCRIPTION", "TIT3"),
    ("ARTIST_SORT", "TSOP"),
    ("ARTIST_ROLE", "TMCL"),
    ("ALBUM_ARTIST_SORT", "TSO2"),
    ("COMPOSERS", "TCOM"),
    ("COMPOSER_SORT", "TSOC"),
    ("ALBUM_SORT", "TSOA"),
    ("GROUPING", "TIT1"),
    ("BPM", "TBPM"),
    ("CONDUCTOR", "TPE3"),
    ("COPYRIGHT", "TCOP"),
    ("INITIAL_KEY", "TKEY"),
    ("REMIXED_BY", "TPE4"),
    ("PUBLISHER", "TPUB"),
    ("ISRC", "TSRC"),
];

const ID3_FIELD_TXXX: &[(&str, &str)] = &[
    ("MUSICBRAINZ_ARTIST_ID", "MusicBrainz Artist Id"),
    ("MUSICBRAINZ_RELEASE_GROUP_ID", "MusicBrainz Release Group Id"),
    ("MUSICBRAINZ_RELEASE_ID", "MusicBrainz Album Id"),
    ("MUSICBRAINZ_RELEASE_ARTIST_ID", "MusicBrainz Album Artist Id"),
    ("MUSICBRAINZ_TRACK_ID", "MusicBrainz Track Id"),
    ("MUSICBRAINZ_DISC_ID", "MusicBrainz Disc Id"),
    ("MUSICIP_ID", "MusicIp Id"),
    ("AMAZON_ID", "Amazon Id"),
    ("MUSICBRAINZ_RELEASE_STATUS", "MusicBrainz Album Status"),
    ("MUSICBRAINZ_RELEASE_TYPE", "MusicBrainz Album Type"),
    ("MUSICBRAINZ_RELEASE_COUNTRY", "MusicBrainz Album Release Country"),
    ("REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_GAIN"),
    ("REPLAYGAIN_TRACK_PEAK", "REPLAYGAIN_TRACK_PEAK"),
    ("REPLAYGAIN_ALBUM_GAIN", "REPLAYGAIN_ALBUM_GAIN"),
    ("REPLAYGAIN_ALBUM_PEAK", "REPLAYGAIN_ALBUM_PEAK"),
];

/// Clears and rewrites the custom fields (ApplyCustomFields port for ID3v2).
fn apply_id3_custom_fields(frames: &mut Vec<Id3Frame>, plan: &EditPlan) {
    // Existing keys: TXXX descriptions plus non-common text frames.
    let mut existing: Vec<String> = Vec::new();
    for frame in frames.iter() {
        if frame.id.eq_ignore_ascii_case("TXXX") {
            if let Some(description) = txxx_description(frame) {
                if !description.trim().is_empty() {
                    existing.push(description);
                }
            }
        } else if !is_common_id3_frame(&frame.id) {
            existing.push(format!("ID3:{}", frame.id));
        }
    }

    let desired = &plan.custom;
    for key in &existing {
        if desired.keys().any(|d| d.eq_ignore_ascii_case(key)) {
            continue;
        }
        if let Some(frame_id) = key.strip_prefix("ID3:") {
            set_text_frame(frames, frame_id, &[]);
        } else {
            set_txxx_frame(frames, key, &[]);
        }
    }
    for (key, value) in desired {
        let values = split_values(value);
        if let Some(frame_id) = key.strip_prefix("ID3:").or_else(|| key.strip_prefix("id3:")) {
            set_text_frame(frames, &frame_id.to_ascii_uppercase(), &values);
        } else {
            set_txxx_frame(frames, key, &values);
        }
    }
}

/// Renders a complete ID3v2.3 tag (UTF-8 encoding byte 3, per the existing
/// draft writer's convention).
fn render_id3v2_tag(frames: &[Id3Frame]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut payload = Vec::new();
    for frame in frames {
        payload.extend_from_slice(frame.id.as_bytes());
        payload.extend_from_slice(&(frame.content.len() as u32).to_be_bytes());
        payload.extend_from_slice(&frame.flags);
        payload.extend_from_slice(&frame.content);
    }
    if payload.len() > 0x0FFF_FFFF {
        return Err("The edited ID3v2 tag is too large for this file.".into());
    }
    let mut tag = Vec::with_capacity(10 + payload.len());
    tag.extend_from_slice(b"ID3");
    tag.push(3); // v2.3
    tag.push(0);
    tag.push(0);
    tag.extend_from_slice(&synchsafe_bytes(payload.len()));
    tag.extend_from_slice(&payload);
    Ok(tag)
}

/// Applies the edit plan to an existing ID3v2 payload and renders a new tag.
/// `None` means the edit emptied the tag (the caller drops it).
fn build_id3_tag(payload: Option<&[u8]>, plan: &EditPlan) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    let (frames, _, _) = payload.and_then(parse_id3_tag).unwrap_or_default();
    let frames = apply_id3_edits(frames, plan);
    if frames.is_empty() {
        return Ok(None);
    }
    Ok(Some(render_id3v2_tag(&frames)?))
}

// ======================================================================
// RIFF/WAVE and FORM/AIFF chunk writers
// ======================================================================

/// Replaces the `ID3 ` chunk inside a RIFF or FORM container, keeping every
/// other chunk verbatim and fixing the container size field.
fn rewrite_riff_chunk(
    data: &[u8],
    plan: &EditPlan,
    form: &[u8; 4],
    big_endian: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let type_ok = if big_endian {
        &data[8..12] == b"AIFF" || &data[8..12] == b"AIFC"
    } else {
        &data[8..12] == b"WAVE"
    };
    if data.len() < 12 || &data[0..4] != form || !type_ok {
        return Err("Invalid container header.".into());
    }
    let read_size = |bytes: &[u8]| -> usize {
        if big_endian {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
        } else {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
        }
    };
    let write_size = |value: usize| -> [u8; 4] {
        if big_endian {
            (value as u32).to_be_bytes()
        } else {
            (value as u32).to_le_bytes()
        }
    };
    let push_chunk = |out: &mut Vec<u8>, id: &[u8], body: &[u8]| {
        out.extend_from_slice(id);
        out.extend_from_slice(&write_size(body.len()));
        out.extend_from_slice(body);
        if body.len() & 1 == 1 {
            out.push(0);
        }
    };

    // Walk the top-level chunks; remember where the ID3 chunk lived.
    #[derive(Clone)]
    struct Chunk {
        id: [u8; 4],
        body: Vec<u8>,
    }
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut trailing: Vec<u8> = Vec::new();
    let mut id3_position: Option<usize> = None;
    let mut offset = 12usize;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let size = read_size(&data[offset + 4..offset + 8]);
        if offset + 8 + size > data.len() {
            // Truncated final chunk: keep everything from here on raw.
            trailing = data[offset..].to_vec();
            break;
        }
        let mut chunk = Chunk {
            id: [chunk_id[0], chunk_id[1], chunk_id[2], chunk_id[3]],
            body: data[offset + 8..offset + 8 + size].to_vec(),
        };
        if chunk_id == b"ID3 " || chunk_id == b"id3 " {
            if id3_position.is_none() {
                id3_position = Some(chunks.len());
            } else {
                // Duplicate tag chunks: the first one wins, like the reader.
                chunk.body.clear();
            }
        }
        chunks.push(chunk);
        offset += 8 + size + (size & 1);
    }

    let payload = id3_position.map(|index| chunks[index].body.clone());
    let new_tag = build_id3_tag(payload.as_deref(), plan)?;

    if new_tag.is_none() && id3_position.is_none() {
        // Nothing to write and no tag present: leave the container unchanged.
        return Ok(data.to_vec());
    }

    let mut out = Vec::with_capacity(data.len() + new_tag.as_ref().map_or(0, |t| t.len()));
    out.extend_from_slice(&data[0..12]);
    let mut inserted = false;
    for (index, chunk) in chunks.iter().enumerate() {
        if Some(index) == id3_position {
            if let Some(tag) = new_tag.as_ref() {
                push_chunk(&mut out, b"ID3 ", tag);
            }
            // An empty frame list drops the ID3 chunk (empty tags removed).
            inserted = true;
            continue;
        }
        push_chunk(&mut out, &chunk.id, &chunk.body);
    }
    if !inserted {
        // TagLib appends the ID3 chunk after the audio data.
        push_chunk(&mut out, b"ID3 ", new_tag.as_ref().unwrap());
    }
    if !trailing.is_empty() {
        out.extend_from_slice(&trailing);
    }
    let container_size = write_size(out.len() - 8);
    out[4..8].copy_from_slice(&container_size);
    Ok(out)
}

// ======================================================================
// DSF writer
// ======================================================================

/// DSF layout: audio bytes then the ID3v2 tail tag. The writer truncates at
/// the metadata pointer, appends the new tag, and updates the pointer plus
/// the total file size.
fn rewrite_dsf(data: &[u8], plan: &EditPlan) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if data.len() < 28 || &data[0..4] != b"DSD " {
        return Err("Invalid DSF file.".into());
    }
    let metadata_ptr = u64::from_le_bytes([
        data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
    ]) as usize;
    let audio_end = if metadata_ptr >= 28 && metadata_ptr <= data.len() {
        metadata_ptr
    } else {
        data.len()
    };
    let payload: Option<&[u8]> = if audio_end < data.len() {
        Some(&data[audio_end..])
    } else {
        None
    };
    let new_tag = build_id3_tag(payload, plan)?;

    let mut out = data[..audio_end].to_vec();
    if let Some(tag) = new_tag {
        out.extend_from_slice(&tag);
        out[20..28].copy_from_slice(&(audio_end as u64).to_le_bytes());
    } else if audio_end < data.len() {
        // An emptied tag removes the tail and clears the pointer.
        out[20..28].copy_from_slice(&0u64.to_le_bytes());
    } else {
        // No tag existed and nothing to write: leave the file unchanged.
        return Ok(out);
    }
    let total_size = (out.len() as u64).to_le_bytes();
    out[12..20].copy_from_slice(&total_size);
    Ok(out)
}

// ======================================================================
// FLAC writer
// ======================================================================

const FLAC_COMMENT_BLOCK: u8 = 4;
const FLAC_PICTURE_BLOCK: u8 = 6;

/// Rebuilds the FLAC metadata blocks: the VORBIS_COMMENT and PICTURE blocks
/// are replaced, everything else is copied verbatim.
fn rewrite_flac(data: &[u8], plan: &EditPlan) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if data.len() < 8 || &data[0..4] != b"fLaC" {
        return Err("Invalid FLAC file.".into());
    }

    struct Block {
        kind: u8,
        body: Vec<u8>,
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut comment_body: Option<Vec<u8>> = None;
    let mut has_picture = false;
    let mut offset = 4usize;
    loop {
        if offset + 4 > data.len() {
            break;
        }
        let is_last = data[offset] & 0x80 != 0;
        let kind = data[offset] & 0x7F;
        let len = ((data[offset + 1] as usize) << 16)
            | ((data[offset + 2] as usize) << 8)
            | (data[offset + 3] as usize);
        offset += 4;
        if offset + len > data.len() {
            break;
        }
        match kind {
            FLAC_COMMENT_BLOCK => comment_body = Some(data[offset..offset + len].to_vec()),
            FLAC_PICTURE_BLOCK => has_picture = true,
            _ => blocks.push(Block {
                kind,
                body: data[offset..offset + len].to_vec(),
            }),
        }
        offset += len;
        if is_last {
            break;
        }
    }
    if blocks.is_empty() || blocks[0].kind != 0 {
        return Err("Invalid FLAC metadata.".into());
    }

    // Existing comments plus their vendor string.
    let (vendor, existing) = match comment_body.as_deref() {
        Some(body) if body.len() >= 4 => {
            let vendor_len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
            let vendor = body[4..(4 + vendor_len).min(body.len())].to_vec();
            (vendor, parse_vorbis_comments(body))
        }
        _ => (Vec::new(), Vec::new()),
    };

    let entries = apply_flac_comments(existing, plan);
    let comment_block = render_vorbis_comment_block(&vendor, &entries);
    let picture_block = plan.artwork.as_ref().map(|(image, mime)| build_flac_picture_body(image, mime));

    if entries.is_empty() && picture_block.is_none() && comment_body.is_none() && !has_picture {
        // No comment block existed and nothing to write.
        return Ok(data.to_vec());
    }

    let mut out = Vec::with_capacity(data.len() + comment_block.len());
    out.extend_from_slice(b"fLaC");
    let total = blocks.len() + 1 + usize::from(picture_block.is_some());
    let mut index = 0usize;
    for block in &blocks {
        index += 1;
        push_flac_block(&mut out, block.kind, &block.body, index == total);
    }
    index += 1;
    push_flac_block(&mut out, FLAC_COMMENT_BLOCK, &comment_block, index == total);
    if let Some(picture) = picture_block.as_ref() {
        push_flac_block(&mut out, FLAC_PICTURE_BLOCK, picture, index == total);
    }
    Ok(out)
}

/// Writes one FLAC metadata block header + body (`is_last` sets the header flag).
fn push_flac_block(out: &mut Vec<u8>, kind: u8, body: &[u8], is_last: bool) {
    out.push(if is_last { kind | 0x80 } else { kind });
    out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..4]);
    out.extend_from_slice(body);
}

/// Removes every comment whose key matches any of the listed keys.
fn vorbis_remove(entries: &mut Vec<(String, Vec<String>)>, keys: &[&str]) {
    entries.retain(|(key, _)| !keys.iter().any(|k| key.eq_ignore_ascii_case(k)));
}

fn vorbis_push(entries: &mut Vec<(String, Vec<String>)>, key: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    entries.push((key.to_string(), values.to_vec()));
}

/// Applies the edit plan to a merged comment list: canonical keys are written,
/// alias keys cleared, untouched comments preserved.
fn apply_flac_comments(
    mut entries: Vec<(String, Vec<String>)>,
    plan: &EditPlan,
) -> Vec<(String, Vec<String>)> {
    if let Some(title) = plan.title.as_ref() {
        vorbis_remove(&mut entries, &["TITLE"]);
        vorbis_push(&mut entries, "TITLE", std::slice::from_ref(title));
    }
    if let Some(artists) = plan.artists.as_ref() {
        vorbis_remove(&mut entries, &["ARTIST"]);
        vorbis_push(&mut entries, "ARTIST", artists);
    }
    if let Some(album) = plan.album.as_ref() {
        vorbis_remove(&mut entries, &["ALBUM"]);
        vorbis_push(&mut entries, "ALBUM", std::slice::from_ref(album));
    }
    if let Some(album_artists) = plan.album_artists.as_ref() {
        vorbis_remove(&mut entries, &["ALBUMARTIST", "ALBUM ARTIST"]);
        vorbis_push(&mut entries, "ALBUMARTIST", album_artists);
    }
    if let Some(genres) = plan.genres.as_ref() {
        vorbis_remove(&mut entries, &["GENRE"]);
        vorbis_push(&mut entries, "GENRE", genres);
    }
    if plan.year.is_some() {
        vorbis_remove(&mut entries, &["DATE", "YEAR"]);
        if let Some(year) = plan.year.filter(|y| *y > 0) {
            vorbis_push(&mut entries, "DATE", &[year.to_string()]);
        }
    }
    if plan.track.is_some() {
        vorbis_remove(&mut entries, &["TRACKNUMBER", "TRACK"]);
        if let Some(track) = plan.track.filter(|t| *t > 0) {
            vorbis_push(&mut entries, "TRACKNUMBER", &[track.to_string()]);
        }
    }
    if let Some(lyrics) = plan.lyrics.as_ref() {
        vorbis_remove(&mut entries, &["LYRICS"]);
        let trimmed = lyrics.trim().to_string();
        if !trimmed.is_empty() {
            vorbis_push(&mut entries, "LYRICS", &[trimmed]);
        }
    }

    // Additional fields: clear aliases, write the canonical key.
    for (key, canonical, names) in XIPH_ADDITIONAL_ALIASES {
        if let Some(values) = xiph_field_values(plan, key) {
            vorbis_remove(&mut entries, names);
            vorbis_push(&mut entries, canonical, &values);
        }
    }

    // Custom fields: reconcile against the existing non-common keys.
    let desired = &plan.custom;
    let existing_keys: Vec<String> = entries
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| !is_common_field(key))
        .collect();
    for key in &existing_keys {
        if desired.keys().any(|d| d.eq_ignore_ascii_case(key)) {
            continue;
        }
        vorbis_remove(&mut entries, &[key.as_str()]);
    }
    for (key, value) in desired {
        let values = split_values(value);
        vorbis_remove(&mut entries, &[key]);
        vorbis_push(&mut entries, key, &values);
    }
    entries
}

/// Resolves an additional field's rendered value list (None = untouched;
/// empty list = clear).
fn xiph_field_values(plan: &EditPlan, key: &str) -> Option<Vec<String>> {
    if let Some(value) = plan.strings.get(key) {
        return Some(if value.is_empty() {
            Vec::new()
        } else {
            vec![value.clone()]
        });
    }
    if let Some(values) = plan.arrays.get(key) {
        return Some(values.clone());
    }
    if let Some(number) = plan_uint(plan, key) {
        return Some(if number == 0 {
            Vec::new()
        } else {
            vec![number.to_string()]
        });
    }
    if let Some(value) = plan.doubles.get(key) {
        return Some(if value.is_nan() {
            Vec::new()
        } else {
            vec![format_double(*value)]
        });
    }
    None
}

fn format_double(value: f64) -> String {
    format!("{}", value)
}

fn plan_uint(plan: &EditPlan, key: &str) -> Option<u32> {
    match key {
        "TRACK_COUNT" => plan.track_count,
        "DISC" => plan.disc,
        "DISC_COUNT" => plan.disc_count,
        "BPM" => plan.bpm,
        _ => None,
    }
}

/// (catalog key, canonical Xiph name, every name the reader maps to the
/// field — the removal list always includes the canonical name). Canonical
/// names follow the reader's first-lookup order.
const XIPH_ADDITIONAL_ALIASES: &[(&str, &str, &[&str])] = &[
    ("TITLE_SORT", "TITLESORT", &["TITLESORT", "TITLE_SORT", "SORTNAME"]),
    ("SUBTITLE", "SUBTITLE", &["SUBTITLE"]),
    ("DESCRIPTION", "DESCRIPTION", &["DESCRIPTION"]),
    ("ARTIST_SORT", "ARTISTSORT", &["ARTISTSORT", "ARTIST_SORT", "SORTARTIST"]),
    ("ARTIST_ROLE", "ARTIST_ROLE", &["ARTIST_ROLE", "PERFORMER_ROLE"]),
    ("ALBUM_ARTIST_SORT", "ALBUMARTISTSORT", &["ALBUMARTISTSORT", "ALBUM_ARTIST_SORT"]),
    ("COMPOSERS", "COMPOSER", &["COMPOSER", "COMPOSERS"]),
    ("COMPOSER_SORT", "COMPOSERSORT", &["COMPOSERSORT", "COMPOSER_SORT"]),
    ("ALBUM_SORT", "ALBUMSORT", &["ALBUMSORT", "ALBUM_SORT", "SORTALBUM"]),
    ("COMMENT", "COMMENT", &["COMMENT"]),
    ("TRACK_COUNT", "TRACKTOTAL", &["TRACKTOTAL", "TRACKCOUNT", "TOTALTRACKS"]),
    ("DISC", "DISCNUMBER", &["DISCNUMBER", "DISC"]),
    ("DISC_COUNT", "DISCTOTAL", &["DISCTOTAL", "DISCCOUNT", "TOTALDISCS"]),
    ("GROUPING", "GROUPING", &["GROUPING", "CONTENTGROUP"]),
    ("BPM", "BPM", &["BPM", "BEATSPERMINUTE"]),
    ("CONDUCTOR", "CONDUCTOR", &["CONDUCTOR"]),
    ("COPYRIGHT", "COPYRIGHT", &["COPYRIGHT"]),
    ("MUSICBRAINZ_ARTIST_ID", "MUSICBRAINZ_ARTIST_ID", &["MUSICBRAINZ_ARTIST_ID"]),
    ("MUSICBRAINZ_RELEASE_GROUP_ID", "MUSICBRAINZ_RELEASE_GROUP_ID", &["MUSICBRAINZ_RELEASE_GROUP_ID"]),
    (
        "MUSICBRAINZ_RELEASE_ID",
        "MUSICBRAINZ_RELEASE_ID",
        &["MUSICBRAINZ_RELEASE_ID", "MUSICBRAINZ_ALBUM_ID"],
    ),
    (
        "MUSICBRAINZ_RELEASE_ARTIST_ID",
        "MUSICBRAINZ_RELEASE_ARTIST_ID",
        &["MUSICBRAINZ_RELEASE_ARTIST_ID", "MUSICBRAINZ_ALBUM_ARTIST_ID"],
    ),
    ("MUSICBRAINZ_TRACK_ID", "MUSICBRAINZ_TRACK_ID", &["MUSICBRAINZ_TRACK_ID"]),
    ("MUSICBRAINZ_DISC_ID", "MUSICBRAINZ_DISC_ID", &["MUSICBRAINZ_DISC_ID"]),
    ("MUSICIP_ID", "MUSICIP_ID", &["MUSICIP_ID"]),
    ("AMAZON_ID", "AMAZON_ID", &["AMAZON_ID"]),
    (
        "MUSICBRAINZ_RELEASE_STATUS",
        "MUSICBRAINZ_RELEASE_STATUS",
        &["MUSICBRAINZ_RELEASE_STATUS", "MUSICBRAINZ_ALBUM_STATUS"],
    ),
    (
        "MUSICBRAINZ_RELEASE_TYPE",
        "MUSICBRAINZ_RELEASE_TYPE",
        &["MUSICBRAINZ_RELEASE_TYPE", "MUSICBRAINZ_ALBUM_TYPE"],
    ),
    (
        "MUSICBRAINZ_RELEASE_COUNTRY",
        "MUSICBRAINZ_RELEASE_COUNTRY",
        &["MUSICBRAINZ_RELEASE_COUNTRY", "MUSICBRAINZ_ALBUM_RELEASE_COUNTRY"],
    ),
    ("DATE_TAGGED", "DATE_TAGGED", &["DATE_TAGGED"]),
    ("REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_GAIN", &["REPLAYGAIN_TRACK_GAIN"]),
    ("REPLAYGAIN_TRACK_PEAK", "REPLAYGAIN_TRACK_PEAK", &["REPLAYGAIN_TRACK_PEAK"]),
    ("REPLAYGAIN_ALBUM_GAIN", "REPLAYGAIN_ALBUM_GAIN", &["REPLAYGAIN_ALBUM_GAIN"]),
    ("REPLAYGAIN_ALBUM_PEAK", "REPLAYGAIN_ALBUM_PEAK", &["REPLAYGAIN_ALBUM_PEAK"]),
    ("INITIAL_KEY", "INITIAL_KEY", &["INITIAL_KEY", "KEY"]),
    ("REMIXED_BY", "REMIXED_BY", &["REMIXED_BY"]),
    ("PUBLISHER", "PUBLISHER", &["PUBLISHER"]),
    ("ISRC", "ISRC", &["ISRC"]),
];

/// Renders a VORBIS_COMMENT block body: one `KEY=value` comment per value.
fn render_vorbis_comment_block(vendor: &[u8], entries: &[(String, Vec<String>)]) -> Vec<u8> {
    let count: usize = entries.iter().map(|(_, values)| values.len().max(1)).sum();
    let mut body = Vec::new();
    body.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    body.extend_from_slice(vendor);
    body.extend_from_slice(&(count as u32).to_le_bytes());
    for (key, values) in entries {
        for value in if values.is_empty() { vec![String::new()] } else { values.clone() } {
            let mut line = key.as_bytes().to_vec();
            line.push(b'=');
            line.extend_from_slice(value.as_bytes());
            body.extend_from_slice(&(line.len() as u32).to_le_bytes());
            body.extend_from_slice(&line);
        }
    }
    body
}

/// Builds a FLAC PICTURE block body (raw binary per the metadata spec).
fn build_flac_picture_body(image: &[u8], mime: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(image.len() + 64);
    body.extend_from_slice(&3u32.to_be_bytes()); // front cover
    body.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    body.extend_from_slice(mime.as_bytes());
    body.extend_from_slice(&0u32.to_be_bytes()); // empty description
    for _ in 0..4 {
        body.extend_from_slice(&0u32.to_be_bytes()); // width/height/depth/colors
    }
    body.extend_from_slice(&(image.len() as u32).to_be_bytes());
    body.extend_from_slice(image);
    body
}

// ======================================================================
// ASF writer (.wma)
// ======================================================================

const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_CONTENT_DESCRIPTION_GUID: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_EXTENDED_CONTENT_GUID: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0x3F, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x36,
];

/// Canonical descriptor names for the additional fields (reader-mapped names
/// first; unmapped fields fall back to the verbatim catalog key).
const ASF_ADDITIONAL_NAMES: &[(&str, &str)] = &[
    ("TITLE_SORT", "WM/TitleSort"),
    ("SUBTITLE", "WM/SubTitle"),
    ("DESCRIPTION", "WM/Description"),
    ("ARTIST_SORT", "WM/ArtistSort"),
    ("ALBUM_ARTIST_SORT", "WM/AlbumArtistSort"),
    ("COMPOSERS", "WM/Composers"),
    ("COMPOSER_SORT", "WM/ComposerSort"),
    ("ALBUM_SORT", "WM/AlbumSort"),
    ("COMMENT", "WM/Comment"),
    ("TRACK_COUNT", "WM/TrackCount"),
    ("DISC", "WM/PartOfSet"),
    ("GROUPING", "WM/ContentGroupDescription"),
    ("BPM", "WM/BeatsPerMinute"),
    ("CONDUCTOR", "WM/Conductor"),
    ("COPYRIGHT", "WM/Copyright"),
    ("MUSICBRAINZ_ARTIST_ID", "MusicBrainz/Artist Id"),
    ("MUSICBRAINZ_RELEASE_GROUP_ID", "MusicBrainz/Release Group Id"),
    ("MUSICBRAINZ_RELEASE_ID", "MusicBrainz/Album Id"),
    ("MUSICBRAINZ_RELEASE_ARTIST_ID", "MusicBrainz/Album Artist Id"),
    ("MUSICBRAINZ_TRACK_ID", "MusicBrainz/Track Id"),
    ("MUSICBRAINZ_DISC_ID", "MusicBrainz/Disc Id"),
    ("MUSICBRAINZ_RELEASE_STATUS", "MusicBrainz/Album Status"),
    ("MUSICBRAINZ_RELEASE_TYPE", "MusicBrainz/Album Type"),
    ("MUSICBRAINZ_RELEASE_COUNTRY", "MusicBrainz/Album Release Country"),
    ("INITIAL_KEY", "WM/InitialKey"),
    ("REMIXED_BY", "WM/ModifiedBy"),
    ("PUBLISHER", "WM/Publisher"),
    ("ISRC", "WM/ISRC"),
];

/// Encodes a UTF-16LE string without BOM, NUL-terminated.
fn asf_wide_string(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(value.len() * 2 + 2);
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

/// The five Content Description strings (title/author/copyright/desc/rating).
fn parse_asf_content_description(body: &[u8]) -> [String; 5] {
    let mut texts: [String; 5] = Default::default();
    if body.len() < 10 {
        return texts;
    }
    let lens: Vec<usize> = (0..5)
        .map(|i| u16::from_le_bytes([body[i * 2], body[i * 2 + 1]]) as usize)
        .collect();
    let mut cursor = 10usize;
    for (index, len) in lens.iter().enumerate() {
        if cursor + len <= body.len() {
            texts[index] = decode_utf16le(&body[cursor..cursor + len]);
        }
        cursor += len;
    }
    texts
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        units.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
        i += 2;
    }
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

/// One Extended Content Description descriptor (raw value bytes preserved).
struct AsfDescriptor {
    name: String,
    value_type: u16,
    value: Vec<u8>,
}

fn parse_asf_extended_content(body: &[u8]) -> Vec<AsfDescriptor> {
    let mut descriptors = Vec::new();
    if body.len() < 2 {
        return descriptors;
    }
    let count = u16::from_le_bytes([body[0], body[1]]) as usize;
    let mut cursor = 2usize;
    for _ in 0..count {
        if cursor + 4 > body.len() {
            break;
        }
        let name_len = u16::from_le_bytes([body[cursor], body[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + name_len > body.len() {
            break;
        }
        let name = decode_utf16le(&body[cursor..cursor + name_len]);
        cursor += name_len;
        if cursor + 4 > body.len() {
            break;
        }
        let value_type = u16::from_le_bytes([body[cursor], body[cursor + 1]]);
        let value_len = u16::from_le_bytes([body[cursor + 2], body[cursor + 3]]) as usize;
        cursor += 4;
        if cursor + value_len > body.len() {
            break;
        }
        descriptors.push(AsfDescriptor {
            name,
            value_type,
            value: body[cursor..cursor + value_len].to_vec(),
        });
        cursor += value_len;
    }
    descriptors
}

/// Rebuilds the ASF header objects: Content Description (title/author) and
/// Extended Content Description (everything else). Stream data stays raw.
fn rewrite_asf(data: &[u8], plan: &EditPlan) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if data.len() < 30 || data[0..16] != ASF_HEADER_GUID {
        return Err("Invalid ASF file.".into());
    }
    let header_size = u64::from_le_bytes([
        data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
    ]);
    let end = (header_size as usize).min(data.len());
    if end < 30 {
        return Err("Invalid ASF file.".into());
    }

    let mut objects: Vec<([u8; 16], Vec<u8>)> = Vec::new();
    let mut offset = 30usize;
    while offset + 24 <= end {
        let guid: [u8; 16] = data[offset..offset + 16]
            .try_into()
            .map_err(|_| "Invalid ASF file.")?;
        let size = u64::from_le_bytes([
            data[offset + 16], data[offset + 17], data[offset + 18], data[offset + 19],
            data[offset + 20], data[offset + 21], data[offset + 22], data[offset + 23],
        ]) as usize;
        if size < 24 || offset + size > end {
            break;
        }
        objects.push((guid, data[offset + 24..offset + size].to_vec()));
        offset += size;
    }
    let trailing = data[end.min(data.len())..].to_vec();

    // Content Description: replace title/author, keep the rest.
    let cd_edited = plan.title.is_some() || plan.artists.is_some();
    let mut cd_text: [String; 5] = Default::default();

    // Extended Content Description: rebuild the unicode descriptors.
    let mut ecd: Vec<AsfDescriptor> = Vec::new();
    let mut has_cd = false;
    let mut has_ecd = false;
    for (guid, body) in &objects {
        if *guid == ASF_CONTENT_DESCRIPTION_GUID {
            has_cd = true;
            cd_text = parse_asf_content_description(body);
        } else if *guid == ASF_EXTENDED_CONTENT_GUID {
            has_ecd = true;
            ecd = parse_asf_extended_content(body);
        }
    }

    // Names whose unicode descriptors get replaced by this edit.
    let standard_edited_names: Vec<&str> = {
        let mut names: Vec<&str> = Vec::new();
        if plan.album.is_some() {
            names.push("WM/AlbumTitle");
        }
        if plan.album_artists.is_some() {
            names.push("WM/AlbumArtist");
        }
        if plan.genres.is_some() {
            names.push("WM/Genre");
        }
        if plan.year.is_some() {
            names.push("WM/Year");
        }
        if plan.track.is_some() {
            names.push("WM/TrackNumber");
        }
        if plan.lyrics.is_some() {
            names.push("WM/Lyrics");
        }
        names
    };
    let additional_edited: Vec<(String, Vec<String>)> = additional_name_values(plan);
    let custom_edited: Vec<(String, Vec<String>)> = plan
        .custom
        .iter()
        .map(|(key, value)| (key.clone(), split_values(value)))
        .collect();

    let mut edited_names: Vec<String> = standard_edited_names
        .iter()
        .map(|name| name.to_string())
        .collect();
    for (name, _) in additional_edited.iter().chain(custom_edited.iter()) {
        edited_names.push(name.clone());
    }

    // Keep non-unicode descriptors verbatim (WM/Picture etc.), plus unicode
    // descriptors this edit does not touch.
    let mut kept: Vec<AsfDescriptor> = Vec::new();
    for descriptor in ecd {
        if descriptor.value_type == 0
            && edited_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&descriptor.name))
        {
            continue;
        }
        kept.push(descriptor);
    }

    let write_descriptor = |values: &[String], name: &str| -> Option<AsfDescriptor> {
        let joined = values.join("; ");
        if joined.is_empty() {
            return None;
        }
        Some(AsfDescriptor {
            name: name.to_string(),
            value_type: 0,
            value: asf_wide_string(&joined),
        })
    };

    for name in standard_edited_names {
        let values: Vec<String> = match name {
            "WM/AlbumTitle" => plan.album.as_ref().map(|v| vec![v.clone()]).unwrap_or_default(),
            "WM/AlbumArtist" => plan.album_artists.clone().unwrap_or_default(),
            "WM/Genre" => plan.genres.clone().unwrap_or_default(),
            "WM/Year" => plan.year.map(|y| y.to_string()).map(|y| vec![y]).unwrap_or_default(),
            "WM/TrackNumber" => plan.track.map(|t| t.to_string()).map(|t| vec![t]).unwrap_or_default(),
            "WM/Lyrics" => plan.lyrics.as_ref().map(|l| vec![l.trim().to_string()]).unwrap_or_default(),
            _ => Vec::new(),
        };
        if let Some(descriptor) = write_descriptor(&values, name) {
            kept.push(descriptor);
        }
    }
    for (name, values) in additional_edited.iter().chain(custom_edited.iter()) {
        if let Some(descriptor) = write_descriptor(values, name) {
            kept.push(descriptor);
        }
    }

    if cd_edited {
        if let Some(title) = plan.title.as_ref() {
            cd_text[0] = title.clone();
        }
        if let Some(artists) = plan.artists.as_ref() {
            cd_text[1] = artists.join("; ");
        }
    }
    let cd_body = if has_cd || cd_edited {
        render_asf_content_description(&cd_text)
    } else {
        Vec::new()
    };
    let ecd_body = if has_ecd || !kept.is_empty() || has_cd {
        render_asf_extended_content(&kept)
    } else {
        Vec::new()
    };

    // Reassemble the header objects. Bytes 24..30 of the header preamble are
    // reserved and preserved; the header size excludes the trailing data.
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&ASF_HEADER_GUID);
    out.extend_from_slice(&0u64.to_le_bytes()); // size patched below
    out.extend_from_slice(&data[24..30]);
    let mut wrote_cd = false;
    let mut wrote_ecd = false;
    for (guid, body) in &objects {
        if *guid == ASF_CONTENT_DESCRIPTION_GUID {
            push_asf_object(&mut out, guid, &cd_body);
            wrote_cd = true;
            continue;
        }
        if *guid == ASF_EXTENDED_CONTENT_GUID {
            push_asf_object(&mut out, guid, &ecd_body);
            wrote_ecd = true;
            continue;
        }
        push_asf_object(&mut out, guid, body);
    }
    // Insert objects the file lacked (readers accept any object order).
    if !wrote_cd && cd_edited {
        push_asf_object(&mut out, &ASF_CONTENT_DESCRIPTION_GUID, &cd_body);
    }
    if !wrote_ecd && !kept.is_empty() {
        push_asf_object(&mut out, &ASF_EXTENDED_CONTENT_GUID, &ecd_body);
    }
    let header_size = out.len() as u64;
    out.extend_from_slice(&trailing);
    out[16..24].copy_from_slice(&header_size.to_le_bytes());
    Ok(out)
}

fn push_asf_object(out: &mut Vec<u8>, guid: &[u8; 16], body: &[u8]) {
    out.extend_from_slice(guid);
    out.extend_from_slice(&((body.len() + 24) as u64).to_le_bytes());
    out.extend_from_slice(body);
}

fn render_asf_content_description(texts: &[String; 5]) -> Vec<u8> {
    let encoded: Vec<Vec<u8>> = texts.iter().map(|text| asf_wide_string(text)).collect();
    let mut body = Vec::new();
    for part in &encoded {
        body.extend_from_slice(&((part.len()) as u16).to_le_bytes());
    }
    for part in &encoded {
        body.extend_from_slice(part);
    }
    body
}

fn render_asf_extended_content(descriptors: &[AsfDescriptor]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(descriptors.len() as u16).to_le_bytes());
    for descriptor in descriptors {
        let name = asf_wide_string(&descriptor.name);
        body.extend_from_slice(&((name.len()) as u16).to_le_bytes());
        body.extend_from_slice(&name);
        body.extend_from_slice(&descriptor.value_type.to_le_bytes());
        body.extend_from_slice(&((descriptor.value.len()) as u16).to_le_bytes());
        body.extend_from_slice(&descriptor.value);
    }
    body
}

/// Resolves an additional field's desired values (None = untouched).
fn additional_values(plan: &EditPlan, key: &str) -> Option<Vec<String>> {
    if key == "DATE_TAGGED" {
        return plan.date_tagged.as_ref().map(|v| vec![v.clone()]);
    }
    if let Some(value) = plan.strings.get(key) {
        return Some(vec![value.clone()]);
    }
    if let Some(values) = plan.arrays.get(key) {
        return Some(values.clone());
    }
    if let Some(number) = plan_uint(plan, key) {
        return Some(if number == 0 {
            Vec::new()
        } else {
            vec![number.to_string()]
        });
    }
    if let Some(value) = plan.doubles.get(key) {
        return Some(if value.is_nan() {
            Vec::new()
        } else {
            vec![format_double(*value)]
        });
    }
    None
}

/// Resolves every edited additional field to its canonical ASF descriptor
/// name and values (unmapped keys use the verbatim catalog key).
fn additional_name_values(plan: &EditPlan) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for (key, _) in ADDITIONAL_STANDARD_FIELDS {
        let Some(values) = additional_values(plan, key) else {
            continue;
        };
        let name = ASF_ADDITIONAL_NAMES
            .iter()
            .find(|(k, _)| *k == *key)
            .map(|(_, name)| *name)
            .unwrap_or(key);
        out.push((name.to_string(), values));
    }
    out
}

// ======================================================================
// APEv2 writer (ape / wv / mpc)
// ======================================================================

const APE_TAG_VERSION: u32 = 2000;
const APE_TAG_HEADER_FLAGS: u32 = 0xA000_0000; // has footer + this is the header
const APE_TAG_FOOTER_FLAGS: u32 = 0x8000_0000; // has a header

/// One raw APEv2 item: key + flags + raw value bytes. Untouched items
/// (including binary artwork items) are preserved verbatim.
struct ApeItem {
    key: String,
    flags: u32,
    value: Vec<u8>,
}

/// Parses the existing APEv2 tag: (items, region start, region end) where the
/// region spans header (when present) + items + footer.
fn parse_ape_tag(data: &[u8]) -> Option<(Vec<ApeItem>, usize, usize)> {
    let footer_pos = find_last_subslice(data, b"APETAGEX")?;
    if footer_pos + 32 > data.len() || footer_pos < 32 {
        return None;
    }
    let footer = &data[footer_pos..footer_pos + 32];
    let tag_size = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]) as usize;
    let item_count = u32::from_le_bytes([footer[16], footer[17], footer[18], footer[19]]) as usize;
    let has_header = footer[23] & 0x80 != 0;
    if tag_size < 32 || tag_size > data.len() {
        return None;
    }
    let region_start = footer_pos + 32 - tag_size;
    let mut items_start = region_start;
    if has_header {
        items_start += 32;
    }
    let items_end = footer_pos;
    let mut items = Vec::new();
    let mut offset = items_start;
    for _ in 0..item_count {
        if offset + 8 > items_end {
            break;
        }
        let size = u32::from_le_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
        ]) as usize;
        let flags = u32::from_le_bytes([
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]);
        offset += 8;
        if size > items_end.saturating_sub(offset) {
            break;
        }
        // Item size covers the key, its NUL terminator and the value.
        let key_end = data[offset..offset + size]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(size);
        let value_start = offset + key_end + 1;
        if key_end == 0 || value_start > offset + size {
            offset += size;
            continue;
        }
        items.push(ApeItem {
            key: String::from_utf8_lossy(&data[offset..offset + key_end]).to_string(),
            flags,
            value: data[value_start..offset + size].to_vec(),
        });
        offset += size;
    }
    Some((items, region_start, footer_pos + 32))
}

fn find_last_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

/// (catalog key, canonical item key, every item key the reader maps to the
/// field — the removal list always includes the canonical key). Canonical
/// keys follow the reader's first-lookup order.
const APE_ADDITIONAL_ALIASES: &[(&str, &str, &[&str])] = &[
    ("TITLE_SORT", "TITLE_SORT", &["TITLE_SORT", "TITLESORT", "SORTNAME", "Sort Title"]),
    ("SUBTITLE", "SUBTITLE", &["SUBTITLE", "Subtitle"]),
    ("DESCRIPTION", "DESCRIPTION", &["DESCRIPTION", "Description"]),
    ("ARTIST_SORT", "ARTIST_SORT", &["ARTIST_SORT", "ARTISTSORT", "SORTARTIST", "Sort Artist"]),
    ("ARTIST_ROLE", "ARTIST_ROLE", &["ARTIST_ROLE", "PERFORMER_ROLE"]),
    ("ALBUM_ARTIST_SORT", "ALBUM_ARTIST_SORT", &["ALBUM_ARTIST_SORT", "ALBUMARTISTSORT", "Sort Album Artist"]),
    ("COMPOSERS", "COMPOSER", &["COMPOSER", "COMPOSERS"]),
    ("COMPOSER_SORT", "COMPOSER_SORT", &["COMPOSER_SORT", "COMPOSERSORT", "Sort Composer"]),
    ("ALBUM_SORT", "ALBUM_SORT", &["ALBUM_SORT", "ALBUMSORT", "SORTALBUM", "Sort Album"]),
    ("COMMENT", "COMMENT", &["COMMENT"]),
    ("TRACK_COUNT", "TRACK_COUNT", &["TRACK_COUNT", "TRACKTOTAL", "TRACKCOUNT"]),
    ("DISC", "DISC", &["DISC", "DISCNUMBER"]),
    ("DISC_COUNT", "DISC_COUNT", &["DISC_COUNT", "DISCTOTAL", "DISCCOUNT"]),
    ("GROUPING", "GROUPING", &["GROUPING", "CONTENTGROUP"]),
    ("BPM", "BPM", &["BPM", "BEATSPERMINUTE"]),
    ("CONDUCTOR", "CONDUCTOR", &["CONDUCTOR"]),
    ("COPYRIGHT", "COPYRIGHT", &["COPYRIGHT"]),
    ("MUSICBRAINZ_ARTIST_ID", "MUSICBRAINZ_ARTIST_ID", &["MUSICBRAINZ_ARTIST_ID", "MusicBrainz Artist Id"]),
    ("MUSICBRAINZ_RELEASE_GROUP_ID", "MUSICBRAINZ_RELEASE_GROUP_ID", &["MUSICBRAINZ_RELEASE_GROUP_ID", "MusicBrainz Release Group Id"]),
    ("MUSICBRAINZ_RELEASE_ID", "MUSICBRAINZ_RELEASE_ID", &["MUSICBRAINZ_RELEASE_ID", "MusicBrainz Album Id"]),
    ("MUSICBRAINZ_RELEASE_ARTIST_ID", "MUSICBRAINZ_RELEASE_ARTIST_ID", &["MUSICBRAINZ_RELEASE_ARTIST_ID", "MusicBrainz Album Artist Id"]),
    ("MUSICBRAINZ_TRACK_ID", "MUSICBRAINZ_TRACK_ID", &["MUSICBRAINZ_TRACK_ID", "MusicBrainz Track Id"]),
    ("MUSICBRAINZ_DISC_ID", "MUSICBRAINZ_DISC_ID", &["MUSICBRAINZ_DISC_ID", "MusicBrainz Disc Id"]),
    ("MUSICIP_ID", "MUSICIP_ID", &["MUSICIP_ID", "MusicIP Id"]),
    ("AMAZON_ID", "AMAZON_ID", &["AMAZON_ID", "Amazon Id"]),
    ("MUSICBRAINZ_RELEASE_STATUS", "MUSICBRAINZ_RELEASE_STATUS", &["MUSICBRAINZ_RELEASE_STATUS", "MusicBrainz Album Status"]),
    ("MUSICBRAINZ_RELEASE_TYPE", "MUSICBRAINZ_RELEASE_TYPE", &["MUSICBRAINZ_RELEASE_TYPE", "MusicBrainz Album Type"]),
    ("MUSICBRAINZ_RELEASE_COUNTRY", "MUSICBRAINZ_RELEASE_COUNTRY", &["MUSICBRAINZ_RELEASE_COUNTRY", "MusicBrainz Album Release Country"]),
    ("DATE_TAGGED", "DATE_TAGGED", &["DATE_TAGGED"]),
    ("REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_GAIN", &["REPLAYGAIN_TRACK_GAIN"]),
    ("REPLAYGAIN_TRACK_PEAK", "REPLAYGAIN_TRACK_PEAK", &["REPLAYGAIN_TRACK_PEAK"]),
    ("REPLAYGAIN_ALBUM_GAIN", "REPLAYGAIN_ALBUM_GAIN", &["REPLAYGAIN_ALBUM_GAIN"]),
    ("REPLAYGAIN_ALBUM_PEAK", "REPLAYGAIN_ALBUM_PEAK", &["REPLAYGAIN_ALBUM_PEAK"]),
    ("INITIAL_KEY", "INITIAL_KEY", &["INITIAL_KEY"]),
    ("REMIXED_BY", "REMIXED_BY", &["REMIXED_BY"]),
    ("PUBLISHER", "PUBLISHER", &["PUBLISHER"]),
    ("ISRC", "ISRC", &["ISRC"]),
];

/// Replaces the APEv2 tag region, keeping audio and any trailing ID3v1 tag.
fn rewrite_ape_tag(data: &[u8], plan: &EditPlan) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (items, region_start, region_end) =
        parse_ape_tag(data).unwrap_or((Vec::new(), data.len(), data.len()));

    // Desired items: (key, values); an empty value list removes the key.
    let mut desired: Vec<(String, Vec<String>)> = Vec::new();
    if let Some(title) = plan.title.as_ref() {
        desired.push(("Title".to_string(), vec![title.clone()]));
    }
    if let Some(artists) = plan.artists.as_ref() {
        desired.push(("Artist".to_string(), artists.clone()));
    }
    if let Some(album) = plan.album.as_ref() {
        desired.push(("Album".to_string(), vec![album.clone()]));
    }
    if let Some(album_artists) = plan.album_artists.as_ref() {
        desired.push(("Album Artist".to_string(), album_artists.clone()));
    }
    if let Some(genres) = plan.genres.as_ref() {
        desired.push(("Genre".to_string(), genres.clone()));
    }
    if plan.year.is_some() {
        let year = plan.year.filter(|y| *y > 0).map(|y| y.to_string()).unwrap_or_default();
        desired.push(("Year".to_string(), vec![year]));
    }
    if plan.track.is_some() {
        let track = plan.track.filter(|t| *t > 0).map(|t| t.to_string()).unwrap_or_default();
        desired.push(("Track".to_string(), vec![track]));
    }
    if let Some(lyrics) = plan.lyrics.as_ref() {
        desired.push(("Lyrics".to_string(), vec![lyrics.trim().to_string()]));
    }
    for (key, canonical, names) in APE_ADDITIONAL_ALIASES {
        let Some(values) = additional_values(plan, key) else {
            continue;
        };
        desired.push((canonical.to_string(), values));
        // Alias keys are cleared whenever their field is edited.
        for name in names.iter() {
            if !name.eq_ignore_ascii_case(canonical) {
                desired.push((name.to_string(), Vec::new()));
            }
        }
    }
    for (key, value) in &plan.custom {
        desired.push((key.clone(), split_values(value)));
    }

    // Existing items survive unless their key is rewritten or cleared.
    let edited: Vec<&String> = desired.iter().map(|(key, _)| key).collect();
    let kept: Vec<&ApeItem> = items
        .iter()
        .filter(|item| !edited.iter().any(|key| key.eq_ignore_ascii_case(&item.key)))
        .collect();
    let written: Vec<(String, Vec<String>)> = desired
        .into_iter()
        .filter(|(_, values)| values.iter().any(|v| !v.is_empty()))
        .collect();

    if kept.is_empty() && written.is_empty() {
        if region_start >= data.len() {
            // Nothing existed and nothing to write: leave the file unchanged.
            return Ok(data.to_vec());
        }
        // Every item was cleared: drop the tag region entirely.
        let mut out = data[..region_start].to_vec();
        out.extend_from_slice(&data[region_end..]);
        return Ok(out);
    }

    // A fresh tag goes before a trailing ID3v1 block (TagLib keeps it last).
    let (audio_end, tail): (usize, &[u8]) = if region_start >= data.len() {
        let insert_at = if data.len() >= 128 && &data[data.len() - 128..data.len() - 125] == b"TAG" {
            data.len() - 128
        } else {
            data.len()
        };
        (insert_at, &data[insert_at..])
    } else {
        (region_start, &data[region_end..])
    };

    let mut out = Vec::with_capacity(data.len() + 128);
    out.extend_from_slice(&data[..audio_end]);
    out.extend_from_slice(&render_ape_tag(&kept, &written));
    out.extend_from_slice(tail);
    Ok(out)
}

/// Renders header + items + footer; `tag_size` covers items + footer.
fn render_ape_tag(kept: &[&ApeItem], written: &[(String, Vec<String>)]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut count = 0u32;
    for item in kept {
        render_ape_item(&mut body, &item.key, item.flags, &item.value);
        count += 1;
    }
    for (key, values) in written {
        let joined = values.join("\u{0}");
        render_ape_item(&mut body, key, 0, joined.as_bytes());
        count += 1;
    }
    let tag_size = (body.len() + 32) as u32;
    let mut out = Vec::with_capacity(body.len() + 64);
    // Header: has footer + this is the header.
    out.extend_from_slice(b"APETAGEX");
    out.extend_from_slice(&APE_TAG_VERSION.to_le_bytes());
    out.extend_from_slice(&tag_size.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&APE_TAG_HEADER_FLAGS.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&body);
    // Footer: has a header.
    out.extend_from_slice(b"APETAGEX");
    out.extend_from_slice(&APE_TAG_VERSION.to_le_bytes());
    out.extend_from_slice(&tag_size.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&APE_TAG_FOOTER_FLAGS.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out
}

fn render_ape_item(out: &mut Vec<u8>, key: &str, flags: u32, value: &[u8]) {
    // Size covers the key, its NUL terminator and the value.
    out.extend_from_slice(&((key.len() + 1 + value.len()) as u32).to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.push(0);
    out.extend_from_slice(value);
}

// ======================================================================
// MP4 writer (m4a / m4b / mp4)
// ======================================================================

const MP4_TEXT_DATA: u32 = 1; // UTF-8 text payload
const MP4_JPEG_DATA: u32 = 13;
const MP4_PNG_DATA: u32 = 14;

/// (catalog key, canonical atom name, every atom name the reader maps to the
/// field — the removal list always includes the canonical name). Freeform
/// names are spelled `----:com.apple.iTunes:<KEY>`.
const MP4_FIELD_ALIASES: &[(&str, &str, &[&str])] = &[
    ("TITLE_SORT", "----:com.apple.iTunes:TITLE_SORT", &["----:com.apple.iTunes:TITLE_SORT"]),
    ("SUBTITLE", "----:com.apple.iTunes:SUBTITLE", &["----:com.apple.iTunes:SUBTITLE"]),
    ("DESCRIPTION", "----:com.apple.iTunes:DESCRIPTION", &["----:com.apple.iTunes:DESCRIPTION"]),
    ("ARTIST_SORT", "----:com.apple.iTunes:ARTIST_SORT", &["----:com.apple.iTunes:ARTIST_SORT"]),
    ("ARTIST_ROLE", "----:com.apple.iTunes:ARTIST_ROLE", &["----:com.apple.iTunes:ARTIST_ROLE"]),
    ("ALBUM_ARTIST_SORT", "----:com.apple.iTunes:ALBUM_ARTIST_SORT", &["----:com.apple.iTunes:ALBUM_ARTIST_SORT"]),
    ("COMPOSERS", "\u{a9}wrt", &["\u{a9}wrt", "----:com.apple.iTunes:COMPOSERS"]),
    ("COMPOSER_SORT", "----:com.apple.iTunes:COMPOSER_SORT", &["----:com.apple.iTunes:COMPOSER_SORT"]),
    ("ALBUM_SORT", "----:com.apple.iTunes:ALBUM_SORT", &["----:com.apple.iTunes:ALBUM_SORT"]),
    ("COMMENT", "\u{a9}cmt", &["\u{a9}cmt", "----:com.apple.iTunes:COMMENT"]),
    (
        "TRACK_COUNT",
        "----:com.apple.iTunes:TRACK_COUNT",
        &["----:com.apple.iTunes:TRACK_COUNT", "----:com.apple.iTunes:TRACKTOTAL"],
    ),
    (
        "DISC",
        "----:com.apple.iTunes:DISC",
        &["----:com.apple.iTunes:DISC", "----:com.apple.iTunes:DISCNUMBER"],
    ),
    (
        "DISC_COUNT",
        "----:com.apple.iTunes:DISC_COUNT",
        &["----:com.apple.iTunes:DISC_COUNT", "----:com.apple.iTunes:DISCTOTAL"],
    ),
    ("GROUPING", "\u{a9}grp", &["\u{a9}grp", "----:com.apple.iTunes:GROUPING"]),
    ("BPM", "tmpo", &["tmpo", "----:com.apple.iTunes:BPM"]),
    ("CONDUCTOR", "----:com.apple.iTunes:CONDUCTOR", &["----:com.apple.iTunes:CONDUCTOR"]),
    ("COPYRIGHT", "cprt", &["cprt", "----:com.apple.iTunes:COPYRIGHT"]),
    ("MUSICBRAINZ_ARTIST_ID", "----:com.apple.iTunes:MUSICBRAINZ_ARTIST_ID", &["----:com.apple.iTunes:MUSICBRAINZ_ARTIST_ID"]),
    ("MUSICBRAINZ_RELEASE_GROUP_ID", "----:com.apple.iTunes:MUSICBRAINZ_RELEASE_GROUP_ID", &["----:com.apple.iTunes:MUSICBRAINZ_RELEASE_GROUP_ID"]),
    ("MUSICBRAINZ_RELEASE_ID", "----:com.apple.iTunes:MUSICBRAINZ_RELEASE_ID", &["----:com.apple.iTunes:MUSICBRAINZ_RELEASE_ID"]),
    ("MUSICBRAINZ_RELEASE_ARTIST_ID", "----:com.apple.iTunes:MUSICBRAINZ_RELEASE_ARTIST_ID", &["----:com.apple.iTunes:MUSICBRAINZ_RELEASE_ARTIST_ID"]),
    ("MUSICBRAINZ_TRACK_ID", "----:com.apple.iTunes:MUSICBRAINZ_TRACK_ID", &["----:com.apple.iTunes:MUSICBRAINZ_TRACK_ID"]),
    ("MUSICBRAINZ_DISC_ID", "----:com.apple.iTunes:MUSICBRAINZ_DISC_ID", &["----:com.apple.iTunes:MUSICBRAINZ_DISC_ID"]),
    ("MUSICIP_ID", "----:com.apple.iTunes:MUSICIP_ID", &["----:com.apple.iTunes:MUSICIP_ID"]),
    ("AMAZON_ID", "----:com.apple.iTunes:AMAZON_ID", &["----:com.apple.iTunes:AMAZON_ID"]),
    ("MUSICBRAINZ_RELEASE_STATUS", "----:com.apple.iTunes:MUSICBRAINZ_RELEASE_STATUS", &["----:com.apple.iTunes:MUSICBRAINZ_RELEASE_STATUS"]),
    ("MUSICBRAINZ_RELEASE_TYPE", "----:com.apple.iTunes:MUSICBRAINZ_RELEASE_TYPE", &["----:com.apple.iTunes:MUSICBRAINZ_RELEASE_TYPE"]),
    ("MUSICBRAINZ_RELEASE_COUNTRY", "----:com.apple.iTunes:MUSICBRAINZ_RELEASE_COUNTRY", &["----:com.apple.iTunes:MUSICBRAINZ_RELEASE_COUNTRY"]),
    ("DATE_TAGGED", "----:com.apple.iTunes:DATE_TAGGED", &["----:com.apple.iTunes:DATE_TAGGED"]),
    ("REPLAYGAIN_TRACK_GAIN", "----:com.apple.iTunes:REPLAYGAIN_TRACK_GAIN", &["----:com.apple.iTunes:REPLAYGAIN_TRACK_GAIN"]),
    ("REPLAYGAIN_TRACK_PEAK", "----:com.apple.iTunes:REPLAYGAIN_TRACK_PEAK", &["----:com.apple.iTunes:REPLAYGAIN_TRACK_PEAK"]),
    ("REPLAYGAIN_ALBUM_GAIN", "----:com.apple.iTunes:REPLAYGAIN_ALBUM_GAIN", &["----:com.apple.iTunes:REPLAYGAIN_ALBUM_GAIN"]),
    ("REPLAYGAIN_ALBUM_PEAK", "----:com.apple.iTunes:REPLAYGAIN_ALBUM_PEAK", &["----:com.apple.iTunes:REPLAYGAIN_ALBUM_PEAK"]),
    ("INITIAL_KEY", "----:com.apple.iTunes:INITIAL_KEY", &["----:com.apple.iTunes:INITIAL_KEY"]),
    ("REMIXED_BY", "----:com.apple.iTunes:REMIXED_BY", &["----:com.apple.iTunes:REMIXED_BY"]),
    ("PUBLISHER", "----:com.apple.iTunes:PUBLISHER", &["----:com.apple.iTunes:PUBLISHER"]),
    ("ISRC", "----:com.apple.iTunes:ISRC", &["----:com.apple.iTunes:ISRC"]),
];

/// One atom queued for writing into the rebuilt ilst.
enum Mp4Write {
    Text { name: String, values: Vec<String> },
    Track { number: u32, total: u32 },
    Cover { image: Vec<u8>, mime: String },
}

/// Rebuilds the `moov/udta/meta/ilst` chain with size awareness: the new moov
/// is padded with a `free` atom when smaller and chunk offsets are patched
/// when the file grows. Audio atoms are copied verbatim.
fn rewrite_mp4(data: &[u8], plan: &EditPlan) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // The first top-level moov atom is the movie box.
    let mut moov_span: Option<(usize, usize)> = None;
    let mut top_offset = 0usize;
    while top_offset + 8 <= data.len() {
        let Some((end, name)) = mp4_atom_at(data, top_offset) else {
            break;
        };
        if &name == b"moov" {
            moov_span = Some((top_offset, end));
            break;
        }
        if end <= top_offset {
            break;
        }
        top_offset = end;
    }
    let Some((moov_start, moov_end)) = moov_span else {
        return Err("No moov atom found.".into());
    };
    let moov_children = mp4_children(&data[moov_start + 8..moov_end]);

    // udta/meta/ilst: the only atoms this edit rewrites.
    let udta_child = moov_children.iter().find(|(name, _)| name == b"udta");
    let udta_children = udta_child
        .map(|(_, payload)| mp4_children(payload))
        .unwrap_or_default();
    let meta_child = udta_children.iter().find(|(name, _)| name == b"meta");
    let meta_payload = meta_child.map(|(_, payload)| payload.as_slice()).unwrap_or(&[]);
    let (meta_prefix, meta_children): ([u8; 4], Vec<([u8; 4], Vec<u8>)>) = if meta_payload.len() >= 4 {
        (
            [meta_payload[0], meta_payload[1], meta_payload[2], meta_payload[3]],
            mp4_children(&meta_payload[4..]),
        )
    } else {
        ([0; 4], Vec::new())
    };
    let old_ilst = meta_children
        .iter()
        .find(|(name, _)| name == b"ilst")
        .map(|(_, payload)| ilst_children(payload))
        .unwrap_or_default();

    // ------------------------------------------------------------------
    // Compute the new ilst children.
    // ------------------------------------------------------------------
    let mut removal: BTreeSet<String> = BTreeSet::new();
    let mut written: Vec<Mp4Write> = Vec::new();

    if let Some(title) = plan.title.as_ref() {
        removal.insert("\u{a9}nam".to_string());
        mp4_push_text(&mut written, "\u{a9}nam", &[title.clone()]);
    }
    if let Some(artists) = plan.artists.as_ref() {
        removal.insert("\u{a9}ART".to_string());
        mp4_push_text(&mut written, "\u{a9}ART", artists);
    }
    if let Some(album) = plan.album.as_ref() {
        removal.insert("\u{a9}alb".to_string());
        mp4_push_text(&mut written, "\u{a9}alb", &[album.clone()]);
    }
    if let Some(album_artists) = plan.album_artists.as_ref() {
        removal.insert("aART".to_string());
        mp4_push_text(&mut written, "aART", album_artists);
    }
    if let Some(genres) = plan.genres.as_ref() {
        removal.insert("\u{a9}gen".to_string());
        removal.insert("gnre".to_string());
        mp4_push_text(&mut written, "\u{a9}gen", genres);
    }
    if plan.year.is_some() {
        removal.insert("\u{a9}day".to_string());
        if let Some(year) = plan.year.filter(|y| *y > 0) {
            mp4_push_text(&mut written, "\u{a9}day", &[year.to_string()]);
        }
    }
    if plan.track.is_some() || plan.track_count.is_some() {
        let (number, total) = mp4_existing_pair(&old_ilst, "trkn");
        let number = plan.track.filter(|t| *t > 0).or(number);
        let total = plan.track_count.filter(|t| *t > 0).or(total);
        removal.insert("trkn".to_string());
        if number.is_some() || total.is_some() {
            written.push(Mp4Write::Track {
                number: number.unwrap_or(0),
                total: total.unwrap_or(0),
            });
        }
    }
    if let Some(lyrics) = plan.lyrics.as_ref() {
        removal.insert("\u{a9}lyr".to_string());
        mp4_push_text(&mut written, "\u{a9}lyr", &[lyrics.trim().to_string()]);
    }
    if let Some((image, mime)) = plan.artwork.as_ref() {
        removal.insert("covr".to_string());
        written.push(Mp4Write::Cover {
            image: image.clone(),
            mime: mime.clone(),
        });
    }
    for (key, canonical, names) in MP4_FIELD_ALIASES {
        let Some(values) = additional_values(plan, key) else {
            continue;
        };
        for name in names.iter() {
            removal.insert(name.to_string());
        }
        mp4_push_text(&mut written, canonical, &values);
    }
    for (key, value) in &plan.custom {
        let name = format!("----:com.apple.iTunes:{}", key);
        removal.insert(name.clone());
        mp4_push_text(&mut written, &name, &split_values(value));
    }

    // Untouched children are preserved verbatim.
    let mut new_ilst_body = Vec::new();
    for (name, body) in &old_ilst {
        let removed = if name == "----" {
            mp4_freeform_key(body).is_some_and(|key| removal.contains(&key))
        } else {
            removal.contains(name)
        };
        if !removed {
            new_ilst_body.extend_from_slice(&mp4_atom(&mp4_atom_name_bytes(name), body));
        }
    }
    for item in &written {
        new_ilst_body.extend_from_slice(&render_mp4_write(item));
    }

    // ------------------------------------------------------------------
    // Rebuild meta / udta / moov.
    // ------------------------------------------------------------------
    let mut new_meta_body = Vec::new();
    new_meta_body.extend_from_slice(&meta_prefix);
    let mut has_hdlr = false;
    for (name, payload) in &meta_children {
        if name == b"ilst" {
            continue;
        }
        if name == b"hdlr" {
            has_hdlr = true;
        }
        new_meta_body.extend_from_slice(&mp4_atom(name, payload));
    }
    if !has_hdlr {
        new_meta_body.extend_from_slice(&mp4_default_hdlr());
    }
    new_meta_body.extend_from_slice(&mp4_atom(b"ilst", &new_ilst_body));
    let new_meta = mp4_atom(b"meta", &new_meta_body);

    let mut new_udta_body = Vec::new();
    for (name, payload) in &udta_children {
        if name == b"meta" {
            continue;
        }
        new_udta_body.extend_from_slice(&mp4_atom(name, payload));
    }
    new_udta_body.extend_from_slice(&new_meta);
    let new_udta = mp4_atom(b"udta", &new_udta_body);

    let mut new_moov_body = Vec::new();
    let mut wrote_udta = false;
    for (name, payload) in &moov_children {
        if name == b"udta" {
            new_moov_body.extend_from_slice(&new_udta);
            wrote_udta = true;
            continue;
        }
        new_moov_body.extend_from_slice(&mp4_atom(name, payload));
    }
    if !wrote_udta {
        new_moov_body.extend_from_slice(&new_udta);
    }
    let new_moov_atom = mp4_atom(b"moov", &new_moov_body);

    // ------------------------------------------------------------------
    // Splice into the file; pad or patch chunk offsets as needed.
    // ------------------------------------------------------------------
    let old_atom_len = moov_end - moov_start;
    let mut out = Vec::with_capacity(data.len() + new_moov_atom.len());
    out.extend_from_slice(&data[..moov_start]);
    out.extend_from_slice(&new_moov_atom);
    let mut delta: i64 = new_moov_atom.len() as i64 - old_atom_len as i64;
    if delta < 0 {
        // A `free` atom keeps the total length stable so chunk offsets stay
        // valid (minimum atom size is 8 bytes).
        let free_size = (-delta) as usize;
        let free_size = free_size.max(8);
        out.extend_from_slice(&mp4_atom(b"free", &vec![0u8; free_size - 8]));
        delta += free_size as i64;
    }
    out.extend_from_slice(&data[moov_end..]);
    if delta > 0 {
        let payload_len = new_moov_atom.len() - 8;
        let payload_start = moov_start + 8;
        patch_mp4_offsets(
            &mut out[payload_start..payload_start + payload_len],
            delta as u64,
            moov_end,
        );
    }
    Ok(out)
}

/// Returns (atom end, name) for the atom at `offset`, handling 64-bit sizes.
fn mp4_atom_at(data: &[u8], offset: usize) -> Option<(usize, [u8; 4])> {
    if offset + 8 > data.len() {
        return None;
    }
    let mut size = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as u64;
    let name: [u8; 4] = data[offset + 4..offset + 8].try_into().ok()?;
    let mut header = 8usize;
    if size == 1 {
        if offset + 16 > data.len() {
            return None;
        }
        size = u64::from_be_bytes([
            data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11],
            data[offset + 12], data[offset + 13], data[offset + 14], data[offset + 15],
        ]);
        header = 16;
    }
    if size < header as u64 {
        return None;
    }
    let end = (offset as u64 + size).min(data.len() as u64) as usize;
    Some((end, name))
}

/// Walks the atoms of a container payload, yielding (name, payload).
fn mp4_children(data: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut children = Vec::new();
    let mut offset = 0usize;
    while offset + 8 <= data.len() {
        let Some((end, name)) = mp4_atom_at(data, offset) else {
            break;
        };
        let header = if end - offset >= 16 && u32::from_be_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
        ]) == 1
        {
            16
        } else {
            8
        };
        children.push((name, data[offset + header..end].to_vec()));
        if end <= offset {
            break;
        }
        offset = end;
    }
    children
}

/// Parses ilst children into (name, payload) with the © byte decoded.
fn ilst_children(ilst: &[u8]) -> Vec<(String, Vec<u8>)> {
    mp4_children(ilst)
        .into_iter()
        .map(|(name, payload)| (decode_ilst_name(&name), payload))
        .collect()
}

/// Existing trkn/disk binary pair (number, total).
fn mp4_existing_pair(children: &[(String, Vec<u8>)], name: &str) -> (Option<u32>, Option<u32>) {
    for (child, body) in children {
        if child != name {
            continue;
        }
        for payload in mp4_data_payloads(body) {
            if payload.len() >= 6 {
                let first = u16::from_be_bytes([payload[2], payload[3]]) as u32;
                let second = u16::from_be_bytes([payload[4], payload[5]]) as u32;
                return (Some(first), Some(second));
            }
        }
    }
    (None, None)
}

fn mp4_push_text(written: &mut Vec<Mp4Write>, name: &str, values: &[String]) {
    if values.iter().any(|v| !v.is_empty()) {
        written.push(Mp4Write::Text {
            name: name.to_string(),
            values: values.to_vec(),
        });
    }
}

/// Renders a queued atom. Freeform names (`----:com.apple.iTunes:KEY`) become
/// mean/name/data chains, everything else a plain text child.
fn render_mp4_write(item: &Mp4Write) -> Vec<u8> {
    match item {
        Mp4Write::Text { name, values } => {
            if let Some(key) = name.strip_prefix("----:com.apple.iTunes:") {
                let mut body = Vec::new();
                body.extend_from_slice(&mp4_mean_name_atom(b"mean", "com.apple.iTunes"));
                body.extend_from_slice(&mp4_mean_name_atom(b"name", key));
                for value in values {
                    body.extend_from_slice(&mp4_data_atom(MP4_TEXT_DATA, value.as_bytes()));
                }
                mp4_atom(b"----", &body)
            } else {
                let mut body = Vec::new();
                for value in values {
                    body.extend_from_slice(&mp4_data_atom(MP4_TEXT_DATA, value.as_bytes()));
                }
                mp4_atom(&mp4_atom_name_bytes(name), &body)
            }
        }
        Mp4Write::Track { number, total } => {
            let mut payload = vec![0u8; 8];
            payload[2] = (number >> 8) as u8;
            payload[3] = (number & 0xFF) as u8;
            payload[4] = (total >> 8) as u8;
            payload[5] = (total & 0xFF) as u8;
            mp4_atom(b"trkn", &mp4_data_atom(0, &payload))
        }
        Mp4Write::Cover { image, mime } => {
            let data_type = match mime.as_str() {
                "image/jpeg" => MP4_JPEG_DATA,
                "image/png" => MP4_PNG_DATA,
                _ => 0,
            };
            mp4_atom(b"covr", &mp4_data_atom(data_type, image))
        }
    }
}

/// ilst child atom name: ASCII with an optional leading © stored as 0xA9.
fn mp4_atom_name_bytes(name: &str) -> Vec<u8> {
    if let Some(rest) = name.strip_prefix('\u{a9}') {
        let mut bytes = vec![0xA9u8];
        bytes.extend_from_slice(rest.as_bytes());
        bytes
    } else {
        name.as_bytes().to_vec()
    }
}

fn mp4_atom(name: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut atom = Vec::with_capacity(payload.len() + 8);
    atom.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    atom.extend_from_slice(name);
    atom.extend_from_slice(payload);
    atom
}

/// `data` sub-atom: size + 'data' + data type + locale + payload.
fn mp4_data_atom(data_type: u32, payload: &[u8]) -> Vec<u8> {
    let mut atom = Vec::with_capacity(payload.len() + 16);
    atom.extend_from_slice(&((payload.len() + 16) as u32).to_be_bytes());
    atom.extend_from_slice(b"data");
    atom.extend_from_slice(&data_type.to_be_bytes());
    atom.extend_from_slice(&0u32.to_be_bytes()); // locale
    atom.extend_from_slice(payload);
    atom
}

/// mean/name sub-atom: size + type + 4-byte version/flags + text.
fn mp4_mean_name_atom(kind: &[u8], text: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + text.len());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(text.as_bytes());
    mp4_atom(kind, &payload)
}

/// iTunes metadata handler atom ('mdir'/'appl').
fn mp4_default_hdlr() -> Vec<u8> {
    let mut payload = Vec::with_capacity(25);
    payload.extend_from_slice(&0u32.to_be_bytes()); // version/flags
    payload.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    payload.extend_from_slice(b"mdir");
    payload.extend_from_slice(b"appl");
    payload.extend_from_slice(&[0u8; 8]); // reserved
    payload.push(0); // empty name
    mp4_atom(b"hdlr", &payload)
}

/// Adds `delta` to every chunk offset (stco/co64) at or beyond `threshold`,
/// walking the container atoms of a moov payload in place.
fn patch_mp4_offsets(payload: &mut [u8], delta: u64, threshold: usize) {
    let mut offset = 0usize;
    while offset + 8 <= payload.len() {
        let Some((end, name)) = mp4_atom_at(payload, offset) else {
            break;
        };
        if end <= offset + 8 {
            break;
        }
        let (body_start, body_end) = (offset + 8, end);
        match &name {
            b"trak" | b"mdia" | b"minf" | b"stbl" => {
                patch_mp4_offsets(&mut payload[body_start..body_end], delta, threshold);
            }
            b"stco" | b"co64" => {
                let count_start = body_start + 4;
                if count_start + 4 <= body_end {
                    let count = u32::from_be_bytes([
                        payload[count_start], payload[count_start + 1],
                        payload[count_start + 2], payload[count_start + 3],
                    ]) as usize;
                    let width = if name == *b"stco" { 4 } else { 8 };
                    let table = body_start + 8;
                    for index in 0..count {
                        let at = table + index * width;
                        if at + width > body_end {
                            break;
                        }
                        if width == 4 {
                            let value = u32::from_be_bytes([
                                payload[at], payload[at + 1], payload[at + 2], payload[at + 3],
                            ]);
                            if value as usize >= threshold {
                                let shifted = (value as u64 + delta).min(u32::MAX as u64) as u32;
                                payload[at..at + 4].copy_from_slice(&shifted.to_be_bytes());
                            }
                        } else {
                            let value = u64::from_be_bytes([
                                payload[at], payload[at + 1], payload[at + 2], payload[at + 3],
                                payload[at + 4], payload[at + 5], payload[at + 6], payload[at + 7],
                            ]);
                            if value as usize >= threshold {
                                let shifted = value + delta;
                                payload[at..at + 8].copy_from_slice(&shifted.to_be_bytes());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        offset = end;
    }
}




