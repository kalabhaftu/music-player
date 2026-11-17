//! Tag reading (TrackReader.cs + TagLib# parity). Reads every supported
//! container directly: ID3v1/v2, FLAC Xiph, MP4 ilst, Ogg/Opus, WAV (RIFF
//! INFO + embedded ID3), ASF/WMA, APEv2, AIFF, DSF, DFF, TTA, and ADTS AAC.
//!
//! Container naming follows TagLib's `MimeType` strings ("taglib/wav", ...)
//! so `TrackDetails` renders the same audio format rows as the original app.

use crate::metadata::artwork::{save_artwork_to_cache, save_artwork_to_cache_in};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default)]
pub struct ExtractedTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: u32,
    pub track_number: u32,
    pub duration_ms: u64,
    pub artwork_path: Option<String>,
    pub has_lyrics: bool,
}

/// `TrackInformation.Read` result (TrackDetails record).
#[derive(Debug, Clone)]
pub struct TrackDetails {
    pub path: String,
    pub container: String,
    pub duration_ms: u64,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub bits_per_sample: u32,
    pub file_size: u64,
    pub modified_utc: DateTime<Utc>,
}

/// Where extracted artwork bytes are written.
enum ArtSink<'a> {
    Global,
    Dir(&'a Path),
}

impl ArtSink<'_> {
    fn save(&self, data: &[u8]) -> Option<String> {
        if data.is_empty() {
            return None;
        }
        let target: PathBuf = match self {
            ArtSink::Global => save_artwork_to_cache(data).ok()?,
            ArtSink::Dir(dir) => {
                std::fs::create_dir_all(dir).ok()?;
                save_artwork_to_cache_in(dir, data).ok()?
            }
        };
        Some(target.to_string_lossy().to_string())
    }
}

fn file_ext(path: &Path) -> String {    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Multi-value display join (TagLib joins value lists with "; ").
fn join_values(values: &[String]) -> Option<String> {
    let joined = values
        .iter()
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn first_value(values: &[String]) -> Option<String> {
    values.iter().find(|v| !v.trim().is_empty()).cloned()
}

/// True when the text would be usable lyrics (Lyrics.HasUsableContent port).
fn has_usable_lyrics(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let doc = crate::lyrics::parse_lrc(text);
    doc.lines.iter().any(|l| !l.text.trim().is_empty())
        || !crate::lyrics::plain_text(text).trim().is_empty()
}

/// Reads tags for any supported container, saving artwork to the global cache.
pub fn read_tags<P: AsRef<Path>>(path: P) -> ExtractedTags {
    read_tags_with_sink(path.as_ref(), ArtSink::Global)
}

/// Same, but artwork is written under `cache_dir` (scan-session parity with
/// `TrackReader.Read(path, artworkCache)`).
pub fn read_tags_in_cache<P: AsRef<Path>>(path: P, cache_dir: &Path) -> ExtractedTags {
    read_tags_with_sink(path.as_ref(), ArtSink::Dir(cache_dir))
}

fn read_tags_with_sink(path: &Path, sink: ArtSink<'_>) -> ExtractedTags {
    match file_ext(path).as_str() {
        "dff" => ExtractedTags {
            duration_ms: crate::metadata::read_dff_duration_ms(path),
            ..Default::default()
        },
        "mp3" => read_id3_container(path, sink).unwrap_or_default(),
        "flac" => read_flac_file(path, sink).unwrap_or_default(),
        "m4a" | "mp4" | "m4b" => read_mp4_file(path, sink).unwrap_or_default(),
        "ogg" | "oga" | "opus" => read_ogg_file(path, sink).unwrap_or_default(),
        "wav" | "wave" => read_wav_file(path, sink).unwrap_or_default(),
        "wma" => read_asf_file(path, sink).unwrap_or_default(),
        "aif" | "aiff" => read_aiff_file(path, sink).unwrap_or_default(),
        "ape" | "wv" | "mpc" => read_ape_container(path, sink).unwrap_or_default(),
        "dsf" => read_dsf_file(path, sink).unwrap_or_default(),
        "tta" => read_tta_file(path, sink).unwrap_or_default(),
        "aac" => read_aac_file(path, sink).unwrap_or_default(),
        _ => ExtractedTags::default(),
    }
}

/// `TrackInformation.Read` port: container + duration + audio quality.
pub fn inspect_file(path: &Path) -> Option<TrackDetails> {
    let info = std::fs::metadata(path).ok()?;
    let full = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let modified: DateTime<Utc> = info.modified().map(DateTime::from).unwrap_or_else(|_| Utc::now());
    let ext = file_ext(path);
    let size = info.len();
    let quality = match ext.as_str() {
        "dff" => {
            let duration = crate::metadata::read_dff_duration_ms(path);
            Quality {
                container: "taglib/dff",
                duration_ms: duration,
                bitrate_kbps: bitrate_from_size(size, duration),
                sample_rate_hz: 0,
                bits_per_sample: 0,
            }
        }
        "mp3" => {
            let (duration, bitrate) = mp3_duration_and_bitrate(path);
            Quality {
                container: "taglib/mpeg",
                duration_ms: duration,
                bitrate_kbps: bitrate,
                sample_rate_hz: mp3_sample_rate(path).unwrap_or(0),
                bits_per_sample: 0,
            }
        }
        "flac" => {
            let q = flac_quality(path, size);
            Quality {
                container: "taglib/x-flac",
                ..q
            }
        }
        "m4a" | "mp4" | "m4b" => {
            let q = mp4_quality(path, size);
            Quality {
                container: "taglib/mp4",
                ..q
            }
        }
        "ogg" | "oga" => {
            let q = ogg_quality(path, size);
            Quality {
                container: "taglib/ogg",
                ..q
            }
        }
        "opus" => {
            let q = ogg_quality(path, size);
            Quality {
                container: "taglib/opus",
                ..q
            }
        }
        "wav" | "wave" => {
            let q = wav_quality(path);
            Quality {
                container: "taglib/wav",
                ..q
            }
        }
        "wma" => {
            let q = asf_quality(path);
            Quality {
                container: "taglib/asf",
                ..q
            }
        }
        "aif" | "aiff" => {
            let q = aiff_quality(path);
            Quality {
                container: "taglib/aiff",
                ..q
            }
        }
        "dsf" => {
            let q = dsf_quality(path);
            Quality {
                container: "taglib/dsf",
                ..q
            }
        }
        "tta" => {
            let q = tta_quality(path);
            Quality {
                container: "taglib/tta",
                ..q
            }
        }
        "aac" => {
            let data = std::fs::read(path).unwrap_or_default();
            let (duration, bitrate, sample_rate) = aac_adts_quality(&data);
            Quality {
                container: "taglib/aac",
                duration_ms: duration,
                bitrate_kbps: bitrate,
                sample_rate_hz: sample_rate,
                bits_per_sample: 0,
            }
        }
        "ape" => Quality {
            container: "taglib/ape",
            ..Default::default()
        },
        "wv" => {
            let q = wavpack_quality(path);
            Quality {
                container: "taglib/wavpack",
                ..q
            }
        }
        "mpc" => Quality {
            container: "taglib/mpc",
            ..Default::default()
        },
        _ => return None,
    };
    Some(TrackDetails {
        path: full,
        container: quality.container.to_string(),
        duration_ms: quality.duration_ms,
        bitrate_kbps: quality.bitrate_kbps,
        sample_rate_hz: quality.sample_rate_hz,
        bits_per_sample: quality.bits_per_sample,
        file_size: size,
        modified_utc: modified,
    })
}

#[derive(Debug, Clone, Default)]
struct Quality {
    container: &'static str,
    duration_ms: u64,
    bitrate_kbps: u32,
    sample_rate_hz: u32,
    bits_per_sample: u32,
}

fn bitrate_from_size(size: u64, duration_ms: u64) -> u32 {
    if duration_ms == 0 {
        0
    } else {
        ((size * 8).saturating_mul(1000) / duration_ms / 1000) as u32
    }
}

// ======================================================================
// ID3 (MP3, WAV, AIFF, AAC, TTA, DSF, DFF embedded tags)
// ======================================================================

/// All text frames of one ID3 tag (frame id → values), plus specials.
#[derive(Debug, Default)]
struct Id3View {
    text: HashMap<String, Vec<String>>, // upper-case frame id → values
    user_text: Vec<(String, Vec<String>)>, // TXXX (description, values)
    uslt: Vec<(String, Vec<String>)>,   // (descriptor, text) — first with text used
    sylt: Option<String>,               // formatted LRC when the format is absolute ms
    pictures: Vec<Vec<u8>>,
}

impl Id3View {
    fn frame(&self, id: &str) -> Vec<String> {
        self.text.get(id).cloned().unwrap_or_default()
    }
}

fn id3_value_split(payload: &[u8], encoding: u8) -> Vec<String> {
    decode_id3_strings(payload, encoding)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .collect()
}

/// Decodes an ID3 string payload, splitting on NUL terminators.
pub(crate) fn decode_id3_strings(payload: &[u8], encoding: u8) -> Vec<String> {
    match encoding {
        0 | 3 => payload
            .split(|&b| b == 0)
            .map(|part| decode_id3_text_raw(part, encoding))
            .collect(),
        1 | 2 => {
            // UTF-16: split on U+0000 pairs (respect alignment), decode each part.
            let mut out = Vec::new();
            let mut start = 0usize;
            let mut i = 0usize;
            let wide = encoding == 1 && payload.len() >= 2 && payload[0] == 0xFE && payload[1] == 0xFF;
            while i + 1 < payload.len() {
                if payload[i] == 0 && payload[i + 1] == 0 {
                    // A lone U+0000 terminator. For UTF-16LE text the preceding
                    // byte is part of the previous char only when it is 0x00
                    // itself, which the decoder below ignores anyway.
                    let end = i;
                    // The terminator is the u16 pair; the code unit at `i`
                    // belongs to the terminator.
                    let unit_len = if i + 1 < payload.len() { 2 } else { 1 };
                    let slice = &payload[start..end];
                    out.push(decode_utf16(slice, wide));
                    i += unit_len;
                    start = i;
                } else {
                    i += 2;
                }
            }
            if start < payload.len() {
                out.push(decode_utf16(&payload[start..], wide));
            }
            out.into_iter()
                .flat_map(|s| s.split('\u{0}').map(|p| p.to_string()).collect::<Vec<_>>())
                .collect()
        }
        _ => Vec::new(),
    }
}

fn decode_id3_text_raw(bytes: &[u8], encoding: u8) -> String {
    match encoding {
        3 => String::from_utf8_lossy(bytes).replace('\u{0}', ""),
        _ => bytes
            .iter()
            .filter(|&&b| b != 0)
            .map(|&b| b as char)
            .collect::<String>(),
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let unit = if big_endian {
            u16::from_be_bytes([bytes[i], bytes[i + 1]])
        } else {
            u16::from_le_bytes([bytes[i], bytes[i + 1]])
        };
        units.push(unit);
        i += 2;
    }
    String::from_utf16_lossy(&units).trim_matches('\u{0}').to_string()
}

/// Parses one ID3v2 tag payload (bytes after the 10-byte header).
fn parse_id3v2_payload(data: &[u8], version: u8) -> Id3View {
    let mut view = Id3View::default();
    let mut cursor = 0usize;

    // Extended header (flag bit 6).
    if cursor + 6 <= data.len() && data[0] == 0 {
        // No cheap way to know without flags; handled by caller.
    }

    let v22 = version == 2;
    let frame_header_len = if v22 { 6 } else { 10 };

    while cursor + frame_header_len <= data.len() {
        if data[cursor] == 0 {
            break; // padding
        }
        let id_raw = if v22 {
            std::str::from_utf8(&data[cursor..cursor + 3]).unwrap_or("").to_string()
        } else {
            std::str::from_utf8(&data[cursor..cursor + 4]).unwrap_or("").to_string()
        };
        let (frame_size, flags_raw): (usize, [u8; 2]) = if v22 {
            let s = (data[cursor + 3] as usize) << 16 | (data[cursor + 4] as usize) << 8 | data[cursor + 5] as usize;
            (s, [0, 0])
        } else {
            let size_bytes = &data[cursor + 4..cursor + 8];
            let s = if version == 4 {
                ((size_bytes[0] as usize & 0x7F) << 21)
                    | ((size_bytes[1] as usize & 0x7F) << 14)
                    | ((size_bytes[2] as usize & 0x7F) << 7)
                    | (size_bytes[3] as usize & 0x7F)
            } else {
                u32::from_be_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]) as usize
            };
            (s, [data[cursor + 8], data[cursor + 9]])
        };
        cursor += frame_header_len;
        if frame_size == 0 || cursor + frame_size > data.len() {
            break;
        }
        let content = &data[cursor..cursor + frame_size];
        cursor += frame_size;

        // Some v2.3 writers store non-synchsafe sizes inside v2.4-style tags;
        // skip implausible frames instead of aborting.
        let id = normalize_v22_frame_id(&id_raw);
        match id.as_str() {
            "APIC" | "PIC" => {
                if let Some(pic) = extract_apic_image(content) {
                    view.pictures.push(pic);
                }
            }
            "TXXX" | "TXX" => {
                if let Some((desc, values)) = extract_user_text(content) {
                    view.user_text.push((desc, values));
                }
            }
            "USLT" | "ULT" => {
                if let Some((desc, text)) = extract_uslt(content) {
                    view.uslt.push((desc, vec![text]));
                }
            }
            "SYLT" | "SLT" => {
                if let Some(lrc) = extract_sylt(content) {
                    view.sylt = Some(lrc);
                }
            }
            _ => {
                if content.is_empty() {
                    continue;
                }
                let encoding = content[0];
                let values = id3_value_split(&content[1..], encoding);
                if !values.is_empty() {
                    *view.text.entry(id).or_default() = values;
                }
            }
        }
        let _ = flags_raw;
    }
    view
}

