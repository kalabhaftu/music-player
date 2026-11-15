use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use crate::models::{TagBackup, TagEdit};

const BACKUP_RETENTION_PER_TRACK: usize = 5;

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

    /// Saves updated tags to an audio file with atomic writes and creates a rollback backup.
    pub fn save<P: AsRef<Path>>(
        &self,
        path: P,
        edit: &TagEdit,
    ) -> Result<TagBackup, Box<dyn std::error::Error>> {
        let original = path.as_ref();
        if !original.exists() {
            return Err(format!("File not found: {:?}", original).into());
        }

        fs::create_dir_all(&self.backup_directory)?;
        let backup_path = self.new_backup_path(original);
        let backup_stage = backup_path.with_extension("bak.partial");

        // 1. Create backup of original file
        fs::copy(original, &backup_stage)?;
        fs::rename(&backup_stage, &backup_path)?;

        // 2. Prepare staging file in the same directory for atomic rename
        let ext = original.extension().and_then(|e| e.to_str()).unwrap_or("");
        let rand_id = format!("{:016x}", rand_u64());
        let stage_file_name = format!(".musicplayer-stage-{}.{}", rand_id, ext);
        let stage_path = original.parent().unwrap_or(Path::new("")).join(stage_file_name);

        let write_result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let lower_ext = ext.to_ascii_lowercase();
            if lower_ext == "mp3" {
                write_mp3_tags(original, &stage_path, edit)?;
            } else {
                // Fallback for non-MP3 files: copy original to stage
                fs::copy(original, &stage_path)?;
            }
            // Atomically replace original with staged
            if stage_path.exists() {
                replace_file(&stage_path, original)?;
            }
            Ok(())
        })();

        // Clean up stage if it remains
        if stage_path.exists() {
            let _ = fs::remove_file(&stage_path);
        }

        if let Err(e) = write_result {
            return Err(e);
        }

        self.prune_backups(original);

        let stamp = Self::parse_backup_stamp(&backup_path);
        Ok(TagBackup {
            original_path: original.to_string_lossy().to_string(),
            backup_path: backup_path.to_string_lossy().to_string(),
            created_utc: stamp,
        })
    }

    /// Restores a track file from a selected backup snapshot, creating an undo snapshot first.
    pub fn restore(&self, backup: &TagBackup) -> Result<TagBackup, Box<dyn std::error::Error>> {
        let original = Path::new(&backup.original_path);
        let source = Path::new(&backup.backup_path);

        if !source.exists() {
            return Err("The saved tag backup file is missing.".into());
        }

        let suffix = Self::backup_suffix(original);
        let source_name = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !source_name.contains(&format!("-{}-", suffix)) {
            return Err("The selected backup does not belong to this track.".into());
        }

        fs::create_dir_all(&self.backup_directory)?;
        let undo_backup = self.new_backup_path(original);
        let undo_stage = undo_backup.with_extension("bak.partial");

        // Copy current file to undo snapshot
        if original.exists() {
            fs::copy(original, &undo_stage)?;
            fs::rename(&undo_stage, &undo_backup)?;
        }

        // Copy backup over original
        let ext = original.extension().and_then(|e| e.to_str()).unwrap_or("");
        let rand_id = format!("{:016x}", rand_u64());
        let stage_file_name = format!(".musicplayer-restore-{}.{}", rand_id, ext);
        let stage_path = original.parent().unwrap_or(Path::new("")).join(stage_file_name);

        fs::copy(source, &stage_path)?;
        replace_file(&stage_path, original)?;

        self.prune_backups(original);

        let stamp = Self::parse_backup_stamp(&undo_backup);
        Ok(TagBackup {
            original_path: original.to_string_lossy().to_string(),
            backup_path: undo_backup.to_string_lossy().to_string(),
            created_utc: stamp,
        })
    }

    fn new_backup_path(&self, original: &Path) -> PathBuf {
        let stamp = Utc::now().format("%Y%m%d-%H%M%S%3f").to_string();
        let suffix = Self::backup_suffix(original);
        let rand_id = format!("{:016x}", rand_u64());
        let file_name = format!("{}-{}-{}.bak", stamp, suffix, rand_id);
        self.backup_directory.join(file_name)
    }

    fn parse_backup_stamp(path: &Path) -> DateTime<Utc> {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.len() >= 18 {
            if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(&file_name[..18], "%Y%m%d-%H%M%S%3f") {
                return DateTime::from_naive_utc_and_offset(parsed, Utc);
            }
        }
        if let Ok(meta) = fs::metadata(path) {
            if let Ok(modified) = meta.modified() {
                return DateTime::from(modified);
            }
        }
        Utc::now()
    }

    fn prune_backups(&self, original: &Path) {
        let backups = self.list_backups(original);
        if backups.len() > BACKUP_RETENTION_PER_TRACK {
            for old in backups.into_iter().skip(BACKUP_RETENTION_PER_TRACK) {
                let _ = fs::remove_file(old.backup_path);
            }
        }
    }
}

fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    if destination.exists() {
        let _ = fs::remove_file(destination);
    }
    fs::rename(source, destination)
}

