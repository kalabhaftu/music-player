use std::collections::HashSet;
use std::time::{Duration, Instant};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricsMatch {
    pub raw: String,
    pub timed: bool,
    pub score: f64,
    pub result: LyricsSearchResult,
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

pub fn timing_matches(track_duration_secs: f64, candidate_duration_secs: f64) -> bool {
    if track_duration_secs > 0.0 && candidate_duration_secs > 0.0 {
        let diff = (track_duration_secs - candidate_duration_secs).abs();
        let tolerance = 8.0f64.max(track_duration_secs * 0.05);
        diff <= tolerance
    } else {
        true
    }
}

pub fn normalized_words(value: &str) -> Vec<String> {
    let lower = value.to_lowercase().replace('&', " and ");
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in lower.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn lyric_match_score(
    track_title: &str,
    track_artist: &str,
    track_duration_secs: f64,
    result: &LyricsSearchResult,
) -> f64 {
    let (identity_title, identity_artist) = normalize_query(track_title, track_artist);
    let (other_title, other_artist) = normalize_query(&result.track_name, &result.artist_name);

    let title_words: HashSet<String> = normalized_words(&identity_title).into_iter().collect();
    let candidate_words: HashSet<String> = normalized_words(&other_title).into_iter().collect();

    if title_words.is_empty() || candidate_words.is_empty() {
        return -1.0;
    }

    let common = title_words.intersection(&candidate_words).count() as f64;
    let coverage = common / title_words.len() as f64;
    let precision = common / candidate_words.len() as f64;

    // Do not automatically borrow lyrics from a longer/different song title.
    if coverage < 0.9 || precision < 0.8 {
        return -1.0;
    }

    let artist_words: HashSet<String> = normalized_words(&identity_artist).into_iter().collect();
    let other_artist_words: HashSet<String> =
        normalized_words(&other_artist).into_iter().collect();

    let artist_common = artist_words.intersection(&other_artist_words).count() as f64;
    if !artist_words.is_empty() {
        if other_artist_words.is_empty() {
            return -1.0;
        }
        let artist_cov = artist_common / artist_words.len() as f64;
        let artist_prec = artist_common / other_artist_words.len() as f64;
        if artist_cov < 0.45 || (artist_cov < 0.9 && artist_prec < 0.8) {
            return -1.0;
        }
    }

    let is_remix = {
        let title_lower = track_title.to_lowercase();
        title_lower.contains("remix")
            || title_lower.contains("slowed")
            || title_lower.contains("reverb")
            || title_lower.contains("sped up")
            || title_lower.contains("speed up")
            || title_lower.contains("nightcore")
    };

    if !timing_matches(track_duration_secs, result.duration) && !is_remix {
        return -1.0;
    }

    let artist_score = if !artist_words.is_empty() {
        (artist_common / artist_words.len() as f64) * 0.22
    } else {
        0.22
    };

    (coverage * 0.78 + precision * 0.22) * 0.78 + artist_score
}

/// Detects if lyrics contain censored profanity (e.g. `f***`, `b****`, `n****`, `s***`, `***`).
pub fn is_censored_lyrics(text: &str) -> bool {
    let lower = text.to_lowercase();
    for word in lower.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphabetic() || *c == '*' || *c == '-' || *c == '#' || *c == '_')
            .collect();
        if clean.contains('*') || clean.contains('#') || clean.contains('_') || clean.contains('-') {
            if clean.starts_with("f*")
                || clean.starts_with("f-")
                || clean.starts_with("f#")
                || clean.starts_with("b*")
                || clean.starts_with("b-")
                || clean.starts_with("n*")
                || clean.starts_with("n-")
                || clean.starts_with("s*")
                || clean.starts_with("s-")
                || clean.starts_with("p*")
                || clean.starts_with("c*")
                || clean.starts_with("d*")
                || clean.starts_with("a**")
                || clean == "***"
                || clean == "****"
                || clean == "*****"
                || clean.contains("***")
                || clean.contains("---")
            {
                return true;
            }
        }
    }
    false
}

