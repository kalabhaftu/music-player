use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::Path;
use chrono::{DateTime, Utc};

/// Computes a streaming SHA-256 hash of a file in 1 MB chunks.
/// Returns a 64-character uppercase hexadecimal string.
pub fn hash_file<P: AsRef<Path>>(path: P) -> IoResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash_bytes = hasher.finalize();
    let mut hex_str = String::with_capacity(64);
    for byte in hash_bytes {
        use std::fmt::Write;
        write!(&mut hex_str, "{:02X}", byte).unwrap();
    }
    Ok(hex_str)
}

/// Retrieves the current file size and UTC modification timestamp in ISO 8601 format.
pub fn get_file_state<P: AsRef<Path>>(path: P) -> IoResult<(i64, String)> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len() as i64;
    let modified: DateTime<Utc> = metadata.modified()?.into();
    let modified_str = modified.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    Ok((size, modified_str))
}
