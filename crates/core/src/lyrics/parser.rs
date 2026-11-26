use crate::models::{LyricsDocument, LyricsLine};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\d+):(\d{1,2})(?:[.:](\d+))?\]").unwrap()
});

static METADATA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\[([a-z][a-z0-9_-]*):([^\]]*)\]$").unwrap()
});

static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\[\s*(?:verse|chorus|bridge|intro|outro|hook|pre-chorus|post-chorus|interlude|solo|breakdown|drop|refrain|tag|part)\b[^\]]*\]\s*").unwrap()
});

pub fn parse_lrc(text: &str) -> LyricsDocument {
    let mut lines = Vec::new();
    let mut metadata = HashMap::new();
    let mut offset_ms: i64 = 0;

    let clean_text = text.trim_start_matches('\u{FEFF}').replace("\r\n", "\n").replace('\r', "\n");

    for raw_line in clean_text.lines() {
        let trimmed = raw_line.trim();

        // Check if line is purely a section tag
        if SECTION_RE.is_match(trimmed) {
            continue;
        }

        // Check metadata tag (e.g. [ar:Artist], [offset:150])
        if let Some(caps) = METADATA_RE.captures(trimmed) {
            let key = caps.get(1).map_or("", |m| m.as_str());
            let val = caps.get(2).map_or("", |m| m.as_str()).trim();

            if key.eq_ignore_ascii_case("offset") {
                if let Ok(ms) = val.parse::<i64>() {
                    offset_ms = ms;
                }
            } else {
                metadata.insert(key.to_string(), val.to_string());
            }
            continue;
        }

        // Extract timestamps
        let matches: Vec<_> = TIMESTAMP_RE.find_iter(raw_line).collect();
        if matches.is_empty() {
            continue;
        }

        // Collect lyric text following the last timestamp
        let last_match = matches.last().unwrap();
        let lyric_start = last_match.end();
        let lyric_text = raw_line[lyric_start..].trim();
        let clean_lyric = SECTION_RE.replace_all(lyric_text, "").trim().to_string();

        if clean_lyric.is_empty() {
            continue;
        }

        for m in &matches {
            if let Some(caps) = TIMESTAMP_RE.captures(m.as_str()) {
                let mins: u64 = caps.get(1).and_then(|c| c.as_str().parse().ok()).unwrap_or(0);
                let secs: u64 = caps.get(2).and_then(|c| c.as_str().parse().ok()).unwrap_or(0);
                let frac_str = caps.get(3).map_or("", |c| c.as_str());

                let ms: u64 = if frac_str.is_empty() {
                    0
                } else if frac_str.len() == 1 {
                    frac_str.parse::<u64>().unwrap_or(0) * 100
                } else if frac_str.len() == 2 {
                    frac_str.parse::<u64>().unwrap_or(0) * 10
                } else {
                    let clamped = &frac_str[..3.min(frac_str.len())];
                    clamped.parse::<u64>().unwrap_or(0)
                };

                let time_ms = mins * 60_000 + secs * 1_000 + ms;
                lines.push(LyricsLine {
                    time_ms,
                    text: clean_lyric.clone(),
                });
            }
        }
    }

    lines.sort_by_key(|l| l.time_ms);
    LyricsDocument {
        lines,
        offset_ms,
        metadata,
    }
}

pub fn plain_text(text: &str) -> String {
    let clean_text = text.trim_start_matches('\u{FEFF}').replace("\r\n", "\n").replace('\r', "\n");
    let mut output = Vec::new();

    for raw in clean_text.lines() {
        let trimmed = raw.trim();
        if METADATA_RE.is_match(trimmed) {
            continue;
        }
        let stripped = TIMESTAMP_RE.replace_all(raw, "");
        let clean = SECTION_RE.replace_all(stripped.trim(), "").trim().to_string();
        if !clean.is_empty() {
            output.push(clean);
        }
    }

    output.join("\n")
}

pub fn format_lrc(doc: &LyricsDocument) -> String {
    let mut out = String::new();

    for (k, v) in &doc.metadata {
        out.push_str(&format!("[{}:{}]\n", k, v));
    }
    if doc.offset_ms != 0 {
        out.push_str(&format!("[offset:{}]\n", doc.offset_ms));
    }

    for line in &doc.lines {
        let mins = line.time_ms / 60_000;
        let secs = (line.time_ms % 60_000) / 1_000;
        let ms = (line.time_ms % 1_000) / 10;
        out.push_str(&format!("[{:02}:{:02}.{:02}]{}\n", mins, secs, ms, line.text));
    }

    out
}
