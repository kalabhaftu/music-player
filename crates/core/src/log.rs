//! Local application log — 1:1 port of `LocalAppLog.cs`.
//!
//! Layout: `%LOCALAPPDATA%\MusicPlayer\Logs\music-player-{yyyy-MM-dd}.log`
//! Entries: `{UTC:O} [{INFO|WARN|ERROR}] [{area}] {message}` (+ exception).
//! Roll: file rolls to `.1` when a write would exceed 4 MiB.
//! Retention: files older than 14 days deleted; 50 MiB total cap enforced
//! oldest-first, at most once per UTC day. Export writes a ZIP of all
//! `music-player-*.log*` files.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use chrono::{DateTime, Local, Utc};

const LOG_PREFIX: &str = "music-player-";
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const RETENTION_DAYS: i64 = 14;
const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

pub struct LocalAppLog {
    folder: PathBuf,
    state: Mutex<LogState>,
}

struct LogState {
    last_cleanup: Option<DateTime<Utc>>,
}

impl LocalAppLog {
    pub fn shared() -> &'static LocalAppLog {
        static SHARED: OnceLock<LocalAppLog> = OnceLock::new();
        SHARED.get_or_init(|| {
            let folder = crate::metadata::app_data_root().join("Logs");
            LocalAppLog {
                folder,
                state: Mutex::new(LogState { last_cleanup: None }),
            }
        })
    }

    pub fn with_folder(folder: PathBuf) -> LocalAppLog {
        LocalAppLog {
            folder,
            state: Mutex::new(LogState { last_cleanup: None }),
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn info(&self, area: &str, message: &str) {
        self.write(LogLevel::Info, area, message, None);
    }

    pub fn warning(&self, area: &str, message: &str) {
        self.write(LogLevel::Warning, area, message, None);
    }

    pub fn error(&self, area: &str, message: &str, err: Option<&dyn std::fmt::Display>) {
        self.write(
            LogLevel::Error,
            area,
            message,
            err.map(|e| e.to_string()),
        );
    }

    fn write(&self, level: LogLevel, area: &str, message: &str, err: Option<String>) {
        let mut entry = format!(
            "{} [{}] [{}] {}\n",
            Utc::now().to_rfc3339(),
            level.label(),
            area,
            message
        );
        if let Some(detail) = err {
            entry.push_str(&detail);
            entry.push('\n');
        }
        if entry.len() > MAX_BYTES as usize {
            entry.truncate(MAX_BYTES as usize);
            entry.push_str("\n[log entry truncated]\n");
        }

        let _ = fs::create_dir_all(&self.folder);
        let stamp = Local::now().format("%Y-%m-%d");
        let path = self.folder.join(format!("{}{}.log", LOG_PREFIX, stamp));

        // Roll when the existing file would exceed the cap.
        if let Ok(meta) = fs::metadata(&path) {
            if meta.len() + entry.len() as u64 > MAX_BYTES {
                let rolled = self.folder.join(format!("{}{}.log.1", LOG_PREFIX, stamp));
                let _ = fs::rename(&path, &rolled);
            }
        }

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(entry.as_bytes());
        }

        self.enforce_retention();
    }

    fn enforce_retention(&self) {
        let mut state = match self.state.try_lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let now = Utc::now();
        if let Some(last) = state.last_cleanup {
            if (now - last).num_hours() < 20 {
                return;
            }
        }
        state.last_cleanup = Some(now);
        drop(state);

        let mut entries: Vec<(PathBuf, u64)> = Vec::new();
        if let Ok(dir) = fs::read_dir(&self.folder) {
            for item in dir.flatten() {
                let path = item.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with(LOG_PREFIX) || !name.ends_with(".log") && !name.contains(".log.") {
                    continue;
                }
                if let Ok(meta) = item.metadata() {
                    let modified = meta
                        .modified()
                        .ok()
                        .map(DateTime::<Utc>::from)
                        .unwrap_or(now);
                    if (now - modified).num_days() > RETENTION_DAYS {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    entries.push((path, meta.len()));
                }
            }
        }

        // Oldest first.
        entries.sort_by_key(|(path, _)| {
            fs::metadata(path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        });

        let mut total: u64 = entries.iter().map(|(_, size)| *size).sum();
        for (path, size) in entries {
            if total <= MAX_TOTAL_BYTES {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    /// Zips every `music-player-*.log*` file into `dest` (stored, no compression).
    pub fn export_to(&self, dest: &Path) -> std::io::Result<()> {
        if dest.extension().and_then(|e| e.to_str()) != Some("zip") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "log export destination must be a .zip file",
            ));
        }
        if dest.starts_with(&self.folder) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "log export destination must be outside the log folder",
            ));
        }

        let mut files: Vec<PathBuf> = Vec::new();
        if let Ok(dir) = fs::read_dir(&self.folder) {
            for item in dir.flatten() {
                let path = item.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if (name.starts_with(LOG_PREFIX) && name.contains(".log")) || name == "crash.log" {
                    files.push(path);
                }
            }
        }
        files.sort();
        write_zip(dest, &self.folder, &files)
    }

    /// Installs a panic hook that logs any unhandled panics with backtrace info
    /// to `%LOCALAPPDATA%\MusicPlayer\Logs\crash.log`.
    pub fn install_panic_hook(&self) {
        let folder = self.folder.clone();
        let old_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let now = Utc::now().to_rfc3339();
            let crash_file = folder.join("crash.log");
            let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
                *s
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.as_str()
            } else {
                "Unknown panic payload"
            };
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown location".to_string());
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("unnamed");

            let report = format!(
                "\n==================== CRASH REPORT ====================\n\
                 Timestamp: {}\n\
                 Thread: {}\n\
                 Location: {}\n\
                 Message: {}\n\
                 ======================================================\n",
                now, thread_name, location, payload
            );

            let _ = std::fs::create_dir_all(&folder);
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&crash_file) {
                let _ = f.write_all(report.as_bytes());
            }

            old_hook(info);
        }));
    }
}