pub(crate) fn normalize_v22_frame_id(id: &str) -> String {
    // Map a few common v2.2 ids to their v2.3 equivalents.
    match id {
        "TT2" => "TIT2".to_string(),
        "TP1" => "TPE1".to_string(),
        "TP2" => "TPE2".to_string(),
        "TAL" => "TALB".to_string(),
        "TCO" => "TCON".to_string(),
        "TYE" => "TYER".to_string(),
        "TRK" => "TRCK".to_string(),
        "TPA" => "TPOS".to_string(),
        "TCM" => "TCOM".to_string(),
        "TPB" => "TPUB".to_string(),
        "TCR" => "TCOP".to_string(),
        "TKE" => "TKEY".to_string(),
        "TOT" => "TOAL".to_string(),
        "TSO" => "TSOC".to_string(),
        _ => id.to_string(),
    }
}

fn extract_user_text(content: &[u8]) -> Option<(String, Vec<String>)> {
    if content.is_empty() {
        return None;
    }
    let encoding = content[0];
    let (description, rest) = split_id3_string(&content[1..], encoding)?;
    let values = id3_value_split(rest, encoding);
    if values.is_empty() {
        return None;
    }
    Some((description, values))
}

/// Splits a leading encoding-aware NUL-terminated string off the payload.
pub(crate) fn split_id3_string(payload: &[u8], encoding: u8) -> Option<(String, &[u8])> {
    match encoding {
        0 | 3 => {
            let end = payload.iter().position(|&b| b == 0)?;
            let text = decode_id3_text_raw(&payload[..end], encoding);
            Some((text, &payload[end + 1..]))
        }
        1 | 2 => {
            // Find the 00 00 terminator on 2-byte alignment.
            let mut i = 0usize;
            while i + 1 < payload.len() {
                if payload[i] == 0 && payload[i + 1] == 0 {
                    let wide = encoding == 1 && payload.len() >= 2 && payload[0] == 0xFE && payload[1] == 0xFF;
                    let text = decode_utf16(&payload[..i], wide);
                    return Some((text, &payload[i + 2..]));
                }
                i += 2;
            }
            None
        }
        _ => None,
    }
}

fn extract_uslt(content: &[u8]) -> Option<(String, String)> {
    if content.is_empty() {
        return None;
    }
    let encoding = content[0];
    if content.len() < 5 {
        return None;
    }
    let rest = &content[4..]; // skip language
    let (descriptor, tail) = split_id3_string(rest, encoding)?;
    let text = decode_id3_strings(tail, encoding).join("");
    if text.trim().is_empty() {
        return None;
    }
    Some((descriptor, text))
}

/// SYLT (format byte 2 == absolute milliseconds) → formatted LRC text.
fn extract_sylt(content: &[u8]) -> Option<String> {
    if content.len() < 7 {
        return None;
    }
    let encoding = content[0];
    let format = content[4]; // 1 = MPEG frames, 2 = absolute ms
    if format != 2 {
        return None;
    }
    let rest = &content[5..];
    let (_descriptor, tail) = split_id3_string(rest, encoding)?;
    let mut lines: Vec<(u64, String)> = Vec::new();
    let mut cursor = 0usize;
    while cursor + 4 <= tail.len() {
        let time = u32::from_be_bytes([tail[cursor], tail[cursor + 1], tail[cursor + 2], tail[cursor + 3]]) as u64;
        cursor += 4;
        let (text, next) = split_id3_string(&tail[cursor..], encoding)?;
        cursor = tail.len() - next.len();
        if !text.trim().is_empty() {
            lines.push((time, text));
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.sort_by_key(|(t, _)| *t);
    let doc = crate::models::LyricsDocument {
        lines: lines
            .into_iter()
            .map(|(t, text)| crate::models::LyricsLine { time_ms: t, text })
            .collect(),
        offset_ms: 0,
        metadata: Default::default(),
    };
    Some(crate::lyrics::format_lrc(&doc))
}

fn extract_apic_image(content: &[u8]) -> Option<Vec<u8>> {
    if content.is_empty() {
        return None;
    }
    let encoding = content[0];
    let mime_end = content[1..].iter().position(|&b| b == 0)? + 1;
    let mut offset = 1 + mime_end;
    if offset >= content.len() {
        return None;
    }
    offset += 1; // picture type
    if encoding == 1 || encoding == 2 {
        while offset + 1 < content.len() {
            if content[offset] == 0 && content[offset + 1] == 0 {
                offset += 2;
                break;
            }
            offset += 2;
        }
    } else {
        let desc_end = content[offset..].iter().position(|&b| b == 0)?;
        offset += desc_end + 1;
    }
    if offset < content.len() {
        Some(content[offset..].to_vec())
    } else {
        None
    }
}

/// Locates and parses the first ID3v2 tag inside a file (returns view + bytes consumed).
fn scan_id3v2(data: &[u8]) -> Option<(Id3View, usize)> {
    if data.len() < 10 || &data[0..3] != b"ID3" {
        return None;
    }
    let version = data[3];
    let tag_size = ((data[6] as usize & 0x7F) << 21)
        | ((data[7] as usize & 0x7F) << 14)
        | ((data[8] as usize & 0x7F) << 7)
        | (data[9] as usize & 0x7F);
    let total = 10 + tag_size;
    if total > data.len() {
        return None;
    }
    let mut payload = &data[10..total];
    // Extended header: flag bit 6 → length (synchsafe in v4, plain u32 in v3).
    if data[5] & 0x40 != 0 && payload.len() >= 4 {
        let ext_len = if version == 4 {
            ((payload[0] as usize & 0x7F) << 21)
                | ((payload[1] as usize & 0x7F) << 14)
                | ((payload[2] as usize & 0x7F) << 7)
                | (payload[3] as usize & 0x7F)
        } else {
            u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize
        };
        payload = &payload[ext_len.min(payload.len())..];
    }
    let view = parse_id3v2_payload(payload, version);
    Some((view, total))
}

/// Tag-level mapping shared by every ID3-carrying container.
fn tags_from_id3(view: &Id3View) -> ExtractedTags {
    let year_source = view
        .frame("TDRC")
        .first()
        .cloned()
        .or_else(|| view.frame("TDAT").first().cloned())
        .or_else(|| view.frame("TYER").first().cloned());
    let track_number = view
        .frame("TRCK")
        .first()
        .and_then(|t| t.split('/').next().map(|n| n.trim().to_string()))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let lyrics = view
        .sylt
        .clone()
        .or_else(|| view.uslt.iter().find_map(|(_, text)| text.first().cloned()));
    let has_lyrics = lyrics.as_deref().map(has_usable_lyrics).unwrap_or(false);

    ExtractedTags {
        title: first_value(&view.frame("TIT2")),
        artist: join_values(&view.frame("TPE1")),
        album: first_value(&view.frame("TALB")),
        album_artist: join_values(&view.frame("TPE2")),
        genre: join_values(&view.frame("TCON")),
        year: year_source
            .and_then(|y| y.chars().take(4).collect::<String>().parse().ok())
            .unwrap_or(0),
        track_number,
        duration_ms: 0,
        artwork_path: None,
        has_lyrics,
    }
}

/// Reads an MP3 file (ID3v2 + ID3v1 fallback + duration/bitrate).
fn read_id3_container(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let mut tags = ExtractedTags::default();
    let mut consumed = 0usize;
    if let Some((view, used)) = scan_id3v2(&data) {
        consumed = used;
        tags = tags_from_id3(&view);
        if let Some(pic) = view.pictures.first() {
            tags.artwork_path = sink.save(pic);
        }
    }
    if tags.title.is_none() || tags.artist.is_none() {
        if let Some(id3v1) = read_id3v1(&data) {
            if tags.title.is_none() {
                tags.title = id3v1.title;
            }
            if tags.artist.is_none() {
                tags.artist = id3v1.artist.clone();
            }
            if tags.album.is_none() {
                tags.album = id3v1.album.clone();
            }
            if tags.genre.is_none() {
                tags.genre = id3v1.genre.clone();
            }
            if tags.year == 0 {
                tags.year = id3v1.year;
            }
            if tags.track_number == 0 {
                tags.track_number = id3v1.track_number;
            }
        }
    }
    let (duration, bitrate) = mp3_duration_and_bitrate_from(&data[consumed.min(data.len())..]);
    tags.duration_ms = if duration > 0 { duration } else { tags.duration_ms };
    if bitrate > 0 {
        // Kept for details; ExtractedTags has no bitrate field.
    }
    let _ = bitrate;
    Some(tags)
}

struct Id3v1Tags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    year: u32,
    track_number: u32,
    genre: Option<String>,
}

const ID3V1_GENRES: &[&str] = &[
    "Blues", "Classic Rock", "Country", "Dance", "Disco", "Funk", "Grunge", "Hip-Hop", "Jazz",
    "Metal", "New Age", "Oldies", "Other", "Pop", "R&B", "Rap", "Reggae", "Rock", "Techno",
    "Industrial", "Alternative", "Ska", "Death Metal", "Pranks", "Soundtrack", "Euro-Techno",
    "Ambient", "Trip-Hop", "Vocal", "Jazz+Funk", "Fusion", "Trance", "Classical", "Instrumental",
    "Acid", "House", "Game", "Sound Clip", "Gospel", "Noise", "Alt. Rock", "Bass", "Soul",
    "Punk", "Space", "Meditative", "Instrumental Pop", "Instrumental Rock", "Ethnic", "Gothic",
    "Darkwave", "Techno-Industrial", "Electronic", "Pop-Folk", "Eurodance", "Dream",
    "Southern Rock", "Comedy", "Cult", "Gangsta", "Top 40", "Christian Rap", "Pop/Funk", "Jungle",
    "Native American", "Cabaret", "New Wave", "Psychedelic", "Rave", "Showtunes", "Trailer",
    "Lo-Fi", "Tribal", "Acid Punk", "Acid Jazz", "Polka", "Retro", "Musical", "Rock & Roll",
    "Hard Rock", "Folk", "Folk-Rock", "National Folk", "Swing", "Fast-Fusion", "Bebob", "Latin",
    "Revival", "Celtic", "Bluegrass", "Avantgarde", "Gothic Rock", "Progressive Rock",
    "Psychedelic Rock", "Symphonic Rock", "Slow Rock", "Big Band", "Chorus", "Easy Listening",
    "Acoustic", "Humour", "Speech", "Chanson", "Opera", "Chamber Music", "Sonata", "Symphony",
    "Booty Bass", "Primus", "Porn Groove", "Satire", "Slow Jam", "Club", "Tango", "Samba",
    "Folklore", "Ballad", "Power Ballad", "Rhythmic Soul", "Freestyle", "Duet", "Punk Rock",
    "Drum Solo", "A Cappella", "Euro-House", "Dance Hall", "Goa", "Drum & Bass", "Club-House",
    "Hardcore", "Terror", "Indie", "BritPop", "Negerpunk", "Polsk Punk", "Beat",
    "Christian Gangsta Rap", "Heavy Metal", "Black Metal", "Crossover", "Contemporary Christian",
    "Christian Rock", "Merengue", "Salsa", "Thrash Metal", "Anime", "JPop", "Synthpop",
];

fn read_id3v1(data: &[u8]) -> Option<Id3v1Tags> {
    if data.len() < 128 {
        return None;
    }
    let tail = &data[data.len() - 128..];
    if &tail[0..3] != b"TAG" {
        return None;
    }
    let latin = |bytes: &[u8]| -> Option<String> {
        let text: String = bytes
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };
    let year = latin(&tail[93..97]).and_then(|y| y.parse().ok()).unwrap_or(0);
    let track_number = if tail[125] == 0 && tail[126] != 0 {
        tail[126] as u32
    } else {
        0
    };
    let genre = ID3V1_GENRES
        .get(tail[127] as usize)
        .map(|g| g.to_string());
    Some(Id3v1Tags {
        title: latin(&tail[3..33]),
        artist: latin(&tail[33..63]),
        album: latin(&tail[63..93]),
        year,
        track_number,
        genre,
    })
}

// ---------------------------------------------------------------- MP3 audio

const MP3_BITRATES_V1_L3: &[u32] = &[0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
const MP3_BITRATES_V2_L3: &[u32] = &[0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
const MP3_SAMPLE_RATES: &[u32] = &[44100, 48000, 32000, 22050, 24000, 16000, 11025, 12000, 8000];

/// Parses the first MPEG frame header found in the audio data.
fn parse_mpeg_frame(data: &[u8]) -> Option<(u32, u32, u32)> {
    let mut i = 0usize;
    while i + 4 <= data.len() {
        if data[i] == 0xFF && (data[i + 1] & 0xE0) == 0xE0 {
            let version_bits = (data[i + 1] >> 3) & 0x03;
            let layer_bits = (data[i + 1] >> 1) & 0x03;
            let bitrate_index = (data[i + 2] >> 4) & 0x0F;
            let rate_index = (data[i + 2] >> 2) & 0x03;
            if version_bits == 1 || layer_bits == 0 || bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
                i += 1;
                continue;
            }
            let sample_rate = match rate_index {
                0 => MP3_SAMPLE_RATES[0],
                1 => MP3_SAMPLE_RATES[1],
                2 => MP3_SAMPLE_RATES[2],
                _ => unreachable!(),
            } / match version_bits {
                0 => 2, // MPEG 2.5
                2 => 2, // MPEG 2
                _ => 1, // MPEG 1
            };
            let bitrate = match version_bits {
                3 => MP3_BITRATES_V1_L3[bitrate_index as usize],
                _ => MP3_BITRATES_V2_L3[bitrate_index as usize],
            };
            let samples_per_frame: u32 = match layer_bits {
                3 => 384,                    // Layer I
                2 => 1152,                   // Layer II
                _ => match version_bits {    // Layer III
                    3 => 1152,
                    _ => 576,
                },
            };
            return Some((sample_rate, bitrate, samples_per_frame));
        }
        i += 1;
    }
    None
}

/// Finds a Xing/VBRI frame count in the first frame's side data.
fn xing_frames(data: &[u8]) -> Option<u32> {
    for marker in [b"Xing".as_slice(), b"Info".as_slice()] {
        let pos = find_subslice(data, marker)?;
        if pos + 8 <= data.len() {
            let flags = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
            if flags & 0x01 != 0 && pos + 12 <= data.len() {
                let frames = u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
                return Some(frames);
            }
        }
    }
    // VBRI header (Fraunhofer) sits 32 bytes after the frame header.
    if let Some(pos) = find_subslice(data, b"VBRI") {
        if pos + 14 <= data.len() {
            let frames = u32::from_be_bytes([data[pos + 10], data[pos + 11], data[pos + 12], data[pos + 13]]);
            if frames > 0 {
                return Some(frames);
            }
        }
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn mp3_duration_and_bitrate_from(audio: &[u8]) -> (u64, u32) {
    let Some((sample_rate, bitrate, samples_per_frame)) = parse_mpeg_frame(audio) else {
        return (0, 0);
    };
    if let Some(frames) = xing_frames(&audio[..audio.len().min(4096)]) {
        let duration = (frames as u64 * samples_per_frame as u64 * 1000) / sample_rate as u64;
        return (duration, bitrate);
    }
    // CBR estimate over the audio payload.
    let duration = (audio.len() as u64 * 8) / (bitrate as u64 * 1000) * 1000;
    (duration, bitrate)
}

fn mp3_duration_and_bitrate(path: &Path) -> (u64, u32) {
    let data = std::fs::read(path).unwrap_or_default();
    let consumed = scan_id3v2(&data).map(|(_, used)| used).unwrap_or(0);
    mp3_duration_and_bitrate_from(&data[consumed..])
}

fn mp3_sample_rate(path: &Path) -> Option<u32> {
    let data = std::fs::read(path).ok()?;
    let consumed = scan_id3v2(&data).map(|(_, used)| used).unwrap_or(0);
    parse_mpeg_frame(&data[consumed..]).map(|(rate, _, _)| rate)
}

// ======================================================================
// FLAC
// ======================================================================

fn read_flac_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 8 || &data[0..4] != b"fLaC" {
        return None;
    }
    let mut tags = ExtractedTags::default();
    let mut offset = 4usize;
    let mut sample_rate = 0u32;
    let mut total_samples = 0u64;
    loop {
        if offset + 4 > data.len() {
            break;
        }
        let is_last = data[offset] & 0x80 != 0;
        let block_type = data[offset] & 0x7F;
        let block_len = ((data[offset + 1] as usize) << 16)
            | ((data[offset + 2] as usize) << 8)
            | (data[offset + 3] as usize);
        offset += 4;
        if offset + block_len > data.len() {
            break;
        }
        let block = &data[offset..offset + block_len];
        offset += block_len;
        match block_type {
            0 => {
                // STREAMINFO
                if block.len() >= 18 {
                    let b = &block[10..18];
                    sample_rate = ((b[0] as u32) << 12) | ((b[1] as u32) << 4) | ((b[2] as u32) >> 4);
                    total_samples = (((b[3] & 0xF) as u64) << 32)
                        | ((b[4] as u64) << 24)
                        | ((b[5] as u64) << 16)
                        | ((b[6] as u64) << 8)
                        | (b[7] as u64);
                }
            }
            4 => {
                let comments = parse_vorbis_comments(block);
                apply_vorbis(&comments, &mut tags);
            }
            6 => {
                if tags.artwork_path.is_none() {
                    if let Some(pic) = extract_flac_picture(block) {
                        tags.artwork_path = sink.save(&pic);
                    }
                }
            }
            _ => {}
        }
        if is_last {
            break;
        }
    }
    if sample_rate > 0 && total_samples > 0 {
        tags.duration_ms = total_samples * 1000 / sample_rate as u64;
    }
    Some(tags)
}

/// Parses a Xiph/Vorbis comment block: (key, values) preserving case.
pub fn parse_vorbis_comments(data: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    if data.len() < 8 {
        return out;
    }
    let vendor_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut offset = 4 + vendor_len;
    if offset + 4 > data.len() {
        return out;
    }
    let count = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4;
    for _ in 0..count {
        if offset + 4 > data.len() {
            break;
        }
        let len = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;
        if offset + len > data.len() {
            break;
        }
        if let Ok(comment) = std::str::from_utf8(&data[offset..offset + len]) {
            if let Some((key, value)) = comment.split_once('=') {
                if !key.is_empty() {
                    push_comment(&mut out, key, value);
                }
            }
        }
        offset += len;
    }
    out
}

fn push_comment(out: &mut Vec<(String, Vec<String>)>, key: &str, value: &str) {
    let key = key.to_string();
    let value = value.trim().to_string();
    if let Some(entry) = out.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&key)) {
        entry.1.push(value);
    } else {
        out.push((key, vec![value]));
    }
}

/// Looks up a case-insensitive comment list and joins the values.
fn vorbis_lookup(comments: &[(String, Vec<String>)], key: &str) -> Vec<String> {
    comments
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(key))
        .flat_map(|(_, v)| v.iter().cloned())
        .filter(|v| !v.trim().is_empty())
        .collect()
}

