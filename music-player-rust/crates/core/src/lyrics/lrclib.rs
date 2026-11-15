use std::time::{Duration, Instant};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LyricsSearchResult {
    pub id: u64,
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    pub album_name: Option<String>,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

/// Normalizes song titles and artist names for accurate lyrics matching.
/// (1:1 port of LyricsSearchQuery.cs)
pub fn normalize_query(title: &str, artist: &str) -> (String, String) {
    let mut clean_title = title.trim().to_string();
    let mut clean_artist = artist.trim().to_string();

    if clean_artist.is_empty()
        || clean_artist.eq_ignore_ascii_case("Unknown artist")
        || clean_artist.eq_ignore_ascii_case("Unknown")
    {
        clean_artist.clear();
        if let Some(pos) = clean_title.find(" - ") {
            if pos > 0 && pos < clean_title.len() - 3 {
                clean_artist = clean_title[..pos].trim().to_string();
                clean_title = clean_title[(pos + 3)..].trim().to_string();
            }
        }
    } else {
        let prefix = format!("{} - ", clean_artist);
        if clean_title.to_lowercase().starts_with(&prefix.to_lowercase()) {
            clean_title = clean_title[prefix.len()..].to_string();
        }
    }

    // Strip bracketed decorations: (official video), [remix], etc.
    let bracketed_re = Regex::new(r"(?i)\([^)]*\)|\[[^\]]*\]|\{[^}]*\}").unwrap();
    let decor_re = Regex::new(r"(?i)\b(?:remix|reverb(?:ed)?|slowed|sped[\s-]*up|speed[\s-]*up|nightcore|bass[\s-]*boosted|8d|official|lyric(?:s)?|audio|music\s+video|visuali[sz]er|hd|hq|\d{2,4}\s*kbps?|\d{2,4}k)\b").unwrap();

    let stripped = bracketed_re.replace_all(&clean_title, |caps: &regex::Captures| {
        if decor_re.is_match(&caps[0]) {
            " ".to_string()
        } else {
            caps[0].to_string()
        }
    });

    // Strip trailing decorations: - Slowed & Reverb, - Official Audio, etc.
    let suffix_re = Regex::new(r"(?i)(?:\s*[-–—|+]\s*|\s+)(?:slowed|reverb(?:ed)?|sped[\s-]*up|speed[\s-]*up|nightcore|remix|official\s+(?:audio|music\s+video|video)|lyrics?\s+video)\b.*$").unwrap();
    let without_suffix = suffix_re.replace(&stripped, "");

    let ws_re = Regex::new(r"\s+").unwrap();
    let normalized = ws_re.replace_all(&without_suffix, " ");
    let final_title = normalized.trim_matches(|c| c == ' ' || c == '-' || c == '–' || c == '—' || c == '|' || c == '+').to_string();

    (final_title, clean_artist)
}

/// Client for searching timed and plain lyrics from lrclib.net with rate limiting.
/// (1:1 port of LrclibSearchClient.cs)
pub struct LrclibSearchClient {
    min_interval: Duration,
    next_request: Option<Instant>,
    backoff_until: Option<Instant>,
}

impl Default for LrclibSearchClient {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_millis(1300),
            next_request: None,
            backoff_until: None,
        }
    }
}

impl LrclibSearchClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks if a request is currently allowed under rate limits and backoff.
    pub fn can_request(&self, now: Instant) -> Result<(), Duration> {
        if let Some(backoff) = self.backoff_until {
            if now < backoff {
                return Err(backoff - now);
            }
        }
        if let Some(next) = self.next_request {
            if now < next {
                return Err(next - now);
            }
        }
        Ok(())
    }

    /// Records that a request was sent, advancing the rate limit window.
    pub fn record_request(&mut self, now: Instant) {
        self.next_request = Some(now + self.min_interval);
    }

    /// Sets backoff cooldown (e.g. on HTTP 429 Too Many Requests).
    pub fn set_backoff(&mut self, now: Instant, duration: Duration) {
        self.backoff_until = Some(now + duration);
    }

    /// Builds the target search URL.
    pub fn build_search_url(&self, title: &str, artist: &str) -> String {
        let (norm_title, norm_artist) = normalize_query(title, artist);
        format!(
            "https://lrclib.net/api/search?track_name={}&artist_name={}",
            urlencoding(&norm_title),
            urlencoding(&norm_artist)
        )
    }

    /// Performs the search and parses results using native JSON deserialization.
    pub fn search(&mut self, title: &str, artist: &str) -> Result<Vec<LyricsSearchResult>, String> {
        let now = Instant::now();
        if let Err(delay) = self.can_request(now) {
            std::thread::sleep(delay);
        }
        self.record_request(Instant::now());

        let url = self.build_search_url(title, artist);

        // Fetch using Windows PowerShell Invoke-RestMethod for zero external C-dependencies
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; (Invoke-WebRequest -Uri '{}' -UseBasicParsing -TimeoutSec 15).Content",
                    url.replace('\'', "''")
                ),
            ])
            .output()
            .map_err(|e| format!("Failed to execute search: {}", e))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            if err.contains("429") {
                self.set_backoff(Instant::now(), Duration::from_secs(15));
                return Err("LRCLIB rate limit reached (HTTP 429). Please retry shortly.".into());
            }
            return Err(format!("Network error: {}", err));
        }

        let json_text = String::from_utf8_lossy(&output.stdout);
        let results: Vec<LyricsSearchResult> = serde_json::from_str(&json_text)
            .unwrap_or_default();

        Ok(results)
    }
}

fn urlencoding(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push_str("%20"),
            _ => encoded.push_str(&format!("%{:02X}", b)),
        }
    }
    encoded
}