// ---------------------------------------------------------------------------
// Minimal stored ZIP writer (no external dependencies)
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, item) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            }
            *item = c;
        }
        table
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

struct ZipEntry {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
}

fn write_zip(dest: &Path, base: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    let tmp = dest.with_extension("zip.tmp");
    let mut out = File::create(&tmp)?;
    let mut entries: Vec<ZipEntry> = Vec::new();

    for file in files {
        let name = file
            .strip_prefix(base)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| file.file_name().unwrap_or_default().to_string_lossy().into_owned());
        let mut data = Vec::new();
        File::open(file)?.read_to_end(&mut data)?;

        let offset = out.stream_position()? as u32;
        let crc = crc32(&data);
        let size = data.len() as u32;

        out.write_all(&[0x50, 0x4B, 0x03, 0x04])?; // local file header
        out.write_all(&0u16.to_le_bytes())?; // version needed
        out.write_all(&0u16.to_le_bytes())?; // flags
        out.write_all(&0u16.to_le_bytes())?; // method: stored
        out.write_all(&0u16.to_le_bytes())?; // mod time
        out.write_all(&0x5900u16.to_le_bytes())?; // mod date (2024 placeholder)
        out.write_all(&crc.to_le_bytes())?;
        out.write_all(&size.to_le_bytes())?;
        out.write_all(&size.to_le_bytes())?;
        out.write_all(&(name.len() as u16).to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?; // extra len
        out.write_all(name.as_bytes())?;
        out.write_all(&data)?;

        entries.push(ZipEntry { name, crc, size, offset });
    }

    let central_offset = out.stream_position()? as u32;
    for entry in &entries {
        out.write_all(&[0x50, 0x4B, 0x01, 0x02])?;
        out.write_all(&0u16.to_le_bytes())?; // version made by
        out.write_all(&0u16.to_le_bytes())?; // version needed
        out.write_all(&0u16.to_le_bytes())?; // flags
        out.write_all(&0u16.to_le_bytes())?; // method
        out.write_all(&0u16.to_le_bytes())?; // time
        out.write_all(&0x5900u16.to_le_bytes())?;
        out.write_all(&entry.crc.to_le_bytes())?;
        out.write_all(&entry.size.to_le_bytes())?;
        out.write_all(&entry.size.to_le_bytes())?;
        out.write_all(&(entry.name.len() as u16).to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?; // comment
        out.write_all(&0u16.to_le_bytes())?; // disk
        out.write_all(&0u16.to_le_bytes())?; // internal attrs
        out.write_all(&0u32.to_le_bytes())?; // external attrs
        out.write_all(&entry.offset.to_le_bytes())?;
        out.write_all(entry.name.as_bytes())?;
    }
    let central_size = (out.stream_position()? as u32) - central_offset;

    out.write_all(&[0x50, 0x4B, 0x05, 0x06])?;
    out.write_all(&0u16.to_le_bytes())?; // disk
    out.write_all(&0u16.to_le_bytes())?; // cd disk
    out.write_all(&(entries.len() as u16).to_le_bytes())?;
    out.write_all(&(entries.len() as u16).to_le_bytes())?;
    out.write_all(&central_size.to_le_bytes())?;
    out.write_all(&central_offset.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.flush()?;
    drop(out);

    fs::rename(&tmp, dest)?;
    Ok(())
}