fn apply_vorbis(comments: &[(String, Vec<String>)], tags: &mut ExtractedTags) {
    let year_value = vorbis_lookup(comments, "DATE")
        .first()
        .cloned()
        .or_else(|| vorbis_lookup(comments, "YEAR").first().cloned());
    let track_value = vorbis_lookup(comments, "TRACKNUMBER")
        .first()
        .cloned()
        .or_else(|| vorbis_lookup(comments, "TRACK").first().cloned());
    tags.title = first_value(&vorbis_lookup(comments, "TITLE"));
    tags.artist = join_values(&vorbis_lookup(comments, "ARTIST"));
    tags.album = first_value(&vorbis_lookup(comments, "ALBUM"));
    tags.album_artist = join_values(&vorbis_lookup(comments, "ALBUMARTIST"))
        .or_else(|| join_values(&vorbis_lookup(comments, "ALBUM ARTIST")));
    tags.genre = join_values(&vorbis_lookup(comments, "GENRE"));
    tags.year = year_value
        .and_then(|y| y.chars().take(4).collect::<String>().parse().ok())
        .unwrap_or(0);
    tags.track_number = track_value
        .and_then(|t| t.split('/').next().map(|n| n.trim().to_string()))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let lyrics = vorbis_lookup(comments, "LYRICS").join("; ");
    tags.has_lyrics = has_usable_lyrics(&lyrics);
}

fn extract_flac_picture(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 32 {
        return None;
    }
    let mut offset = 4;
    let mime_len = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4 + mime_len;
    if offset + 4 > data.len() {
        return None;
    }
    let desc_len = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4 + desc_len + 16;
    if offset + 4 > data.len() {
        return None;
    }
    let data_len = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4;
    if offset + data_len <= data.len() {
        Some(data[offset..offset + data_len].to_vec())
    } else {
        None
    }
}

fn flac_quality(path: &Path, size: u64) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    if data.len() < 8 || &data[0..4] != b"fLaC" {
        return Quality::default();
    }
    let mut offset = 4usize;
    let mut metadata_bytes = 4usize;
    let mut sample_rate = 0u32;
    let mut total_samples = 0u64;
    let mut bits = 0u32;
    loop {
        if offset + 4 > data.len() {
            break;
        }
        let is_last = data[offset] & 0x80 != 0;
        let block_len = ((data[offset + 1] as usize) << 16)
            | ((data[offset + 2] as usize) << 8)
            | (data[offset + 3] as usize);
        let block_type = data[offset] & 0x7F;
        offset += 4;
        metadata_bytes += 4 + block_len;
        if block_type == 0 && offset + 18 <= data.len() {
            let b = &data[offset + 10..offset + 18];
            sample_rate = ((b[0] as u32) << 12) | ((b[1] as u32) << 4) | ((b[2] as u32) >> 4);
            bits = (((b[2] & 0x1) << 4) | (b[3] >> 4)) as u32 + 1;
            total_samples = (((b[3] & 0xF) as u64) << 32)
                | ((b[4] as u64) << 24)
                | ((b[5] as u64) << 16)
                | ((b[6] as u64) << 8)
                | (b[7] as u64);
        }
        offset += block_len;
        if is_last || offset + 4 > data.len() {
            break;
        }
    }
    let duration = if sample_rate > 0 && total_samples > 0 {
        total_samples * 1000 / sample_rate as u64
    } else {
        0
    };
    let audio_size = size.saturating_sub(metadata_bytes as u64);
    Quality {
        container: "taglib/x-flac",
        duration_ms: duration,
        bitrate_kbps: bitrate_from_size(audio_size, duration),
        sample_rate_hz: sample_rate,
        bits_per_sample: bits,
    }
}

// ======================================================================
// MP4 / M4A
// ======================================================================

fn read_mp4_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let (tags, quality) = parse_mp4_file(&data, sink)?;
    Some(ExtractedTags {
        duration_ms: quality.duration_ms,
        ..tags
    })
}

fn parse_mp4_file(data: &[u8], sink: ArtSink<'_>) -> Option<(ExtractedTags, Quality)> {
    let moov = find_atom_payload(data, b"moov", 0)?;
    let mut tags = ExtractedTags::default();
    let mut quality = Quality {
        container: "taglib/mp4",
        ..Default::default()
    };

    // mvhd → duration (payload includes the version byte).
    if let Some(mvhd) = find_atom_payload(moov, b"mvhd", 0) {
        if mvhd.len() >= 24 {
            let version = mvhd[0];
            let (timescale, duration): (u64, u64) = if version == 1 {
                if mvhd.len() >= 32 {
                    (
                        u32::from_be_bytes([mvhd[20], mvhd[21], mvhd[22], mvhd[23]]) as u64,
                        u64::from_be_bytes([mvhd[24], mvhd[25], mvhd[26], mvhd[27], mvhd[28], mvhd[29], mvhd[30], mvhd[31]]),
                    )
                } else {
                    (0, 0)
                }
            } else if mvhd.len() >= 20 {
                (
                    u32::from_be_bytes([mvhd[12], mvhd[13], mvhd[14], mvhd[15]]) as u64,
                    u32::from_be_bytes([mvhd[16], mvhd[17], mvhd[18], mvhd[19]]) as u64,
                )
            } else {
                (0, 0)
            };
            if timescale > 0 {
                quality.duration_ms = duration * 1000 / timescale;
            }
        }
    }

    // stsd/mp4a → sample rate & bits
    if let Some((sample_rate, bits)) = mp4_audio_format(moov) {
        quality.sample_rate_hz = sample_rate;
        quality.bits_per_sample = bits;
    }

    // udta/meta/ilst → tags
    if let Some(udta) = find_atom_payload(moov, b"udta", 0) {
        if let Some(meta) = find_atom_payload(udta, b"meta", 4) {
            if let Some(ilst) = find_atom_payload(meta, b"ilst", 0) {
                apply_mp4_ilst(ilst, sink, &mut tags);
            }
        }
    }
    let size = data.len() as u64;
    quality.bitrate_kbps = bitrate_from_size(size, quality.duration_ms);
    Some((tags, quality))
}

fn apply_mp4_ilst(ilst: &[u8], sink: ArtSink<'_>, tags: &mut ExtractedTags) {
    let mut text_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cover: Option<Vec<u8>> = None;
    let mut track_pair: (u32, u32) = (0, 0);
    let mut disc_pair: (u32, u32) = (0, 0);
    let mut offset = 0usize;
    while offset + 8 <= ilst.len() {
        let size = u32::from_be_bytes([ilst[offset], ilst[offset + 1], ilst[offset + 2], ilst[offset + 3]]) as usize;
        if size < 8 || offset + size > ilst.len() {
            break;
        }
        let name = decode_ilst_name(&ilst[offset + 4..offset + 8]);
        let body = &ilst[offset + 8..offset + size];
        offset += size;

        match name.as_str() {
            "trkn" | "disk" => {
                for payload in mp4_data_payloads(body) {
                    if payload.len() >= 6 {
                        let first = u16::from_be_bytes([payload[2], payload[3]]) as u32;
                        let second = u16::from_be_bytes([payload[4], payload[5]]) as u32;
                        if name == "trkn" {
                            track_pair = (first, second);
                        } else {
                            disc_pair = (first, second);
                        }
                    }
                }
            }
            "covr" => {
                if cover.is_none() {
                    cover = mp4_data_payloads(body).first().cloned();
                }
            }
            "----" => {
                // Freeform: mean + name atoms describe the full key.
                if let Some(key) = mp4_freeform_key(body) {
                    for payload in mp4_data_payloads(body) {
                        if let Ok(text) = String::from_utf8(payload.clone()) {
                            text_values.entry(key.clone()).or_default().push(text);
                        }
                    }
                }
            }
            _ => {
                for payload in mp4_data_payloads(body) {
                    if let Ok(text) = String::from_utf8(payload.clone()) {
                        let text = text.trim().to_string();
                        if !text.is_empty() {
                            text_values.entry(name.clone()).or_default().push(text);
                        }
                    }
                }
            }
        }
    }

    let lookup = |keys: &[&str]| -> Vec<String> {
        keys.iter()
            .filter_map(|k| text_values.get(*k).cloned())
            .flat_map(|v| v.into_iter())
            .collect()
    };
    tags.title = first_value(&lookup(&["\u{a9}nam"]));
    tags.artist = join_values(&lookup(&["\u{a9}ART"]));
    tags.album = first_value(&lookup(&["\u{a9}alb"]));
    tags.album_artist = join_values(&lookup(&["aART"]));
    tags.genre = join_values(&lookup(&["\u{a9}gen", "gnre"]))
        .or_else(|| {
            // gnre is a numeric ID3 genre index.
            lookup(&["gnre"]).first().and_then(|v| {
                v.parse::<u32>().ok().and_then(|index| {
                    ID3V1_GENRES.get(index.saturating_sub(1) as usize).map(|g| g.to_string())
                })
            })
        });
    tags.year = lookup(&["\u{a9}day"])
        .first()
        .and_then(|y| y.chars().take(4).collect::<String>().parse().ok())
        .unwrap_or(0);
    tags.track_number = track_pair.0;
    tags.has_lyrics = lookup(&["\u{a9}lyr"]).join("").trim().is_empty() == false
        && has_usable_lyrics(&lookup(&["\u{a9}lyr"]).join(""));
    if let Some(pic) = cover {
        tags.artwork_path = sink.save(&pic);
    }
    let _ = disc_pair;
}

