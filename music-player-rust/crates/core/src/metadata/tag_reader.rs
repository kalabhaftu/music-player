use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use crate::metadata::artwork::save_artwork_to_cache;

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
}

/// Reads tags and extracts embedded artwork from an audio file.
pub fn read_tags<P: AsRef<Path>>(path: P) -> ExtractedTags {
    let path_ref = path.as_ref();
    let extension = path_ref
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match extension.as_str() {
        "mp3" => read_id3v2(path_ref).unwrap_or_default(),
        "flac" => read_flac(path_ref).unwrap_or_default(),
        "m4a" | "mp4" => read_mp4(path_ref).unwrap_or_default(),
        _ => ExtractedTags::default(),
    }
}

// ----------------------------------------------------------------------
// ID3v2 Parser (MP3)
// ----------------------------------------------------------------------

fn read_id3v2(path: &Path) -> Option<ExtractedTags> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 10];
    file.read_exact(&mut header).ok()?;

    if &header[0..3] != b"ID3" {
        return None;
    }

    let version = header[3]; // e.g. 3 for ID3v2.3, 4 for ID3v2.4
    let tag_size = ((header[6] as usize & 0x7F) << 21)
        | ((header[7] as usize & 0x7F) << 14)
        | ((header[8] as usize & 0x7F) << 7)
        | (header[9] as usize & 0x7F);

    let mut tag_data = vec![0u8; tag_size];
    file.read_exact(&mut tag_data).ok()?;

    let mut tags = ExtractedTags::default();
    let mut cursor = 0;

    while cursor + 10 < tag_data.len() {
        let frame_id = &tag_data[cursor..cursor + 4];
        if frame_id[0] == 0 {
            break; // Padding
        }

        let frame_size = if version == 4 {
            // Synchsafe integer in ID3v2.4
            ((tag_data[cursor + 4] as usize & 0x7F) << 21)
                | ((tag_data[cursor + 5] as usize & 0x7F) << 14)
                | ((tag_data[cursor + 6] as usize & 0x7F) << 7)
                | (tag_data[cursor + 7] as usize & 0x7F)
        } else {
            // Big-endian 32-bit in ID3v2.3
            ((tag_data[cursor + 4] as usize) << 24)
                | ((tag_data[cursor + 5] as usize) << 16)
                | ((tag_data[cursor + 6] as usize) << 8)
                | (tag_data[cursor + 7] as usize)
        };

        cursor += 10;
        if cursor + frame_size > tag_data.len() || frame_size == 0 {
            break;
        }

        let frame_content = &tag_data[cursor..cursor + frame_size];
        cursor += frame_size;

        match frame_id {
            b"TIT2" => tags.title = decode_text_frame(frame_content),
            b"TPE1" => tags.artist = decode_text_frame(frame_content),
            b"TALB" => tags.album = decode_text_frame(frame_content),
            b"TPE2" => tags.album_artist = decode_text_frame(frame_content),
            b"TCON" => tags.genre = decode_text_frame(frame_content),
            b"TYER" | b"TDRC" => {
                if let Some(text) = decode_text_frame(frame_content) {
                    tags.year = text.chars().take(4).collect::<String>().parse().unwrap_or(0);
                }
            }
            b"TRCK" => {
                if let Some(text) = decode_text_frame(frame_content) {
                    let num_str = text.split('/').next().unwrap_or("");
                    tags.track_number = num_str.trim().parse().unwrap_or(0);
                }
            }
            b"TLEN" => {
                if let Some(text) = decode_text_frame(frame_content) {
                    tags.duration_ms = text.trim().parse().unwrap_or(0);
                }
            }
            b"APIC" => {
                if tags.artwork_path.is_none() {
                    if let Some(image_data) = extract_apic_image(frame_content) {
                        tags.artwork_path = save_artwork_to_cache(&image_data)
                            .ok()
                            .map(|p| p.to_string_lossy().to_string());
                    }
                }
            }
            _ => {}
        }
    }

    Some(tags)
}

fn decode_text_frame(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    let encoding = data[0];
    let payload = &data[1..];

    match encoding {
        0 => {
            // ISO-8859-1
            let text = payload.iter().map(|&b| b as char).collect::<String>();
            Some(text.trim_matches('\0').trim().to_string())
        }
        1 | 2 => {
            // UTF-16 with or without BOM
            let u16_slice: Vec<u16> = payload
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .filter(|&c| c != 0xFEFF && c != 0)
                .collect();
            String::from_utf16(&u16_slice).ok().map(|s| s.trim().to_string())
        }
        3 => {
            // UTF-8
            String::from_utf8(payload.to_vec())
                .ok()
                .map(|s| s.trim_matches('\0').trim().to_string())
        }
        _ => None,
    }
}

fn extract_apic_image(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 {
        return None;
    }
    let encoding = data[0];
    let mut offset = 1;

    // Find end of MIME type (null terminated)
    let mime_end = data[offset..].iter().position(|&b| b == 0)?;
    offset += mime_end + 1; // Skip MIME and null byte

    if offset >= data.len() {
        return None;
    }

    // Skip picture type (1 byte)
    offset += 1;

    // Skip description (null terminated depending on encoding)
    if encoding == 1 || encoding == 2 {
        // UTF-16 null terminator is \0\0 aligned
        while offset + 1 < data.len() {
            if data[offset] == 0 && data[offset + 1] == 0 {
                offset += 2;
                break;
            }
            offset += 2;
        }
    } else {
        // Single null terminator
        let desc_end = data[offset..].iter().position(|&b| b == 0)?;
        offset += desc_end + 1;
    }

    if offset < data.len() {
        Some(data[offset..].to_vec())
    } else {
        None
    }
}

