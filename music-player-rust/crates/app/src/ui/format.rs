use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn fmt_time(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

pub fn fmt_added(epoch_secs: u64) -> String {
    if epoch_secs == 0 {
        return String::new();
    }
    // Compact ISO date — the WebUI shows a short relative date; the exact
    // wording is supplied by the toast layer and is not a behavioral surface.
    let days = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(epoch_secs)
        .saturating_sub(epoch_secs))
        / 86400;
    match days {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        d if d < 7 => format!("{} days ago", d),
        _ => {
            let secs = epoch_secs as i64;
            let date = chrono_fallback_date(secs);
            date
        }
    }
}

pub fn fmt_count(count: u32) -> String {
    let mut grouped = String::new();
    for (index, digit) in count.to_string().chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped.chars().rev().collect()
}

pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} bytes", bytes)
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

pub fn chrono_fallback_date(secs: i64) -> String {
    // chrono is already a transitive core dependency and is available here.
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%b %d, %Y").to_string())
        .unwrap_or_default()
}

pub fn crossfade_label(seconds: i64) -> String {
    match seconds {
        2 => "2 s".into(),
        3 => "3 s".into(),
        5 => "5 s".into(),
        8 => "8 s".into(),
        10 => "10 s".into(),
        _ => "Off".into(),
    }
}

pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

pub fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name(path))
}