pub fn select_lyrics_match(
    track_title: &str,
    track_artist: &str,
    track_duration_secs: f64,
    results: &[LyricsSearchResult],
) -> Option<LyricsMatch> {
    let mut candidates: Vec<LyricsMatch> = results
        .iter()
        .filter_map(|result| {
            let synced = result.synced_lyrics.as_deref().unwrap_or("").trim();
            let plain = result.plain_lyrics.as_deref().unwrap_or("").trim();
            let parsed = crate::lyrics::parser::parse_lrc(synced);
            let timed =
                !parsed.lines.is_empty() && timing_matches(track_duration_secs, result.duration);
            let raw = if timed {
                synced.to_string()
            } else if !plain.is_empty() {
                plain.to_string()
            } else if !parsed.lines.is_empty() {
                crate::lyrics::parser::plain_text(synced)
            } else {
                String::new()
            };

            if raw.is_empty() {
                return None;
            }

            let score = lyric_match_score(track_title, track_artist, track_duration_secs, result);
            if score < 0.0 {
                return None;
            }

            Some(LyricsMatch {
                raw,
                timed,
                score,
                result: result.clone(),
            })
        })
        .collect();

    candidates.sort_by(|a, b| {
        let a_censored = is_censored_lyrics(&a.raw);
        let b_censored = is_censored_lyrics(&b.raw);
        b.timed
            .cmp(&a.timed)
            .then_with(|| a_censored.cmp(&b_censored))
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    candidates.into_iter().next()
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
        format!(
            "https://lrclib.net/api/search?track_name={}&artist_name={}",
            urlencoding(title),
            urlencoding(artist)
        )
    }

    /// Executes raw LRCLIB query.
    pub fn search_raw(&mut self, title: &str, artist: &str) -> Result<Vec<LyricsSearchResult>, String> {
        const RATE_LIMITED: &str = "LRCLIB is limiting requests. Please try again shortly.";

        let started = Instant::now();
        let deadline = started + Duration::from_secs(100);
        let url = self.build_search_url(title, artist);
        let mut attempt = 0usize;

        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err("The lyrics search timed out.".to_string());
            }
            if let Some(backoff) = self.backoff_until {
                if now < backoff {
                    return Err(RATE_LIMITED.to_string());
                }
            }
            if let Err(delay) = self.can_request(now) {
                if now + delay >= deadline {
                    return Err("The lyrics search timed out.".to_string());
                }
                std::thread::sleep(delay);
            }
            self.record_request(Instant::now());

            let response = match crate::lyrics::http::get(
                &url,
                "MusicPlayer/1.0 (+https://github.com/kalabhaftu/music-player)",
                Duration::from_secs(15),
            ) {
                Ok(response) => response,
                Err(error) => {
                    if attempt == 0 {
                        attempt += 1;
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    return Err(format!("Network error: {error}"));
                }
            };

            match response.status {
                200 => {
                    return serde_json::from_slice::<Vec<LyricsSearchResult>>(&response.body)
                        .map_err(|error| format!("LRCLIB returned invalid lyrics data: {error}"));
                }
                429 => {
                    self.set_backoff(Instant::now(), Duration::from_secs(15));
                    return Err(RATE_LIMITED.to_string());
                }
                408 | 500 | 502 | 503 | 504 if attempt == 0 => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_secs(1));
                }
                status => {
                    return Err(format!("LRCLIB returned HTTP {status}."));
                }
            }
        }
    }

    /// Performs search with normalized title/artist first, falling back to raw inputs if needed.
    pub fn search(&mut self, title: &str, artist: &str) -> Result<Vec<LyricsSearchResult>, String> {
        let (norm_title, norm_artist) = normalize_query(title, artist);
        let q_title = if !norm_title.is_empty() { &norm_title } else { title };
        let results = self.search_raw(q_title, &norm_artist)?;
        if !results.is_empty() || (q_title == title && norm_artist == artist) {
            return Ok(results);
        }
        self.search_raw(title, artist)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_censored_lyrics_detection() {
        assert!(is_censored_lyrics("What the f*** is going on"));
        assert!(is_censored_lyrics("You little b****"));
        assert!(is_censored_lyrics("They are all **** here"));
        assert!(is_censored_lyrics("Look at that f---ing car"));
        assert!(!is_censored_lyrics("What the fuck is going on"));
        assert!(!is_censored_lyrics("You little bitch"));
        assert!(!is_censored_lyrics("Clean lyrics without any curses"));
    }
}