/// Decodes a 4-byte ilst child atom name: ASCII plus the 0xA9 copyright
/// prefix byte (Latin-1 `©`), as QuickTime stores them.
pub(crate) fn decode_ilst_name(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                char::from_u32(b as u32).unwrap_or('\u{FFFD}')
            }
        })
        .collect()
}

/// Returns the payloads of every `data` sub-atom in an ilst child body.
pub(crate) fn mp4_data_payloads(body: &[u8]) -> Vec<Vec<u8>> {
    let mut payloads = Vec::new();
    let mut offset = 0usize;
    while offset + 8 <= body.len() {
        let size = u32::from_be_bytes([body[offset], body[offset + 1], body[offset + 2], body[offset + 3]]) as usize;
        if size < 8 || offset + size > body.len() {
            break;
        }
        if &body[offset + 4..offset + 8] == b"data" && size >= 16 {
            payloads.push(body[offset + 16..offset + size].to_vec());
        }
        offset += size;
    }
    payloads
}

/// `----` freeform key: "----:mean:name".
pub(crate) fn mp4_freeform_key(body: &[u8]) -> Option<String> {
    let mean = find_atom_payload(body, b"mean", 4)?;
    let name = find_atom_payload(body, b"name", 4)?;
    let mean_text = String::from_utf8_lossy(mean);
    let name_text = String::from_utf8_lossy(name);
    Some(format!("----:{}:{}", mean_text.trim_end_matches('\0'), name_text.trim_end_matches('\0')))
}

/// Finds a top-level atom's payload by scanning sibling atoms (skips mdat).
fn find_atom_payload<'a>(data: &'a [u8], name: &[u8; 4], skip: usize) -> Option<&'a [u8]> {
    let mut offset = 0usize;
    while offset + 8 <= data.len() {
        let mut size = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as u64;
        let atom_name = &data[offset + 4..offset + 8];
        if size == 1 {
            // 64-bit extended size.
            if offset + 16 > data.len() {
                return None;
            }
            size = u64::from_be_bytes([
                data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11],
                data[offset + 12], data[offset + 13], data[offset + 14], data[offset + 15],
            ]);
        }
        if size < 8 {
            return None;
        }
        let end = (offset as u64 + size).min(data.len() as u64) as usize;
        if atom_name == name {
            let start = offset + 8 + skip;
            if start <= end {
                return Some(&data[start..end]);
            }
            return None;
        }
        offset += size as usize;
    }
    None
}

fn mp4_audio_format(moov: &[u8]) -> Option<(u32, u32)> {
    let mut trak = moov;
    loop {
        let (payload, consumed) = first_atom(trak)?;
        if consumed == 0 {
            break;
        }
        if let Some(found) = mp4_audio_format_in_trak(payload) {
            return Some(found);
        }
        trak = &trak[consumed..];
    }
    None
}

/// Walks the immediate children of a container atom yielding payloads.
fn first_atom(data: &[u8]) -> Option<(&[u8], usize)> {
    if data.len() < 8 {
        return None;
    }
    let mut size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as u64;
    if size == 1 {
        if data.len() < 16 {
            return None;
        }
        size = u64::from_be_bytes([data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15]]);
    }
    if size < 8 {
        return None;
    }
    let end = (size as usize).min(data.len());
    Some((&data[8..end], end))
}

fn mp4_audio_format_in_trak(trak: &[u8]) -> Option<(u32, u32)> {
    let mdia = find_atom_payload(trak, b"mdia", 0)?;
    let minf = find_atom_payload(mdia, b"minf", 0)?;
    let stbl = find_atom_payload(minf, b"stbl", 0)?;
    // stsd body: 4-byte version/flags + 4-byte entry count, then sample entries.
    let stsd = find_atom_payload(stbl, b"stsd", 8)?;
    // First sample entry atom (typically mp4a); payload = reserved(6) + ref(2)
    // + version(2) + revision(2) + vendor(4) + channels(2) + sample size(2)
    // + compression(2) + packet size(2) + sample rate (16.16 fixed).
    let (entry, _) = first_atom(stsd)?;
    if entry.len() >= 28 {
        let sample_rate = u16::from_be_bytes([entry[24], entry[25]]) as u32;
        let bits = u16::from_be_bytes([entry[18], entry[19]]) as u32;
        return Some((sample_rate, bits));
    }
    None
}

fn mp4_quality(path: &Path, _size: u64) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    parse_mp4_file(&data, ArtSink::Global)
        .map(|(_, quality)| quality)
        .unwrap_or_default()
}

// ======================================================================
// OGG / OPUS
// ======================================================================

fn read_ogg_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let is_opus = find_subslice(&data[..data.len().min(65536)], b"OpusTags").is_some();
    let (comments, sample_rate) = ogg_comments_and_rate(&data);
    let mut tags = ExtractedTags::default();
    apply_vorbis(&comments, &mut tags);
    let granule = ogg_last_granule(&data);
    let rate = if is_opus { 48000 } else { sample_rate };
    if granule > 0 && rate > 0 {
        // The first granule is normally zero; Opus pre-skip is small enough to ignore.
        tags.duration_ms = granule * 1000 / rate as u64;
    }
    if let Some(pic) = comments.iter().find_map(|(key, values)| {
        if key.eq_ignore_ascii_case("METADATA_BLOCK_PICTURE") {
            values.first().and_then(|b| decode_base64_flac_picture(b))
        } else if key.eq_ignore_ascii_case("COVERART") {
            values.first().and_then(|b| decode_base64_flac_picture(b))
        } else {
            None
        }
    }) {
        tags.artwork_path = sink.save(&pic);
    }
    Some(tags)
}

fn ogg_comments_and_rate(data: &[u8]) -> (Vec<(String, Vec<String>)>, u32) {
    let mut sample_rate = 0u32;
    if let Some(pos) = find_subslice(&data[..data.len().min(262144)], &[0x01, b'v', b'o', b'r', b'b', b'i', b's']) {
        // vorbis identification header: version(4) channels(1) rate(4) ...
        if pos + 18 <= data.len() {
            sample_rate = u32::from_le_bytes([data[pos + 12], data[pos + 13], data[pos + 14], data[pos + 15]]);
        }
    }
    for marker in [
        &[0x03u8, b'v', b'o', b'r', b'b', b'i', b's'][..],
        b"OpusTags".as_slice(),
    ] {
        if let Some(pos) = find_subslice(&data[..data.len().min(262144)], &marker) {
            let comments = parse_vorbis_comments(&data[pos + marker.len()..]);
            if !comments.is_empty() {
                return (comments, sample_rate);
            }
        }
    }
    (Vec::new(), sample_rate)
}

fn ogg_last_granule(data: &[u8]) -> u64 {
    if data.len() < 27 {
        return 0;
    }
    // Walk chunks backwards looking for the final OggS page.
    let mut start = data.len().saturating_sub(65536);
    loop {
        if let Some(pos) = data[start..].windows(4).rposition(|w| w == b"OggS") {
            let abs = start + pos;
            if abs + 14 <= data.len() {
                let granule = u64::from_le_bytes([
                    data[abs + 6], data[abs + 7], data[abs + 8], data[abs + 9],
                    data[abs + 10], data[abs + 11], data[abs + 12], data[abs + 13],
                ]);
                return granule;
            }
            return 0;
        }
        if start == 0 {
            return 0;
        }
        start = start.saturating_sub(65536);
    }
}

fn decode_base64_flac_picture(text: &str) -> Option<Vec<u8>> {
    let decoded = base64_decode(text.trim())?;
    let mut offset = 4;
    if decoded.len() < 32 {
        return None;
    }
    let mime_len = u32::from_be_bytes([decoded[offset], decoded[offset + 1], decoded[offset + 2], decoded[offset + 3]]) as usize;
    offset += 4 + mime_len;
    if offset + 4 > decoded.len() {
        return None;
    }
    let desc_len = u32::from_be_bytes([decoded[offset], decoded[offset + 1], decoded[offset + 2], decoded[offset + 3]]) as usize;
    offset += 4 + desc_len + 16;
    if offset + 4 > decoded.len() {
        return None;
    }
    let data_len = u32::from_be_bytes([decoded[offset], decoded[offset + 1], decoded[offset + 2], decoded[offset + 3]]) as usize;
    offset += 4;
    if offset + data_len <= decoded.len() {
        Some(decoded[offset..offset + data_len].to_vec())
    } else {
        None
    }
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &byte in text.as_bytes() {
        if byte == b'\r' || byte == b'\n' || byte == b' ' || byte == b'\t' {
            continue;
        }
        let value = TABLE.iter().position(|&t| t == byte)? as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((acc >> bits) as u8);
        }
    }
    Some(output)
}

fn ogg_quality(path: &Path, size: u64) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    let is_opus = find_subslice(&data[..data.len().min(65536)], b"OpusTags").is_some();
    let (_, sample_rate) = ogg_comments_and_rate(&data);
    let rate = if is_opus { 48000 } else { sample_rate };
    let granule = ogg_last_granule(&data);
    let duration = if granule > 0 && rate > 0 { granule * 1000 / rate as u64 } else { 0 };
    Quality {
        container: if is_opus { "taglib/opus" } else { "taglib/ogg" },
        duration_ms: duration,
        bitrate_kbps: bitrate_from_size(size, duration),
        sample_rate_hz: rate,
        bits_per_sample: 0,
    }
}

// ======================================================================
// WAV (RIFF)
// ======================================================================

fn read_wav_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut tags = ExtractedTags::default();
    let mut info = wav_quality_from(&data);
    let mut id3: Option<Id3View> = None;
    let mut info_tags: BTreeMap<String, String> = BTreeMap::new();

    let mut offset = 12usize;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let size = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
        if offset + 8 + size > data.len() {
            break;
        }
        let body = &data[offset + 8..offset + 8 + size];
        match chunk_id {
            b"fmt " => info = wav_quality_from(&data),
            b"ID3 " | b"id3 " => {
                if let Some((view, _)) = scan_id3v2(body) {
                    id3 = Some(view);
                }
            }
            b"LIST" => {
                if body.len() >= 4 && &body[0..4] == b"INFO" {
                    read_riff_info(&body[4..], &mut info_tags);
                }
            }
            _ => {}
        }
        offset += 8 + size + (size & 1); // chunks are word-aligned
    }

    let lookup_info = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .filter_map(|k| info_tags.get(*k).cloned())
            .find(|v| !v.trim().is_empty())
    };
    tags.title = lookup_info(&["INAM"]);
    tags.artist = lookup_info(&["IART"]);
    tags.album = lookup_info(&["IPRD"]);
    tags.genre = lookup_info(&["IGNR"]);
    tags.year = lookup_info(&["ICRD"])
        .and_then(|y| y.chars().take(4).collect::<String>().parse().ok())
        .unwrap_or(0);
    tags.track_number = lookup_info(&["ITRK"])
        .and_then(|t| t.trim().split('/').next().map(|n| n.trim().to_string()))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    if let Some(view) = id3.as_ref() {
        let from_id3 = tags_from_id3(view);
        tags.title = tags.title.or(from_id3.title);
        tags.artist = tags.artist.or(from_id3.artist);
        tags.album = tags.album.or(from_id3.album);
        tags.album_artist = tags.album_artist.or(from_id3.album_artist);
        tags.genre = tags.genre.or(from_id3.genre);
        if tags.year == 0 {
            tags.year = from_id3.year;
        }
        if tags.track_number == 0 {
            tags.track_number = from_id3.track_number;
        }
        tags.has_lyrics = tags.has_lyrics || from_id3.has_lyrics;
        if let Some(pic) = view.pictures.first() {
            tags.artwork_path = sink.save(pic);
        }
    }
    tags.duration_ms = info.duration_ms;
    Some(tags)
}

fn read_riff_info(data: &[u8], out: &mut BTreeMap<String, String>) {
    let mut offset = 0usize;
    while offset + 8 <= data.len() {
        let id = String::from_utf8_lossy(&data[offset..offset + 4]).to_string();
        let size = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
        if offset + 8 + size > data.len() {
            break;
        }
        let value = String::from_utf8_lossy(&data[offset + 8..offset + 8 + size])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if !value.is_empty() {
            out.insert(id, value);
        }
        offset += 8 + size + (size & 1);
    }
}

fn wav_quality_from(data: &[u8]) -> Quality {
    let mut quality = Quality {
        container: "taglib/wav",
        ..Default::default()
    };
    let mut offset = 12usize;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let size = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
        if offset + 8 + size > data.len() {
            break;
        }
        if chunk_id == b"fmt " && size >= 16 {
            let body = &data[offset + 8..offset + 8 + size];
            let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let byte_rate = u32::from_le_bytes([body[8], body[9], body[10], body[11]]);
            let bits = u16::from_le_bytes([body[14], body[15]]) as u32;
            quality.sample_rate_hz = sample_rate;
            quality.bits_per_sample = bits;
            quality.bitrate_kbps = byte_rate * 8 / 1000;
        }
        if chunk_id == b"data" {
            let byte_rate = quality.bitrate_kbps as u64 * 1000 / 8;
            if byte_rate > 0 {
                quality.duration_ms = size as u64 * 1000 / byte_rate;
            }
        }
        offset += 8 + size + (size & 1);
    }
    quality
}