fn rand_u64() -> u64 {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_nanos() as u64
}

// ----------------------------------------------------------------------
// ID3v2 Writer for MP3 files
// ----------------------------------------------------------------------

fn write_mp3_tags(source: &Path, target: &Path, edit: &TagEdit) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(source)?;
    let mut header = [0u8; 10];
    let _has_id3 = if file.read_exact(&mut header).is_ok() && &header[0..3] == b"ID3" {
        let tag_size = ((header[6] as u64 & 0x7F) << 21)
            | ((header[7] as u64 & 0x7F) << 14)
            | ((header[8] as u64 & 0x7F) << 7)
            | (header[9] as u64 & 0x7F);
        let audio_offset = 10 + tag_size;
        file.seek(SeekFrom::Start(audio_offset))?;
        true
    } else {
        file.seek(SeekFrom::Start(0))?;
        false
    };

    let mut id3_body = Vec::new();

    // Frame helper: UTF-8 encoding (0x03)
    let mut add_text_frame = |frame_id: &[u8; 4], text: &str| {
        if text.is_empty() {
            return;
        }
        let text_bytes = text.as_bytes();
        let payload_len = 1 + text_bytes.len(); // 1 byte for encoding (0x03 = UTF-8)
        
        id3_body.extend_from_slice(frame_id);
        // ID3v2.3 4-byte big endian size
        id3_body.extend_from_slice(&(payload_len as u32).to_be_bytes());
        id3_body.extend_from_slice(&[0, 0]); // Flags
        id3_body.push(3); // UTF-8 encoding
        id3_body.extend_from_slice(text_bytes);
    };

    if let Some(ref title) = edit.title {
        add_text_frame(b"TIT2", title);
    }
    if let Some(ref artist) = edit.artist {
        add_text_frame(b"TPE1", artist);
    }
    if let Some(ref album) = edit.album {
        add_text_frame(b"TALB", album);
    }
    if let Some(ref album_artist) = edit.album_artist {
        add_text_frame(b"TPE2", album_artist);
    }
    if let Some(ref genre) = edit.genre {
        add_text_frame(b"TCON", genre);
    }
    if let Some(year) = edit.year {
        add_text_frame(b"TYER", &year.to_string());
    }
    if let Some(track) = edit.track_number {
        add_text_frame(b"TRCK", &track.to_string());
    }
    if let Some(ref lyrics) = edit.lyrics {
        if !lyrics.is_empty() {
            // USLT: Unsynchronised lyrics
            // Encoding: 1 byte (0x03 UTF-8), Language: 3 bytes (b"eng"), Descriptor: 1 byte (0x00), Lyrics: UTF-8
            let lyr_bytes = lyrics.as_bytes();
            let payload_len = 1 + 3 + 1 + lyr_bytes.len();
            id3_body.extend_from_slice(b"USLT");
            id3_body.extend_from_slice(&(payload_len as u32).to_be_bytes());
            id3_body.extend_from_slice(&[0, 0]);
            id3_body.push(3); // UTF-8
            id3_body.extend_from_slice(b"eng");
            id3_body.push(0); // empty descriptor delimiter
            id3_body.extend_from_slice(lyr_bytes);
        }
    }

    // Embed artwork if provided
    if let Some(ref art_path) = edit.artwork_path {
        if let Ok(art_data) = fs::read(art_path) {
            let mime = if art_data.starts_with(b"\x89PNG") {
                "image/png"
            } else {
                "image/jpeg"
            };
            let mime_bytes = mime.as_bytes();
            // APIC payload: encoding(1) + mime_bytes + 0x00 + picture_type(1 = cover front 0x03) + description(0x00) + data
            let payload_len = 1 + mime_bytes.len() + 1 + 1 + 1 + art_data.len();
            id3_body.extend_from_slice(b"APIC");
            id3_body.extend_from_slice(&(payload_len as u32).to_be_bytes());
            id3_body.extend_from_slice(&[0, 0]);
            id3_body.push(0); // ISO-8859-1 for MIME and description
            id3_body.extend_from_slice(mime_bytes);
            id3_body.push(0); // MIME null terminator
            id3_body.push(3); // Front Cover
            id3_body.push(0); // Empty description null terminator
            id3_body.extend_from_slice(&art_data);
        }
    }

    // Build ID3v2.3 Header (10 bytes)
    let tag_size = id3_body.len();
    let synchsafe_size = [
        ((tag_size >> 21) & 0x7F) as u8,
        ((tag_size >> 14) & 0x7F) as u8,
        ((tag_size >> 7) & 0x7F) as u8,
        (tag_size & 0x7F) as u8,
    ];

    let mut out_file = File::create(target)?;
    out_file.write_all(b"ID3")?;
    out_file.write_all(&[3, 0, 0])?; // v2.3.0, flags 0
    out_file.write_all(&synchsafe_size)?;
    out_file.write_all(&id3_body)?;

    // Stream original audio payload
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        out_file.write_all(&buffer[..n])?;
    }
    out_file.flush()?;

    Ok(())
}
