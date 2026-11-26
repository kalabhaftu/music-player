//! DSDIFF (.dff) duration reader — 1:1 port of `DffAudio.ReadDuration`.
//!
//! Walks the FRM8 chunk tree; duration = audio bytes * 8 / sample_rate /
//! channels, valid only for uncompressed ("DSD ") streams. Max 256 chunks;
//! malformed files yield 0.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

const MAX_CHUNKS: usize = 256;

pub fn read_dff_duration_ms(path: &Path) -> u64 {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut reader = BufReader::with_capacity(64 * 1024, file);

    let mut header = [0u8; 16];
    if reader.read_exact(&mut header).is_err() {
        return 0;
    }
    if &header[0..4] != b"FRM8" || &header[12..16] != b"DSD " {
        return 0;
    }

    let mut sample_rate: Option<u32> = None;
    let mut channels: Option<u16> = None;
    let mut uncompressed = false;
    let mut properties_end: Option<u64> = None;
    let file_len = reader.get_ref().metadata().map(|m| m.len()).unwrap_or(0);

    let mut chunk_id = [0u8; 4];
    for _ in 0..MAX_CHUNKS {
        if reader.read_exact(&mut chunk_id).is_err() {
            break;
        }
        let mut len_bytes = [0u8; 8];
        if reader.read_exact(&mut len_bytes).is_err() {
            break;
        }
        let len = u64::from_be_bytes(len_bytes);

        if len > file_len {
            break;
        }

        match &chunk_id {
            b"PROP" => {
                if len < 4 {
                    break;
                }
                let mut prop_tag = [0u8; 4];
                if reader.read_exact(&mut prop_tag).is_err() {
                    break;
                }
                if &prop_tag != b"SND " {
                    break;
                }
                properties_end = Some(reader.stream_position().unwrap_or(0) + len - 4);
                // Skip remaining payload
                let payload = len - 4;
                if reader.seek_relative(payload as i64).is_err() {
                    break;
                }
            }
            b"FS  " => {
                if len == 4 {
                    let mut rate = [0u8; 4];
                    if reader.read_exact(&mut rate).is_ok() {
                        sample_rate = Some(u32::from_be_bytes(rate));
                    }
                } else {
                    let _ = reader.seek_relative(len as i64);
                }
            }
            b"CHNL" => {
                if len >= 2 {
                    let mut count = [0u8; 2];
                    if reader.read_exact(&mut count).is_ok() {
                        channels = Some(u16::from_be_bytes(count));
                    }
                    let rest = len - 2;
                    if reader.seek_relative(rest as i64).is_err() {
                        break;
                    }
                } else {
                    let _ = reader.seek_relative(len as i64);
                }
            }
            b"CMPR" => {
                if len >= 4 {
                    let mut compression = [0u8; 4];
                    if reader.read_exact(&mut compression).is_ok() && &compression == b"DSD " {
                        uncompressed = true;
                    }
                    let rest = len - 4;
                    if reader.seek_relative(rest as i64).is_err() {
                        break;
                    }
                } else {
                    let _ = reader.seek_relative(len as i64);
                }
            }
            b"DSD " => {
                if let (true, Some(rate), Some(channels)) = (uncompressed, sample_rate, channels) {
                    if rate == 0 || channels == 0 {
                        return 0;
                    }
                    let ticks = (len as u128)
                        .saturating_mul(8)
                        .saturating_mul(10_000_000u128)
                        .checked_div(u128::from(rate) * u128::from(channels));
                    if let Some(ticks) = ticks {
                        if ticks < u128::from(i64::MAX as u64) {
                            return (ticks / 10_000) as u64; // ticks -> ms
                        }
                    }
                }
                return 0;
            }
            _ => {
                if reader.seek_relative(len as i64).is_err() {
                    break;
                }
            }
        }

        // Odd-length chunks are padded to even boundaries.
        if len % 2 == 1 {
            let _ = reader.seek_relative(1);
        }

        if let Some(end) = properties_end {
            let pos = reader.stream_position().unwrap_or(0);
            if pos >= end {
                properties_end = None;
            }
        }
    }

    0
}

/// Locates the `ID3 ` chunk inside a DFF buffer: (payload start, payload length).
pub fn dff_id3_payload(data: &[u8]) -> Option<(usize, usize)> {
    if data.len() < 16 || &data[0..4] != b"FRM8" || &data[12..16] != b"DSD " {
        return None;
    }
    let file_len = data.len() as u64;
    let mut offset = 16usize;
    for _ in 0..MAX_CHUNKS {
        if offset + 12 > data.len() {
            break;
        }
        let chunk_id = &data[offset..offset + 4];
        let len = u64::from_be_bytes([
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
            data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11],
        ]);
        if len > file_len {
            break;
        }
        if chunk_id == b"ID3 " {
            let start = offset + 12;
            if start + len as usize <= data.len() {
                return Some((start, len as usize));
            }
            break;
        }
        // Skip the chunk payload (pad to even boundary).
        let mut advance = 12 + len as usize;
        if len % 2 == 1 {
            advance += 1;
        }
        offset += advance;
    }
    None
}