fn wav_quality(path: &Path) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        wav_quality_from(&data)
    } else {
        Quality::default()
    }
}

// ======================================================================
// ASF / WMA
// ======================================================================

const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_FILE_PROPERTIES_GUID: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const ASF_CONTENT_DESCRIPTION_GUID: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_EXTENDED_CONTENT_GUID: [u8; 16] = [
    0xD2, 0xD0, 0xA4, 0x40, 0xE3, 0x07, 0xD2, 0x11, 0x97, 0x3F, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x36,
];
const ASF_STREAM_PROPERTIES_GUID: [u8; 16] = [
    0x91, 0x07, 0xDC, 0xB7, 0xB7, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];

#[derive(Debug, Default)]
struct AsfView {
    title: Option<String>,
    author: Option<String>,
    copyright: Option<String>,
    description: Option<String>,
    rating: Option<String>,
    extended: Vec<(String, String)>, // unicode descriptors
}

fn read_asf_file(path: &Path, _sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let (view, quality) = parse_asf(&data)?;
    let tags = ExtractedTags {
        title: view.title.clone(),
        artist: view.author.clone(),
        album: asf_lookup(&view, "WM/AlbumTitle"),
        album_artist: asf_lookup(&view, "WM/AlbumArtist").or_else(|| asf_lookup(&view, "WM/AlbumArtistSort")),
        genre: asf_lookup(&view, "WM/Genre").or_else(|| asf_lookup(&view, "WM/GenreID")),
        year: asf_lookup(&view, "WM/Year")
            .and_then(|y| y.chars().take(4).collect::<String>().parse().ok())
            .unwrap_or(0),
        track_number: asf_lookup(&view, "WM/TrackNumber")
            .and_then(|t| t.trim().split('/').next().map(|n| n.trim().to_string()))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0),
        duration_ms: quality.duration_ms,
        artwork_path: None,
        has_lyrics: asf_lookup(&view, "WM/Lyrics").map(|l| has_usable_lyrics(&l)).unwrap_or(false),
    };
    Some(tags)
}

fn asf_lookup(view: &AsfView, name: &str) -> Option<String> {
    view.extended
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
        .filter(|value| !value.trim().is_empty())
}

fn parse_asf(data: &[u8]) -> Option<(AsfView, Quality)> {
    if data.len() < 30 || data[0..16] != ASF_HEADER_GUID {
        return None;
    }
    let header_size = u64::from_le_bytes([
        data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
    ]);
    let end = (header_size as usize).min(data.len());
    let mut view = AsfView::default();
    let mut quality = Quality {
        container: "taglib/asf",
        ..Default::default()
    };
    let mut offset = 30usize;
    while offset + 24 <= end {
        let guid = &data[offset..offset + 16];
        let size = u64::from_le_bytes([
            data[offset + 16], data[offset + 17], data[offset + 18], data[offset + 19],
            data[offset + 20], data[offset + 21], data[offset + 22], data[offset + 23],
        ]);
        if size < 24 || offset + size as usize > end {
            break;
        }
        let body = &data[offset + 24..offset + size as usize];
        if guid == ASF_FILE_PROPERTIES_GUID && body.len() >= 48 {
            let play_duration = u64::from_le_bytes([
                body[40], body[41], body[42], body[43], body[44], body[45], body[46], body[47],
            ]);
            quality.duration_ms = play_duration / 10_000;
        } else if guid == ASF_CONTENT_DESCRIPTION_GUID && body.len() >= 10 {
            let lens: Vec<usize> = (0..5)
                .map(|i| u16::from_le_bytes([body[i * 2], body[i * 2 + 1]]) as usize)
                .collect();
            let mut cursor = 10usize;
            let mut texts: Vec<String> = Vec::new();
            for len in &lens {
                if cursor + len <= body.len() {
                    texts.push(decode_utf16le_string(&body[cursor..cursor + len]));
                }
                cursor += len;
            }
            view.title = non_empty(&texts.get(0).cloned().unwrap_or_default());
            view.author = non_empty(&texts.get(1).cloned().unwrap_or_default());
            view.copyright = non_empty(&texts.get(2).cloned().unwrap_or_default());
            view.description = non_empty(&texts.get(3).cloned().unwrap_or_default());
            view.rating = non_empty(&texts.get(4).cloned().unwrap_or_default());
        } else if guid == ASF_EXTENDED_CONTENT_GUID && body.len() >= 2 {
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
                let name = decode_utf16le_string(&body[cursor..cursor + name_len]);
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
                let value_bytes = &body[cursor..cursor + value_len];
                cursor += value_len;
                if value_type == 0 {
                    view.extended.push((name, decode_utf16le_string(value_bytes)));
                }
            }
        } else if guid == ASF_STREAM_PROPERTIES_GUID && body.len() >= 68 {
            let stream_type = &body[0..16];
            // Audio stream type GUID: 00000001-0000-0010-8000-00AA00389B71.
            if stream_type == [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71] {
                let channels = u16::from_le_bytes([body[54], body[55]]) as u32;
                let sample_rate = u32::from_le_bytes([body[56], body[57], body[58], body[59]]);
                let avg_bytes = u32::from_le_bytes([body[60], body[61], body[62], body[63]]);
                let bits = u16::from_le_bytes([body[66], body[67]]) as u32;
                quality.sample_rate_hz = sample_rate;
                quality.bits_per_sample = bits;
                quality.bitrate_kbps = avg_bytes * 8 / 1000;
                let _ = channels;
            }
        }
        offset += size as usize;
    }
    Some((view, quality))
}

fn decode_utf16le_string(bytes: &[u8]) -> String {
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

fn non_empty(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn asf_quality(path: &Path) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    parse_asf(&data)
        .map(|(_, quality)| quality)
        .unwrap_or_default()
}

// ======================================================================
// APE (ape / wv / mpc)
// ======================================================================

fn read_ape_container(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let items = parse_ape_items(&data);
    let mut tags = ExtractedTags::default();
    let get = |key: &str| -> Vec<String> {
        items
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    tags.title = first_value(&get("Title"));
    tags.artist = join_values(&get("Artist"));
    tags.album = first_value(&get("Album"));
    tags.album_artist = join_values(&get("Album Artist")).or_else(|| join_values(&get("AlbumArtist")));
    tags.genre = join_values(&get("Genre"));
    let year_value = get("Year").first().cloned();
    let track_value = get("Track").first().cloned().or_else(|| get("TrackNumber").first().cloned());
    tags.year = year_value
        .and_then(|y| y.chars().take(4).collect::<String>().parse().ok())
        .unwrap_or(0);
    tags.track_number = track_value
        .and_then(|t| t.split('/').next().map(|n| n.trim().to_string()))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let lyrics = get("Lyrics").join("\n");
    tags.has_lyrics = has_usable_lyrics(&lyrics);
    let _ = sink;
    Some(tags)
}

/// Parses an APEv2 tag (footer/version 2000) into (key, values) items.
pub fn parse_ape_items(data: &[u8]) -> Vec<(String, Vec<String>)> {
    let marker = b"APETAGEX";
    let mut items = Vec::new();
    let Some(footer_pos) = find_last_subslice(data, marker) else {
        return items;
    };
    if footer_pos + 32 > data.len() {
        return items;
    }
    let footer = &data[footer_pos..footer_pos + 32];
    let tag_size = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]) as usize;
    let item_count = u32::from_le_bytes([footer[16], footer[17], footer[18], footer[19]]) as usize;
    let has_header = footer[23] & 0x80 != 0;
    if tag_size < 32 || tag_size > data.len() {
        return items;
    }
    let mut items_start = footer_pos + 32 - tag_size;
    if has_header {
        items_start += 32;
    }
    let items_end = footer_pos;
    let mut offset = items_start;
    for _ in 0..item_count {
        if offset + 8 > items_end {
            break;
        }
        let size = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
        let _flags = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]);
        offset += 8;
        if size > items_end.saturating_sub(offset) {
            break;
        }
        let key_end = data[offset..offset + size]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(size);
        let key = String::from_utf8_lossy(&data[offset..offset + key_end]).to_string();
        let value = data[offset + key_end + 1..offset + size].to_vec();
        offset += size;
        if key.is_empty() {
            continue;
        }
        // Item type flags bit 29-30: 0 = text (utf8), 1 = binary, 2 = locator.
        let text = String::from_utf8_lossy(&value);
        let values: Vec<String> = text
            .split('\u{0}')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        items.push((key, values));
    }
    items
}

fn find_last_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

const WAVPACK_RATES: &[u32] = &[
    6000, 8000, 9600, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000,
    176400, 192000,
];

fn wavpack_quality(path: &Path) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    let Some(pos) = find_last_subslice(&data, b"wvpk") else {
        return Quality::default();
    };
    if pos + 28 > data.len() {
        return Quality::default();
    }
    let total_samples = u32::from_le_bytes([data[pos + 16], data[pos + 17], data[pos + 18], data[pos + 19]]);
    let flags = u32::from_le_bytes([data[pos + 24], data[pos + 25], data[pos + 26], data[pos + 27]]);
    let rate_index = ((flags >> 23) & 0x0F) as usize;
    let rate = WAVPACK_RATES.get(rate_index).copied().unwrap_or(44100);
    let samples = if total_samples == 0xFFFF_FFFF { 0 } else { total_samples as u64 };
    let duration = if samples > 0 { samples * 1000 / rate as u64 } else { 0 };
    Quality {
        container: "taglib/wavpack",
        duration_ms: duration,
        bitrate_kbps: bitrate_from_size(data.len() as u64, duration),
        sample_rate_hz: rate,
        bits_per_sample: 0,
    }
}

// ======================================================================
// AIFF
// ======================================================================

fn read_aiff_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 || &data[0..4] != b"FORM" || (&data[8..12] != b"AIFF" && &data[8..12] != b"AIFC") {
        return None;
    }
    let mut tags = ExtractedTags::default();
    let mut offset = 12usize;
    while offset + 8 <= data.len() {
        let chunk = &data[offset..offset + 4];
        let size = u32::from_be_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
        if offset + 8 + size > data.len() {
            break;
        }
        let body = &data[offset + 8..offset + 8 + size];
        if chunk == b"ID3 " || chunk == b"id3 " {
            if let Some((view, _)) = scan_id3v2(body) {
                tags = tags_from_id3(&view);
                if let Some(pic) = view.pictures.first() {
                    tags.artwork_path = sink.save(pic);
                }
            }
        }
        offset += 8 + size + (size & 1);
    }
    let quality = aiff_quality(path);
    tags.duration_ms = quality.duration_ms;
    Some(tags)
}

fn read_ieee_754_80_be(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 10 {
        return None;
    }
    let sign = if bytes[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bytes[0] & 0x7F) as i32) << 8 | bytes[1] as i32;
    let mut mantissa: u128 = 0;
    for &b in &bytes[2..10] {
        mantissa = (mantissa << 8) | b as u128;
    }
    if mantissa == 0 && exponent == 0 {
        return Some(0.0);
    }
    Some(sign * (mantissa as f64) * 2f64.powi(exponent - 16383 - 63))
}

fn aiff_quality(path: &Path) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    if data.len() < 12 || &data[0..4] != b"FORM" || (&data[8..12] != b"AIFF" && &data[8..12] != b"AIFC") {
        return Quality::default();
    }
    let mut quality = Quality {
        container: "taglib/aiff",
        ..Default::default()
    };
    let mut offset = 12usize;
    while offset + 8 <= data.len() {
        let chunk = &data[offset..offset + 4];
        let size = u32::from_be_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
        if offset + 8 + size > data.len() {
            break;
        }
        if chunk == b"COMM" && size >= 18 {
            let body = &data[offset + 8..offset + 8 + size];
            let channels = u16::from_be_bytes([body[0], body[1]]) as u32;
            let frames = u32::from_be_bytes([body[2], body[3], body[4], body[5]]) as u64;
            let bits = u16::from_be_bytes([body[6], body[7]]) as u32;
            let rate = read_ieee_754_80_be(&body[8..18]).unwrap_or(0.0) as u64;
            quality.sample_rate_hz = rate as u32;
            quality.bits_per_sample = bits;
            if rate > 0 {
                quality.duration_ms = frames * 1000 / rate;
            }
            let _ = channels;
        }
        offset += 8 + size + (size & 1);
    }
    quality
}

// ======================================================================
// DSF
// ======================================================================

fn read_dsf_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 || &data[0..4] != b"DSD " {
        return None;
    }
    let metadata_ptr = u64::from_le_bytes([data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27]]) as usize;
    if metadata_ptr > 0 && metadata_ptr + 10 <= data.len() {
        if let Some((view, _)) = scan_id3v2(&data[metadata_ptr..]) {
            let mut tags = tags_from_id3(&view);
            if let Some(pic) = view.pictures.first() {
                tags.artwork_path = sink.save(pic);
            }
            let quality = dsf_quality(path);
            tags.duration_ms = quality.duration_ms;
            return Some(tags);
        }
    }
    let quality = dsf_quality(path);
    Some(ExtractedTags {
        duration_ms: quality.duration_ms,
        ..Default::default()
    })
}

fn dsf_quality(path: &Path) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    if data.len() < 28 || &data[0..4] != b"DSD " {
        return Quality::default();
    }
    // The DSD chunk's pointer to the fmt chunk (spec offset 20; 0 falls back
    // to the first position after the 28-byte DSD chunk).
    let fmt_start = u64::from_le_bytes([
        data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
    ]) as usize;
    let offset = if fmt_start >= 28 { fmt_start } else { 28 };
    if offset + 52 > data.len() || &data[offset..offset + 4] != b"fmt " {
        return Quality::default();
    }
    let freq = u32::from_le_bytes([data[offset + 16], data[offset + 17], data[offset + 18], data[offset + 19]]);
    let bits = u32::from_le_bytes([data[offset + 20], data[offset + 21], data[offset + 22], data[offset + 23]]);
    let samples = u64::from_le_bytes([
        data[offset + 24], data[offset + 25], data[offset + 26], data[offset + 27],
        data[offset + 28], data[offset + 29], data[offset + 30], data[offset + 31],
    ]);
    let duration = if freq > 0 { samples * 1000 / freq as u64 } else { 0 };
    Quality {
        container: "taglib/dsf",
        duration_ms: duration,
        bitrate_kbps: bitrate_from_size(data.len() as u64, duration),
        sample_rate_hz: freq,
        bits_per_sample: bits,
    }
}