// ----------------------------------------------------------------------
// FLAC Parser
// ----------------------------------------------------------------------

fn read_flac(path: &Path) -> Option<ExtractedTags> {
    let mut file = File::open(path).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;

    if &magic != b"fLaC" {
        return None;
    }

    let mut tags = ExtractedTags::default();

    loop {
        let mut block_header = [0u8; 4];
        if file.read_exact(&mut block_header).is_err() {
            break;
        }

        let is_last = (block_header[0] & 0x80) != 0;
        let block_type = block_header[0] & 0x7F;
        let block_len = ((block_header[1] as usize) << 16)
            | ((block_header[2] as usize) << 8)
            | (block_header[3] as usize);

        let mut block_data = vec![0u8; block_len];
        if file.read_exact(&mut block_data).is_err() {
            break;
        }

        match block_type {
            0 => {
                // STREAMINFO: min_block(2), max_block(2), min_frame(3), max_frame(3),
                // sample_rate(20 bits), channels(3 bits), bits_per_sample(5 bits), total_samples(36 bits)
                if block_data.len() >= 18 {
                    let b = &block_data[10..18];
                    let sample_rate = ((b[0] as u64) << 12) | ((b[1] as u64) << 4) | ((b[2] as u64) >> 4);
                    let total_samples = (((b[3] as u64) & 0x0F) << 32)
                        | ((b[4] as u64) << 24)
                        | ((b[5] as u64) << 16)
                        | ((b[6] as u64) << 8)
                        | (b[7] as u64);

                    if sample_rate > 0 {
                        tags.duration_ms = (total_samples * 1000) / sample_rate;
                    }
                }
            }
            4 => {
                // VORBIS_COMMENT
                parse_vorbis_comment(&block_data, &mut tags);
            }
            6 => {
                // PICTURE
                if tags.artwork_path.is_none() {
                    if let Some(img) = extract_flac_picture(&block_data) {
                        tags.artwork_path = save_artwork_to_cache(&img)
                            .ok()
                            .map(|p| p.to_string_lossy().to_string());
                    }
                }
            }
            _ => {}
        }

        if is_last {
            break;
        }
    }

    Some(tags)
}

fn parse_vorbis_comment(data: &[u8], tags: &mut ExtractedTags) {
    if data.len() < 8 {
        return;
    }
    let vendor_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut offset = 4 + vendor_len;
    if offset + 4 > data.len() {
        return;
    }

    let comment_count = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4;

    for _ in 0..comment_count {
        if offset + 4 > data.len() {
            break;
        }
        let length = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;
        if offset + length > data.len() {
            break;
        }

        if let Ok(comment_str) = std::str::from_utf8(&data[offset..offset + length]) {
            if let Some((key, val)) = comment_str.split_once('=') {
                match key.to_ascii_uppercase().as_str() {
                    "TITLE" => tags.title = Some(val.to_string()),
                    "ARTIST" => tags.artist = Some(val.to_string()),
                    "ALBUM" => tags.album = Some(val.to_string()),
                    "ALBUMARTIST" => tags.album_artist = Some(val.to_string()),
                    "GENRE" => tags.genre = Some(val.to_string()),
                    "DATE" | "YEAR" => {
                        tags.year = val.chars().take(4).collect::<String>().parse().unwrap_or(0);
                    }
                    "TRACKNUMBER" => {
                        let num = val.split('/').next().unwrap_or("");
                        tags.track_number = num.trim().parse().unwrap_or(0);
                    }
                    _ => {}
                }
            }
        }
        offset += length;
    }
}

fn extract_flac_picture(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 32 {
        return None;
    }
    let mut offset = 4; // Skip picture type (4 bytes)
    let mime_len = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4 + mime_len;

    let desc_len = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
    offset += 4 + desc_len;

    offset += 16; // Skip width(4), height(4), depth(4), colors(4)
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

// ----------------------------------------------------------------------
// MP4 / M4A Parser
// ----------------------------------------------------------------------

fn read_mp4(path: &Path) -> Option<ExtractedTags> {
    let mut file = File::open(path).ok()?;
    let mut tags = ExtractedTags::default();

    // Scan for atoms
    scan_mp4_atoms(&mut file, 0, &mut tags);
    Some(tags)
}

fn scan_mp4_atoms(file: &mut File, depth: usize, tags: &mut ExtractedTags) {
    if depth > 10 {
        return;
    }

    let mut header = [0u8; 8];
    while file.read_exact(&mut header).is_ok() {
        let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let name = &header[4..8];

        if size < 8 {
            break;
        }

        let content_size = size - 8;
        let current_pos = file.stream_position().unwrap_or(0);

        match name {
            b"moov" | b"udta" | b"meta" | b"ilst" => {
                // If meta atom, skip 4 bytes version/flags
                if name == b"meta" {
                    let _ = file.seek(SeekFrom::Current(4));
                }
                scan_mp4_atoms(file, depth + 1, tags);
            }
            b"covr" => {
                if tags.artwork_path.is_none() {
                    let mut data = vec![0u8; content_size as usize];
                    if file.read_exact(&mut data).is_ok() {
                        // covr contains child atom 'data' (header is 8 bytes + 8 bytes flags/reserved = 16 bytes)
                        if data.len() > 16 && &data[4..8] == b"data" {
                            let img_bytes = &data[16..];
                            tags.artwork_path = save_artwork_to_cache(img_bytes)
                                .ok()
                                .map(|p| p.to_string_lossy().to_string());
                        }
                    }
                }
            }
            _ => {
                let _ = file.seek(SeekFrom::Start(current_pos + content_size));
            }
        }
    }
}