// ======================================================================
// TTA
// ======================================================================

fn read_tta_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let header = find_tta_header(&data)?;
    let quality = tta_quality(path);
    let mut tags = if let Some((view, _)) = scan_id3v2(&data[..header]) {
        let mut tags = tags_from_id3(&view);
        if let Some(pic) = view.pictures.first() {
            tags.artwork_path = sink.save(pic);
        }
        tags
    } else {
        ExtractedTags::default()
    };
    tags.duration_ms = quality.duration_ms;
    let _ = sink;
    Some(tags)
}

fn find_tta_header(data: &[u8]) -> Option<usize> {
    if data.len() < 20 {
        return None;
    }
    if &data[0..4] == b"TTA1" || &data[0..4] == b"TTA2" {
        return Some(0);
    }
    // ID3v2 tag precedes the header.
    if &data[0..3] == b"ID3" {
        let tag_size = ((data[6] as usize & 0x7F) << 21)
            | ((data[7] as usize & 0x7F) << 14)
            | ((data[8] as usize & 0x7F) << 7)
            | (data[9] as usize & 0x7F);
        let offset = 10 + tag_size;
        if offset + 4 <= data.len() && (&data[offset..offset + 4] == b"TTA1" || &data[offset..offset + 4] == b"TTA2") {
            return Some(offset);
        }
    }
    None
}

fn tta_quality(path: &Path) -> Quality {
    let data = std::fs::read(path).unwrap_or_default();
    if let Some(header) = find_tta_header(&data) {
        if header + 20 <= data.len() {
            let rate = u32::from_le_bytes([data[header + 10], data[header + 11], data[header + 12], data[header + 13]]);
            let samples = u32::from_le_bytes([data[header + 14], data[header + 15], data[header + 16], data[header + 17]]) as u64;
            let bits = u16::from_le_bytes([data[header + 8], data[header + 9]]) as u32;
            let channels = u16::from_le_bytes([data[header + 6], data[header + 7]]) as u32;
            let duration = if rate > 0 { samples * 1000 / rate as u64 } else { 0 };
            let _ = channels;
            return Quality {
                container: "taglib/tta",
                duration_ms: duration,
                bitrate_kbps: bitrate_from_size(data.len() as u64, duration),
                sample_rate_hz: rate,
                bits_per_sample: bits,
            };
        }
    }
    Quality {
        container: "taglib/tta",
        ..Default::default()
    }
}

// ======================================================================
// AAC (ADTS)
// ======================================================================

const AAC_RATES: &[u32] = &[
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0, 0,
];

fn read_aac_file(path: &Path, sink: ArtSink<'_>) -> Option<ExtractedTags> {
    let data = std::fs::read(path).ok()?;
    let mut tags = ExtractedTags::default();
    let mut consumed = 0usize;
    if let Some((view, used)) = scan_id3v2(&data) {
        consumed = used;
        tags = tags_from_id3(&view);
        if let Some(pic) = view.pictures.first() {
            tags.artwork_path = sink.save(pic);
        }
    }
    let (duration, _bitrate, rate) = aac_adts_quality(&data[consumed.min(data.len())..]);
    tags.duration_ms = duration;
    let _ = rate;
    let _ = sink;
    Some(tags)
}

/// Scans ADTS frames returning (duration_ms, bitrate_kbps, sample_rate).
fn aac_adts_quality(data: &[u8]) -> (u64, u32, u32) {
    let mut offset = 0usize;
    let mut frames = 0u64;
    let mut sample_rate = 0u32;
    while offset + 7 <= data.len() {
        if data[offset] == 0xFF && (data[offset + 1] & 0xF0) == 0xF0 {
            let rate_index = ((data[offset + 2] >> 2) & 0x0F) as usize;
            if rate_index < 13 {
                let rate = AAC_RATES[rate_index];
                if rate == 0 {
                    break;
                }
                let length = (((data[offset + 3] & 0x03) as usize) << 11)
                    | ((data[offset + 4] as usize) << 3)
                    | ((data[offset + 5] >> 5) as usize);
                if length < 7 {
                    break;
                }
                sample_rate = rate;
                frames += 1;
                offset += length;
            } else {
                break;
            }
        } else {
            offset += 1;
        }
    }
    if frames == 0 || sample_rate == 0 {
        return (0, 0, sample_rate);
    }
    let duration = frames * 1024 * 1000 / sample_rate as u64;
    let bitrate = bitrate_from_size(data.len() as u64, duration);
    (duration, bitrate, sample_rate)
}

// ======================================================================
// Custom + additional standard tag fields (TagEditor.cs read surface)
// ======================================================================

pub const ADDITIONAL_STANDARD_FIELDS: &[(&str, &str)] = &[
    ("TITLE_SORT", "Title sort"),
    ("SUBTITLE", "Subtitle"),
    ("DESCRIPTION", "Description"),
    ("ARTIST_SORT", "Artist sort"),
    ("ARTIST_ROLE", "Artist role / instruments"),
    ("ALBUM_ARTIST_SORT", "Album artist sort"),
    ("COMPOSERS", "Composers"),
    ("COMPOSER_SORT", "Composer sort"),
    ("ALBUM_SORT", "Album sort"),
    ("COMMENT", "Comment"),
    ("TRACK_COUNT", "Track count"),
    ("DISC", "Disc number"),
    ("DISC_COUNT", "Disc count"),
    ("GROUPING", "Grouping"),
    ("BPM", "Beats per minute"),
    ("CONDUCTOR", "Conductor"),
    ("COPYRIGHT", "Copyright"),
    ("MUSICBRAINZ_ARTIST_ID", "MusicBrainz artist ID"),
    ("MUSICBRAINZ_RELEASE_GROUP_ID", "MusicBrainz release group ID"),
    ("MUSICBRAINZ_RELEASE_ID", "MusicBrainz release ID"),
    ("MUSICBRAINZ_RELEASE_ARTIST_ID", "MusicBrainz release artist ID"),
    ("MUSICBRAINZ_TRACK_ID", "MusicBrainz track ID"),
    ("MUSICBRAINZ_DISC_ID", "MusicBrainz disc ID"),
    ("MUSICIP_ID", "MusicIP ID"),
    ("AMAZON_ID", "Amazon ID"),
    ("MUSICBRAINZ_RELEASE_STATUS", "Release status"),
    ("MUSICBRAINZ_RELEASE_TYPE", "Release type"),
    ("MUSICBRAINZ_RELEASE_COUNTRY", "Release country"),
    ("DATE_TAGGED", "Date tagged (ISO 8601)"),
    ("REPLAYGAIN_TRACK_GAIN", "ReplayGain track gain (dB)"),
    ("REPLAYGAIN_TRACK_PEAK", "ReplayGain track peak"),
    ("REPLAYGAIN_ALBUM_GAIN", "ReplayGain album gain (dB)"),
    ("REPLAYGAIN_ALBUM_PEAK", "ReplayGain album peak"),
    ("INITIAL_KEY", "Initial key"),
    ("REMIXED_BY", "Remixed by"),
    ("PUBLISHER", "Publisher"),
    ("ISRC", "ISRC"),
];

/// Field names that are edited by the standard tag surface (CommonFields set).
pub const COMMON_FIELDS: &[&str] = &[
    "TITLE", "ARTIST", "ALBUM", "ALBUMARTIST", "ALBUM ARTIST", "GENRE", "DATE", "YEAR", "TRACK",
    "TRACKNUMBER", "LYRICS", "COVERART", "METADATA_BLOCK_PICTURE", "WM/TITLE", "WM/AUTHOR",
    "WM/ALBUMTITLE", "WM/ALBUMARTIST", "WM/GENRE", "WM/YEAR", "WM/TRACKNUMBER", "WM/LYRICS",
    "WM/PICTURE", "ALBUM ARTIST", "TRACK", "TITLE_SORT", "TITLESORT", "SORTNAME", "SUBTITLE",
    "DESCRIPTION", "ARTIST_SORT", "ARTISTSORT", "SORTARTIST", "ARTIST_ROLE", "PERFORMER_ROLE",
    "ALBUM_ARTIST_SORT", "ALBUMARTISTSORT", "COMPOSER", "COMPOSERS", "COMPOSER_SORT",
    "COMPOSERSORT", "ALBUM_SORT", "SORTALBUM", "COMMENT", "TRACKCOUNT", "TRACKTOTAL",
    "TOTALTRACKS", "DISC", "DISCNUMBER", "DISCCOUNT", "DISCTOTAL", "TOTALDISCS", "GROUPING",
    "CONTENTGROUP", "BPM", "BEATSPERMINUTE", "CONDUCTOR", "COPYRIGHT", "DATE_TAGGED",
    "MUSICBRAINZ_ARTIST_ID", "MUSICBRAINZ_RELEASE_GROUP_ID", "MUSICBRAINZ_RELEASE_ID",
    "MUSICBRAINZ_RELEASE_ARTIST_ID", "MUSICBRAINZ_TRACK_ID", "MUSICBRAINZ_DISC_ID", "MUSICIP_ID",
    "AMAZON_ID", "MUSICBRAINZ_RELEASE_STATUS", "MUSICBRAINZ_RELEASE_TYPE",
    "MUSICBRAINZ_RELEASE_COUNTRY", "REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_PEAK",
    "REPLAYGAIN_ALBUM_GAIN", "REPLAYGAIN_ALBUM_PEAK", "INITIAL_KEY", "REMIXED_BY", "PUBLISHER",
    "ISRC", "WM/TITLESORT", "WM/SUBTITLE", "WM/DESCRIPTION", "WM/ARTISTSORT",
    "WM/ALBUMARTISTSORT", "WM/COMPOSERS", "WM/COMPOSERSORT", "WM/ALBUMSORT", "WM/COMMENT",
    "WM/TRACKCOUNT", "WM/PARTOFSET", "WM/BEATSPERMINUTE", "WM/CONDUCTOR", "WM/COPYRIGHT",
    "WM/PUBLISHER", "WM/ISRC", "WM/INITIALKEY", "WM/MODIFIEDBY", "MUSICBRAINZ/ARTIST ID",
    "MUSICBRAINZ/RELEASE GROUP ID", "MUSICBRAINZ/ALBUM ID", "MUSICBRAINZ/ALBUM ARTIST ID",
    "MUSICBRAINZ/TRACK ID", "MUSICBRAINZ/DISC ID", "MUSICBRAINZ/ALBUM STATUS",
    "MUSICBRAINZ/ALBUM TYPE", "MUSICBRAINZ/ALBUM RELEASE COUNTRY", "MUSICBRAINZ ARTIST ID",
    "MUSICBRAINZ RELEASE GROUP ID", "MUSICBRAINZ ALBUM ID", "MUSICBRAINZ ALBUM ARTIST ID",
    "MUSICBRAINZ TRACK ID", "MUSICBRAINZ DISC ID", "MUSICBRAINZ ALBUM STATUS",
    "MUSICBRAINZ ALBUM TYPE", "MUSICBRAINZ ALBUM RELEASE COUNTRY",
];

pub fn is_common_field(key: &str) -> bool {
    COMMON_FIELDS.iter().any(|f| f.eq_ignore_ascii_case(key))
}

const COMMON_ID3_FRAMES: &[&str] = &[
    "TIT2", "TPE1", "TALB", "TPE2", "TCON", "TYER", "TDRC", "TRCK", "TSOT", "TIT3", "TIT1", "TSOP",
    "TMCL", "TIPL", "TCOM", "TSOC", "TSO2", "TSOA", "TPOS", "TBPM", "TPE3", "TCOP", "TKEY", "TPE4",
    "TPUB", "TSRC",
];

fn is_common_id3_frame(id: &str) -> bool {
    COMMON_ID3_FRAMES.iter().any(|f| f.eq_ignore_ascii_case(id))
}

/// `TagEditor.CustomFieldFormat` port.
pub fn custom_field_format(path: &Path) -> Option<&'static str> {
    match file_ext(path).as_str() {
        "flac" | "ogg" | "oga" | "opus" => Some("Xiph/Vorbis comments"),
        "wma" => Some("ASF descriptors"),
        "ape" | "wv" | "mpc" => Some("APEv2 items"),
        "mp3" | "wav" | "wave" | "aif" | "aiff" | "aac" | "tta" | "dsf" | "dff" => {
            Some("ID3v2 text frames and user text")
        }
        _ => None,
    }
}

/// The custom (non-standard) tag fields stored in the file.
pub fn read_custom_fields(path: &Path) -> BTreeMap<String, String> {
    let data = std::fs::read(path).unwrap_or_default();
    let mut out = BTreeMap::new();
    match file_ext(path).as_str() {
        "flac" => {
            if let Some((comments, _)) = flac_comments(&data) {
                for (key, values) in comments {
                    if !is_common_field(&key) {
                        out.insert(key, join_values(&values).unwrap_or_default());
                    }
                }
            }
        }
        "ogg" | "oga" | "opus" => {
            let (comments, _) = ogg_comments_and_rate(&data);
            for (key, values) in comments {
                if !is_common_field(&key) {
                    out.insert(key, join_values(&values).unwrap_or_default());
                }
            }
        }
        "wma" => {
            if let Some((view, _)) = parse_asf(&data) {
                for (name, value) in view.extended {
                    if !is_common_field(&name) && !value.trim().is_empty() {
                        out.insert(name, value);
                    }
                }
            }
        }
        "ape" | "wv" | "mpc" => {
            for (key, values) in parse_ape_items(&data) {
                if !is_common_field(&key) {
                    out.insert(key, join_values(&values).unwrap_or_default());
                }
            }
        }
        "mp3" | "wav" | "wave" | "aif" | "aiff" | "aac" | "tta" | "dsf" | "dff" => {
            if let Some((view, _)) = id3_tag_payload(&data, path) {
                for (description, values) in &view.user_text {
                    if !description.trim().is_empty() {
                        out.insert(description.clone(), join_values(values).unwrap_or_default());
                    }
                }
                for (id, values) in &view.text {
                    if !is_common_id3_frame(id) && !values.is_empty() {
                        out.insert(format!("ID3:{}", id), join_values(values).unwrap_or_default());
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Locates the ID3v2 payload bytes for a container (returns payload view + position).
fn id3_tag_payload<'a>(data: &'a [u8], path: &Path) -> Option<(Id3View, usize)> {
    let ext = file_ext(path);
    match ext.as_str() {
        "wav" | "wave" => {
            let mut offset = 12usize;
            while offset + 8 <= data.len() {
                let chunk = &data[offset..offset + 4];
                let size = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
                if offset + 8 + size > data.len() {
                    break;
                }
                if chunk == b"ID3 " || chunk == b"id3 " {
                    return scan_id3v2(&data[offset + 8..offset + 8 + size]).map(|(v, u)| (v, offset + 8 + u));
                }
                offset += 8 + size + (size & 1);
            }
            None
        }
        "aif" | "aiff" => {
            let mut offset = 12usize;
            while offset + 8 <= data.len() {
                let chunk = &data[offset..offset + 4];
                let size = u32::from_be_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
                if offset + 8 + size > data.len() {
                    break;
                }
                if chunk == b"ID3 " || chunk == b"id3 " {
                    return scan_id3v2(&data[offset + 8..offset + 8 + size]).map(|(v, u)| (v, offset + 8 + u));
                }
                offset += 8 + size + (size & 1);
            }
            None
        }
        "dsf" => {
            let metadata_ptr = u64::from_le_bytes([data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27]]) as usize;
            if metadata_ptr > 0 && metadata_ptr < data.len() {
                return scan_id3v2(&data[metadata_ptr..]).map(|(v, u)| (v, metadata_ptr + u));
            }
            None
        }
        "dff" => crate::metadata::dff_id3_payload(data).map(|(start, len)| {
            let (view, used) = scan_id3v2(&data[start..start + len]).unwrap_or_default();
            (view, used)
        }),
        _ => scan_id3v2(data).map(|(v, used)| (v, used)),
    }
}

fn flac_comments(data: &[u8]) -> Option<(Vec<(String, Vec<String>)>, u32)> {
    if data.len() < 8 || &data[0..4] != b"fLaC" {
        return None;
    }
    let mut offset = 4usize;
    loop {
        if offset + 4 > data.len() {
            break;
        }
        let is_last = data[offset] & 0x80 != 0;
        let block_type = data[offset] & 0x7F;
        let block_len = ((data[offset + 1] as usize) << 16)
            | ((data[offset + 2] as usize) << 8)
            | (data[offset + 3] as usize);
        offset += 4;
        if offset + block_len > data.len() {
            break;
        }
        if block_type == 4 {
            return Some((parse_vorbis_comments(&data[offset..offset + block_len]), 0));
        }
        offset += block_len;
        if is_last {
            break;
        }
    }
    None
}

/// `TagEditor.ReadAdditionalStandardFields` port: every catalog key, "" default.
pub fn read_additional_standard_fields(path: &Path) -> BTreeMap<String, String> {
    let data = std::fs::read(path).unwrap_or_default();
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (key, _) in ADDITIONAL_STANDARD_FIELDS {
        out.insert(key.to_string(), String::new());
    }
    let ext = file_ext(path);
    match ext.as_str() {
        "flac" => {
            if let Some((comments, _)) = flac_comments(&data) {
                apply_xiph_additional(&comments, &mut out);
            }
        }
        "ogg" | "oga" | "opus" => {
            let (comments, _) = ogg_comments_and_rate(&data);
            apply_xiph_additional(&comments, &mut out);
        }
        "wma" => {
            if let Some((view, _)) = parse_asf(&data) {
                apply_asf_additional(&view, &mut out);
            }
        }
        "ape" | "wv" | "mpc" => {
            let items = parse_ape_items(&data);
            apply_ape_additional(&items, &mut out);
        }
        "m4a" | "mp4" | "m4b" => {
            if let Some((values, _)) = mp4_ilst_values(&data) {
                apply_mp4_additional(&values, &mut out);
            }
        }
        "mp3" | "wav" | "wave" | "aif" | "aiff" | "aac" | "tta" | "dsf" | "dff" => {
            if let Some((view, _)) = id3_tag_payload(&data, path) {
                apply_id3_additional(&view, &mut out);
            }
        }
        _ => {}
    }
    out
}

fn lookup_any(comments: &[(String, Vec<String>)], keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| {
            comments
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_default()
}

/// Inserts a non-empty value under `key` (defaults stay "").
fn put(out: &mut BTreeMap<String, String>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
        out.insert(key.to_string(), value);
    }
}

fn apply_xiph_additional(comments: &[(String, Vec<String>)], out: &mut BTreeMap<String, String>) {
    put(out, "TITLE_SORT", join_values(&lookup_any(comments, &["TITLESORT", "TITLE_SORT", "SORTNAME"])));
    put(out, "SUBTITLE", join_values(&lookup_any(comments, &["SUBTITLE"])));
    put(out, "DESCRIPTION", join_values(&lookup_any(comments, &["DESCRIPTION"])));
    put(out, "ARTIST_SORT", join_values(&lookup_any(comments, &["ARTISTSORT", "ARTIST_SORT", "SORTARTIST"])));
    put(out, "ARTIST_ROLE", join_values(&lookup_any(comments, &["ARTIST_ROLE", "PERFORMER_ROLE"])));
    put(out, "ALBUM_ARTIST_SORT", join_values(&lookup_any(comments, &["ALBUMARTISTSORT", "ALBUM_ARTIST_SORT"])));
    put(out, "COMPOSERS", join_values(&lookup_any(comments, &["COMPOSER", "COMPOSERS"])));
    put(out, "COMPOSER_SORT", join_values(&lookup_any(comments, &["COMPOSERSORT", "COMPOSER_SORT"])));
    put(out, "ALBUM_SORT", join_values(&lookup_any(comments, &["ALBUMSORT", "ALBUM_SORT", "SORTALBUM"])));
    put(out, "COMMENT", join_values(&lookup_any(comments, &["COMMENT"])));
    put(out, "TRACK_COUNT", join_values(&lookup_any(comments, &["TRACKTOTAL", "TRACKCOUNT", "TOTALTRACKS"])));
    put(out, "DISC", join_values(&lookup_any(comments, &["DISCNUMBER", "DISC"])));
    put(out, "DISC_COUNT", join_values(&lookup_any(comments, &["DISCTOTAL", "DISCCOUNT", "TOTALDISCS"])));
    put(out, "GROUPING", join_values(&lookup_any(comments, &["GROUPING", "CONTENTGROUP"])));
    put(out, "BPM", join_values(&lookup_any(comments, &["BPM", "BEATSPERMINUTE"])));
    put(out, "CONDUCTOR", join_values(&lookup_any(comments, &["CONDUCTOR"])));
    put(out, "COPYRIGHT", join_values(&lookup_any(comments, &["COPYRIGHT"])));
    put(out, "MUSICBRAINZ_ARTIST_ID", join_values(&lookup_any(comments, &["MUSICBRAINZ_ARTIST_ID"])));
    put(out, "MUSICBRAINZ_RELEASE_GROUP_ID", join_values(&lookup_any(comments, &["MUSICBRAINZ_RELEASE_GROUP_ID"])));
    put(out, "MUSICBRAINZ_RELEASE_ID", join_values(&lookup_any(comments, &["MUSICBRAINZ_RELEASE_ID", "MUSICBRAINZ_ALBUM_ID"])));
    put(out, "MUSICBRAINZ_RELEASE_ARTIST_ID", join_values(&lookup_any(comments, &["MUSICBRAINZ_RELEASE_ARTIST_ID", "MUSICBRAINZ_ALBUM_ARTIST_ID"])));
    put(out, "MUSICBRAINZ_TRACK_ID", join_values(&lookup_any(comments, &["MUSICBRAINZ_TRACK_ID"])));
    put(out, "MUSICBRAINZ_DISC_ID", join_values(&lookup_any(comments, &["MUSICBRAINZ_DISC_ID"])));
    put(out, "MUSICIP_ID", join_values(&lookup_any(comments, &["MUSICIP_ID"])));
    put(out, "AMAZON_ID", join_values(&lookup_any(comments, &["AMAZON_ID"])));
    put(out, "MUSICBRAINZ_RELEASE_STATUS", join_values(&lookup_any(comments, &["MUSICBRAINZ_RELEASE_STATUS", "MUSICBRAINZ_ALBUM_STATUS"])));
    put(out, "MUSICBRAINZ_RELEASE_TYPE", join_values(&lookup_any(comments, &["MUSICBRAINZ_RELEASE_TYPE", "MUSICBRAINZ_ALBUM_TYPE"])));
    put(out, "MUSICBRAINZ_RELEASE_COUNTRY", join_values(&lookup_any(comments, &["MUSICBRAINZ_RELEASE_COUNTRY", "MUSICBRAINZ_ALBUM_RELEASE_COUNTRY"])));
    put(out, "DATE_TAGGED", join_values(&lookup_any(comments, &["DATE_TAGGED"])));
    put(out, "REPLAYGAIN_TRACK_GAIN", join_values(&lookup_any(comments, &["REPLAYGAIN_TRACK_GAIN"])));
    put(out, "REPLAYGAIN_TRACK_PEAK", join_values(&lookup_any(comments, &["REPLAYGAIN_TRACK_PEAK"])));
    put(out, "REPLAYGAIN_ALBUM_GAIN", join_values(&lookup_any(comments, &["REPLAYGAIN_ALBUM_GAIN"])));
    put(out, "REPLAYGAIN_ALBUM_PEAK", join_values(&lookup_any(comments, &["REPLAYGAIN_ALBUM_PEAK"])));
    put(out, "INITIAL_KEY", join_values(&lookup_any(comments, &["INITIAL_KEY", "KEY"])));
    put(out, "REMIXED_BY", join_values(&lookup_any(comments, &["REMIXED_BY"])));
    put(out, "PUBLISHER", join_values(&lookup_any(comments, &["PUBLISHER"])));
    put(out, "ISRC", join_values(&lookup_any(comments, &["ISRC"])));
}

fn apply_asf_additional(view: &AsfView, out: &mut BTreeMap<String, String>) {
    put(out, "TITLE_SORT", asf_lookup(view, "WM/TitleSort"));
    put(out, "SUBTITLE", asf_lookup(view, "WM/SubTitle"));
    put(out, "DESCRIPTION", asf_lookup(view, "WM/Description"));
    put(out, "ARTIST_SORT", asf_lookup(view, "WM/ArtistSort"));
    put(out, "ALBUM_ARTIST_SORT", asf_lookup(view, "WM/AlbumArtistSort"));
    put(out, "COMPOSERS", asf_lookup(view, "WM/Composers"));
    put(out, "COMPOSER_SORT", asf_lookup(view, "WM/ComposerSort"));
    put(out, "ALBUM_SORT", asf_lookup(view, "WM/AlbumSort"));
    put(out, "COMMENT", asf_lookup(view, "WM/Comment"));
    put(out, "TRACK_COUNT", asf_lookup(view, "WM/TrackCount"));
    put(out, "DISC", asf_lookup(view, "WM/PartOfSet"));
    put(out, "GROUPING", asf_lookup(view, "WM/ContentGroupDescription"));
    put(out, "BPM", asf_lookup(view, "WM/BeatsPerMinute"));
    put(out, "CONDUCTOR", asf_lookup(view, "WM/Conductor"));
    put(out, "COPYRIGHT", asf_lookup(view, "WM/Copyright"));
    put(out, "MUSICBRAINZ_ARTIST_ID", asf_lookup(view, "MusicBrainz/Artist Id"));
    put(out, "MUSICBRAINZ_RELEASE_GROUP_ID", asf_lookup(view, "MusicBrainz/Release Group Id"));
    put(out, "MUSICBRAINZ_RELEASE_ID", asf_lookup(view, "MusicBrainz/Album Id"));
    put(out, "MUSICBRAINZ_RELEASE_ARTIST_ID", asf_lookup(view, "MusicBrainz/Album Artist Id"));
    put(out, "MUSICBRAINZ_TRACK_ID", asf_lookup(view, "MusicBrainz/Track Id"));
    put(out, "MUSICBRAINZ_DISC_ID", asf_lookup(view, "MusicBrainz/Disc Id"));
    put(out, "MUSICBRAINZ_RELEASE_STATUS", asf_lookup(view, "MusicBrainz/Album Status"));
    put(out, "MUSICBRAINZ_RELEASE_TYPE", asf_lookup(view, "MusicBrainz/Album Type"));
    put(out, "MUSICBRAINZ_RELEASE_COUNTRY", asf_lookup(view, "MusicBrainz/Album Release Country"));
    put(out, "INITIAL_KEY", asf_lookup(view, "WM/InitialKey"));
    put(out, "REMIXED_BY", asf_lookup(view, "WM/ModifiedBy"));
    put(out, "PUBLISHER", asf_lookup(view, "WM/Publisher"));
    put(out, "ISRC", asf_lookup(view, "WM/ISRC"));
}

fn apply_ape_additional(items: &[(String, Vec<String>)], out: &mut BTreeMap<String, String>) {
    put(out, "TITLE_SORT", join_values(&lookup_any(items, &["TITLE_SORT", "TITLESORT", "SORTNAME", "Sort Title"])));
    put(out, "SUBTITLE", join_values(&lookup_any(items, &["SUBTITLE", "Subtitle"])));
    put(out, "DESCRIPTION", join_values(&lookup_any(items, &["DESCRIPTION", "Description"])));
    put(out, "ARTIST_SORT", join_values(&lookup_any(items, &["ARTIST_SORT", "ARTISTSORT", "SORTARTIST", "Sort Artist"])));
    put(out, "ARTIST_ROLE", join_values(&lookup_any(items, &["ARTIST_ROLE", "PERFORMER_ROLE"])));
    put(out, "ALBUM_ARTIST_SORT", join_values(&lookup_any(items, &["ALBUM_ARTIST_SORT", "ALBUMARTISTSORT", "Sort Album Artist"])));
    put(out, "COMPOSERS", join_values(&lookup_any(items, &["COMPOSER", "COMPOSERS"])));
    put(out, "COMPOSER_SORT", join_values(&lookup_any(items, &["COMPOSER_SORT", "COMPOSERSORT", "Sort Composer"])));
    put(out, "ALBUM_SORT", join_values(&lookup_any(items, &["ALBUM_SORT", "ALBUMSORT", "SORTALBUM", "Sort Album"])));
    put(out, "COMMENT", join_values(&lookup_any(items, &["COMMENT"])));
    put(out, "TRACK_COUNT", join_values(&lookup_any(items, &["TRACK_COUNT", "TRACKTOTAL", "TRACKCOUNT"])));
    put(out, "DISC", join_values(&lookup_any(items, &["DISC", "DISCNUMBER"])));
    put(out, "DISC_COUNT", join_values(&lookup_any(items, &["DISC_COUNT", "DISCTOTAL", "DISCCOUNT"])));
    put(out, "GROUPING", join_values(&lookup_any(items, &["GROUPING", "CONTENTGROUP"])));
    put(out, "BPM", join_values(&lookup_any(items, &["BPM", "BEATSPERMINUTE"])));
    put(out, "CONDUCTOR", join_values(&lookup_any(items, &["CONDUCTOR"])));
    put(out, "COPYRIGHT", join_values(&lookup_any(items, &["COPYRIGHT"])));
    put(out, "MUSICBRAINZ_ARTIST_ID", join_values(&lookup_any(items, &["MUSICBRAINZ_ARTIST_ID", "MusicBrainz Artist Id"])));
    put(out, "MUSICBRAINZ_RELEASE_GROUP_ID", join_values(&lookup_any(items, &["MUSICBRAINZ_RELEASE_GROUP_ID", "MusicBrainz Release Group Id"])));
    put(out, "MUSICBRAINZ_RELEASE_ID", join_values(&lookup_any(items, &["MUSICBRAINZ_RELEASE_ID", "MusicBrainz Album Id"])));
    put(out, "MUSICBRAINZ_RELEASE_ARTIST_ID", join_values(&lookup_any(items, &["MUSICBRAINZ_RELEASE_ARTIST_ID", "MusicBrainz Album Artist Id"])));
    put(out, "MUSICBRAINZ_TRACK_ID", join_values(&lookup_any(items, &["MUSICBRAINZ_TRACK_ID", "MusicBrainz Track Id"])));
    put(out, "MUSICBRAINZ_DISC_ID", join_values(&lookup_any(items, &["MUSICBRAINZ_DISC_ID", "MusicBrainz Disc Id"])));
    put(out, "MUSICIP_ID", join_values(&lookup_any(items, &["MUSICIP_ID", "MusicIP Id"])));
    put(out, "AMAZON_ID", join_values(&lookup_any(items, &["AMAZON_ID", "Amazon Id"])));
    put(out, "MUSICBRAINZ_RELEASE_STATUS", join_values(&lookup_any(items, &["MUSICBRAINZ_RELEASE_STATUS", "MusicBrainz Album Status"])));
    put(out, "MUSICBRAINZ_RELEASE_TYPE", join_values(&lookup_any(items, &["MUSICBRAINZ_RELEASE_TYPE", "MusicBrainz Album Type"])));
    put(out, "MUSICBRAINZ_RELEASE_COUNTRY", join_values(&lookup_any(items, &["MUSICBRAINZ_RELEASE_COUNTRY", "MusicBrainz Album Release Country"])));
    put(out, "DATE_TAGGED", join_values(&lookup_any(items, &["DATE_TAGGED"])));
    put(out, "REPLAYGAIN_TRACK_GAIN", join_values(&lookup_any(items, &["REPLAYGAIN_TRACK_GAIN"])));
    put(out, "REPLAYGAIN_TRACK_PEAK", join_values(&lookup_any(items, &["REPLAYGAIN_TRACK_PEAK"])));
    put(out, "REPLAYGAIN_ALBUM_GAIN", join_values(&lookup_any(items, &["REPLAYGAIN_ALBUM_GAIN"])));
    put(out, "REPLAYGAIN_ALBUM_PEAK", join_values(&lookup_any(items, &["REPLAYGAIN_ALBUM_PEAK"])));
    put(out, "INITIAL_KEY", join_values(&lookup_any(items, &["INITIAL_KEY"])));
    put(out, "REMIXED_BY", join_values(&lookup_any(items, &["REMIXED_BY"])));
    put(out, "PUBLISHER", join_values(&lookup_any(items, &["PUBLISHER"])));
    put(out, "ISRC", join_values(&lookup_any(items, &["ISRC"])));
}

/// Extracts the raw ilst child values (key → payloads as text) for additional reads.
fn mp4_ilst_values(data: &[u8]) -> Option<(BTreeMap<String, Vec<String>>, ())> {
    let moov = find_atom_payload(data, b"moov", 0)?;
    let udta = find_atom_payload(moov, b"udta", 0)?;
    let meta = find_atom_payload(udta, b"meta", 4)?;
    let ilst = find_atom_payload(meta, b"ilst", 0)?;
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut offset = 0usize;
    while offset + 8 <= ilst.len() {
        let size = u32::from_be_bytes([ilst[offset], ilst[offset + 1], ilst[offset + 2], ilst[offset + 3]]) as usize;
        if size < 8 || offset + size > ilst.len() {
            break;
        }
        let name = decode_ilst_name(&ilst[offset + 4..offset + 8]);
        let body = &ilst[offset + 8..offset + size];
        offset += size;
        let key = if name == "----" {
            mp4_freeform_key(body)?
        } else {
            name.clone()
        };
        for payload in mp4_data_payloads(body) {
            if let Ok(text) = String::from_utf8(payload) {
                let text = text.trim().to_string();
                if !text.is_empty() {
                    values.entry(key.clone()).or_default().push(text);
                }
            }
        }
    }
    Some((values, ()))
}

fn apply_mp4_additional(values: &BTreeMap<String, Vec<String>>, out: &mut BTreeMap<String, String>) {
    let get = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter_map(|n| values.get(*n).cloned())
            .flat_map(|v| v.into_iter())
            .collect()
    };
    let ff = |key: &str| format!("----:com.apple.iTunes:{}", key);
    put(out, "TITLE_SORT", join_values(&get(&[&ff("TITLE_SORT")])));
    put(out, "SUBTITLE", join_values(&get(&[&ff("SUBTITLE")])));
    put(out, "DESCRIPTION", join_values(&get(&[&ff("DESCRIPTION")])));
    put(out, "ARTIST_SORT", join_values(&get(&[&ff("ARTIST_SORT")])));
    put(out, "ARTIST_ROLE", join_values(&get(&[&ff("ARTIST_ROLE")])));
    put(out, "ALBUM_ARTIST_SORT", join_values(&get(&[&ff("ALBUM_ARTIST_SORT")])));
    put(out, "COMPOSERS", join_values(&get(&["\u{a9}wrt", &ff("COMPOSERS")])));
    put(out, "COMPOSER_SORT", join_values(&get(&[&ff("COMPOSER_SORT")])));
    put(out, "ALBUM_SORT", join_values(&get(&[&ff("ALBUM_SORT")])));
    put(out, "COMMENT", join_values(&get(&["\u{a9}cmt", &ff("COMMENT")])));
    put(out, "TRACK_COUNT", join_values(&get(&[&ff("TRACK_COUNT"), &ff("TRACKTOTAL")])));
    put(out, "DISC", join_values(&get(&[&ff("DISC"), &ff("DISCNUMBER")])));
    put(out, "DISC_COUNT", join_values(&get(&[&ff("DISC_COUNT"), &ff("DISCTOTAL")])));
    put(out, "GROUPING", join_values(&get(&["\u{a9}grp", &ff("GROUPING")])));
    put(out, "BPM", join_values(&get(&["tmpo", &ff("BPM")])));
    put(out, "CONDUCTOR", join_values(&get(&[&ff("CONDUCTOR")])));
    put(out, "COPYRIGHT", join_values(&get(&["cprt", &ff("COPYRIGHT")])));
    for (key, _) in ADDITIONAL_STANDARD_FIELDS {
        if !out.contains_key(*key) {
            let name = ff(key);
            put(out, key, join_values(&get(&[&name])));
        }
    }
}

fn apply_id3_additional(view: &Id3View, out: &mut BTreeMap<String, String>) {
    let frame = |id: &str| -> Vec<String> { view.frame(id) };
    let user = |descriptions: &[&str]| -> Vec<String> {
        descriptions
            .iter()
            .find_map(|d| {
                view.user_text
                    .iter()
                    .find(|(desc, _)| desc.eq_ignore_ascii_case(d))
                    .map(|(_, values)| values.clone())
            })
            .unwrap_or_default()
    };
    put(out, "TITLE_SORT", join_values(&frame("TSOT")));
    put(out, "SUBTITLE", join_values(&frame("TIT3")));
    put(out, "DESCRIPTION", join_values(&frame("TIT3")));
    put(out, "ARTIST_SORT", join_values(&frame("TSOP")));
    put(out, "ARTIST_ROLE", join_values(&frame("TMCL")));
    put(out, "ALBUM_ARTIST_SORT", join_values(&frame("TSO2")));
    put(out, "COMPOSERS", join_values(&frame("TCOM")));
    put(out, "COMPOSER_SORT", join_values(&frame("TSOC")));
    put(out, "ALBUM_SORT", join_values(&frame("TSOA")));
    put(out, "COMMENT", join_values(&frame("COMM")));
    // TRCK/TPOS carry "n/total"; split into the numeric parts.
    if let Some(value) = view.frame("TRCK").first() {
        let parts: Vec<&str> = value.split('/').collect();
        if parts.len() > 1 {
            put(out, "TRACK_COUNT", Some(parts[1].trim().to_string()));
        }
    }
    if let Some(value) = view.frame("TPOS").first() {
        let parts: Vec<&str> = value.split('/').collect();
        if let Some(first) = parts.first() {
            put(out, "DISC", Some(first.trim().to_string()));
        }
        if parts.len() > 1 {
            put(out, "DISC_COUNT", Some(parts[1].trim().to_string()));
        }
    }
    put(out, "GROUPING", join_values(&frame("TIT1")));
    put(out, "BPM", join_values(&frame("TBPM")));
    put(out, "CONDUCTOR", join_values(&frame("TPE3")));
    put(out, "COPYRIGHT", join_values(&frame("TCOP")));
    put(out, "MUSICBRAINZ_ARTIST_ID", join_values(&user(&["MusicBrainz Artist Id"])));
    put(out, "MUSICBRAINZ_RELEASE_GROUP_ID", join_values(&user(&["MusicBrainz Release Group Id"])));
    put(out, "MUSICBRAINZ_RELEASE_ID", join_values(&user(&["MusicBrainz Album Id"])));
    put(out, "MUSICBRAINZ_RELEASE_ARTIST_ID", join_values(&user(&["MusicBrainz Album Artist Id"])));
    put(out, "MUSICBRAINZ_TRACK_ID", join_values(&user(&["MusicBrainz Track Id"])));
    put(out, "MUSICBRAINZ_DISC_ID", join_values(&user(&["MusicBrainz Disc Id"])));
    put(out, "MUSICIP_ID", join_values(&user(&["MusicIp Id"])));
    put(out, "AMAZON_ID", join_values(&user(&["Amazon Id"])));
    put(out, "MUSICBRAINZ_RELEASE_STATUS", join_values(&user(&["MusicBrainz Album Status"])));
    put(out, "MUSICBRAINZ_RELEASE_TYPE", join_values(&user(&["MusicBrainz Album Type"])));
    put(out, "MUSICBRAINZ_RELEASE_COUNTRY", join_values(&user(&["MusicBrainz Album Release Country"])));
    put(out, "DATE_TAGGED", join_values(&frame("TDRC")));
    put(out, "REPLAYGAIN_TRACK_GAIN", join_values(&user(&["REPLAYGAIN_TRACK_GAIN"])));
    put(out, "REPLAYGAIN_TRACK_PEAK", join_values(&user(&["REPLAYGAIN_TRACK_PEAK"])));
    put(out, "REPLAYGAIN_ALBUM_GAIN", join_values(&user(&["REPLAYGAIN_ALBUM_GAIN"])));
    put(out, "REPLAYGAIN_ALBUM_PEAK", join_values(&user(&["REPLAYGAIN_ALBUM_PEAK"])));
    put(out, "INITIAL_KEY", join_values(&frame("TKEY")));
    put(out, "REMIXED_BY", join_values(&frame("TPE4")));
    put(out, "PUBLISHER", join_values(&frame("TPUB")));
    put(out, "ISRC", join_values(&frame("TSRC")));
}

/// Keeps track of whether artwork extraction has been globally disabled
/// (thumbnail-only paths can set this to avoid disk churn).
static ARTWORK_EXTRACTION_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_artwork_extraction_enabled(enabled: bool) {
    ARTWORK_EXTRACTION_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn artwork_extraction_enabled() -> bool {
    ARTWORK_EXTRACTION_ENABLED.load(Ordering::Relaxed)
}
