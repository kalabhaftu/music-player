use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use regex::Regex;
use serde::{Deserialize, Serialize};

fn deserialize_null_as_default_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<f64>::deserialize(deserializer)?;
    Ok(opt.unwrap_or(0.0))
}

fn deserialize_null_as_default_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<bool>::deserialize(deserializer)?;
    Ok(opt.unwrap_or(false))
}

fn deserialize_null_as_default_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSearchResult {
    pub id: u64,
    #[serde(default, deserialize_with = "deserialize_null_as_default_string")]
    pub track_name: String,
    #[serde(default, deserialize_with = "deserialize_null_as_default_string")]
    pub artist_name: String,
    pub album_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_as_default_f64")]
    pub duration: f64,
    #[serde(default, deserialize_with = "deserialize_null_as_default_bool")]
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
    let final_title = normalized
        .trim_matches(|c| c == ' ' || c == '-' || c == '–' || c == '—' || c == '|' || c == '+')
        .to_string();

    (final_title, clean_artist)
}

/// Strips parenthetical / trailing featured-artist & release edition notes for
/// secondary LRCLIB fallback searches and matching when exact title omits them.
pub fn strip_featured_and_edition(title: &str) -> String {
    let bracketed_re = Regex::new(r"(?i)\([^)]*\)|\[[^\]]*\]|\{[^}]*\}").unwrap();
    let extra_re = Regex::new(r"(?i)\b(?:feat(?:uring)?|ft|with|prod(?:uced)?\s+by|deluxe|explicit|clean|remaster(?:ed)?|bonus\s+track|version|edit|radio|live)\b").unwrap();
    let stripped = bracketed_re.replace_all(title, |caps: &regex::Captures| {
        if extra_re.is_match(&caps[0]) {
            " ".to_string()
        } else {
            caps[0].to_string()
        }
    });
    let trail_re = Regex::new(r"(?i)\s+(?:feat\.?|ft\.?|featuring)\s+.*$").unwrap();
    let without_trail = trail_re.replace(&stripped, "");
    let ws_re = Regex::new(r"\s+").unwrap();
    ws_re
        .replace_all(&without_trail, " ")
        .trim_matches(|c| c == ' ' || c == '-' || c == '–' || c == '—')
        .to_string()
}

/// Extracts the primary artist when metadata lists multiple artists (`A; B`, `A / B`, `A, B`, `A feat. B`).
pub fn primary_artist_name(artist: &str) -> String {
    let trimmed = artist.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let tag_sep_re = Regex::new(r"(?i)\s*(?:;|/|,\s+|\bfeat\.?\b|\bft\.?\b|\bfeaturing\b)\s*").unwrap();
    if let Some(m) = tag_sep_re.find(trimmed) {
        let head = trimmed[..m.start()].trim();
        if !head.is_empty() {
            return head.to_string();
        }
    }
    trimmed.to_string()
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

fn fold_latin_char(ch: char) -> char {
    match ch {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ž' | 'ź' | 'ż' => 'z',
        'š' | 'ś' => 's',
        'ř' => 'r',
        'ł' => 'l',
        _ => ch,
    }
}

pub fn normalized_words(value: &str) -> Vec<String> {
    let lower = value
        .to_lowercase()
        .replace(['\u{2018}', '\u{2019}', '\u{02BC}', '\''], "")
        .replace('&', " and ");
    let mut words = Vec::new();
    let mut current = String::new();
    for raw_ch in lower.chars() {
        let ch = fold_latin_char(raw_ch);
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
    let stripped_title_words: HashSet<String> =
        normalized_words(&strip_featured_and_edition(&identity_title))
            .into_iter()
            .collect();
    let candidate_words: HashSet<String> = normalized_words(&other_title).into_iter().collect();
    let stripped_candidate_words: HashSet<String> =
        normalized_words(&strip_featured_and_edition(&other_title))
            .into_iter()
            .collect();

    if title_words.is_empty() || candidate_words.is_empty() {
        return -1.0;
    }

    let word_overlap = |a: &HashSet<String>, b: &HashSet<String>| -> (f64, f64) {
        if a.is_empty() || b.is_empty() {
            return (0.0, 0.0);
        }
        let common = a.intersection(b).count() as f64;
        (common / a.len() as f64, common / b.len() as f64)
    };
    let (exact_cov, exact_prec) = word_overlap(&title_words, &candidate_words);
    let (clean_cov, clean_prec) = word_overlap(&stripped_title_words, &stripped_candidate_words);
    let (coverage, precision) = if exact_cov >= 0.9 && exact_prec >= 0.8 {
        (exact_cov, exact_prec)
    } else if clean_cov >= 0.9 && clean_prec >= 0.8 {
        (clean_cov, clean_prec)
    } else {
        return -1.0;
    };

    let artist_words: HashSet<String> = normalized_words(&identity_artist).into_iter().collect();
    let primary_words: HashSet<String> = normalized_words(&primary_artist_name(&identity_artist))
        .into_iter()
        .collect();
    let other_artist_words: HashSet<String> =
        normalized_words(&other_artist).into_iter().collect();

    let artist_common = artist_words.intersection(&other_artist_words).count() as f64;
    let primary_common = primary_words.intersection(&other_artist_words).count() as f64;
    if !artist_words.is_empty() {
        if other_artist_words.is_empty() {
            return -1.0;
        }
        let artist_cov = artist_common / artist_words.len() as f64;
        let artist_prec = artist_common / other_artist_words.len() as f64;
        let primary_ok = !primary_words.is_empty()
            && (primary_common / primary_words.len() as f64) >= 0.85;
        if !primary_ok && (artist_cov < 0.45 || (artist_cov < 0.9 && artist_prec < 0.8)) {
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
        (artist_common / artist_words.len() as f64)
            .max(if primary_words.is_empty() { 0.0 } else { primary_common / primary_words.len() as f64 })
            * 0.22
    } else {
        0.22
    };

    (coverage * 0.78 + precision * 0.22) * 0.78 + artist_score
}

/// Detects if lyrics contain censored profanity (e.g. `f***`, `b****`, `n****`, `s***`, `***`).
pub fn is_censored_lyrics(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("[censored]") || lower.contains("[bleep]") {
        return true;
    }
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
                || clean.contains("f**")
                || clean.contains("f--")
                || clean.contains("b**")
                || clean.contains("b--")
                || clean.contains("n**")
                || clean.contains("sh*")
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

struct GlobalRateState {
    next_request: Option<Instant>,
    backoff_until: Option<Instant>,
}

fn global_rate_state() -> &'static Mutex<GlobalRateState> {
    static STATE: OnceLock<Mutex<GlobalRateState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(GlobalRateState {
            next_request: None,
            backoff_until: None,
        })
    })
}

/// Client for searching timed and plain lyrics from lrclib.net with shared
/// process-wide rate limiting (matching the singleton `LrclibSearchClient`).
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

    /// Checks if a request is currently allowed under local rate limits and backoff.
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
        if artist.trim().is_empty() {
            format!("https://lrclib.net/api/search?q={}", urlencoding(title.trim()))
        } else {
            format!(
                "https://lrclib.net/api/search?track_name={}&artist_name={}",
                urlencoding(title.trim()),
                urlencoding(artist.trim())
            )
        }
    }

    fn execute_url(&mut self, url: &str) -> Result<Vec<LyricsSearchResult>, String> {
        const RATE_LIMITED: &str = "LRCLIB is limiting requests. Please try again shortly.";

        let started = Instant::now();
        let deadline = started + Duration::from_secs(30);
        let mut attempt = 0usize;

        loop {
            let wait_for = {
                let mut guard = global_rate_state().lock().unwrap_or_else(|e| e.into_inner());
                let now = Instant::now();
                if now >= deadline {
                    return Err("The lyrics search timed out.".to_string());
                }
                if let Some(backoff) = guard.backoff_until.or(self.backoff_until) {
                    if now < backoff {
                        return Err(RATE_LIMITED.to_string());
                    }
                }
                let scheduled = guard
                    .next_request
                    .or(self.next_request)
                    .filter(|&t| t > now)
                    .unwrap_or(now);
                if scheduled >= deadline {
                    return Err("The lyrics search timed out.".to_string());
                }
                guard.next_request = Some(scheduled + self.min_interval);
                self.next_request = guard.next_request;
                scheduled.saturating_duration_since(now)
            };

            if !wait_for.is_zero() {
                std::thread::sleep(wait_for);
            }

            let response = match crate::lyrics::http::get(
                url,
                "MusicPlayer/1.0 (+https://github.com/kalabhaftu/music-player)",
                Duration::from_secs(12),
            ) {
                Ok(response) => response,
                Err(error) => {
                    if attempt == 0 {
                        attempt += 1;
                        std::thread::sleep(Duration::from_millis(500));
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
                    let until = Instant::now() + Duration::from_secs(15);
                    if let Ok(mut guard) = global_rate_state().lock() {
                        guard.backoff_until = Some(until);
                    }
                    self.backoff_until = Some(until);
                    return Err(RATE_LIMITED.to_string());
                }
                408 | 500 | 502 | 503 | 504 if attempt == 0 => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(700));
                }
                status => {
                    return Err(format!("LRCLIB returned HTTP {status}."));
                }
            }
        }
    }

    /// Executes raw LRCLIB query.
    pub fn search_raw(&mut self, title: &str, artist: &str) -> Result<Vec<LyricsSearchResult>, String> {
        let url = self.build_search_url(title, artist);
        self.execute_url(&url)
    }

    /// Performs search with normalized title/artist first, then primary artist,
    /// and finally general `?q=` query if structured search returned no results.
    pub fn search(&mut self, title: &str, artist: &str) -> Result<Vec<LyricsSearchResult>, String> {
        let (norm_title, norm_artist) = normalize_query(title, artist);
        let q_title = if !norm_title.is_empty() {
            norm_title.as_str()
        } else {
            title.trim()
        };
        if q_title.is_empty() {
            return Ok(Vec::new());
        }

        let results = self.search_raw(q_title, &norm_artist)?;
        if !results.is_empty() {
            return Ok(results);
        }

        let primary = primary_artist_name(&norm_artist);
        let stripped_title = strip_featured_and_edition(q_title);
        if !stripped_title.is_empty() && stripped_title != q_title {
            let search_artist = if !primary.is_empty() { &primary } else { &norm_artist };
            let results = self.search_raw(&stripped_title, search_artist)?;
            if !results.is_empty() {
                return Ok(results);
            }
        }

        if !primary.is_empty() && primary != norm_artist {
            let results = self.search_raw(q_title, &primary)?;
            if !results.is_empty() {
                return Ok(results);
            }
        }

        let fallback_title = if !stripped_title.is_empty() {
            stripped_title
        } else {
            q_title.to_string()
        };
        let fallback_artist = if !primary.is_empty() {
            primary
        } else {
            norm_artist.clone()
        };
        let combined = if fallback_artist.is_empty() {
            fallback_title
        } else {
            format!("{} {}", fallback_title, fallback_artist)
        };
        self.search_raw(&combined, "")
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

    #[test]
    fn test_normalize_and_primary_artist_and_match() {
        assert_eq!(
            strip_featured_and_edition(&normalize_query("3500 (feat. Future & 2 Chainz) [Official Audio]", "Travis Scott; Future").0),
            "3500".to_string()
        );
        assert_eq!(primary_artist_name("50 Cent; Sonny Digital"), "50 Cent");
        assert_eq!(primary_artist_name("GloRilla/Sexyy Red"), "GloRilla");
        assert_eq!(normalized_words("I\u{2019}m The Man (Caf\u{00e9})"), vec!["im", "the", "man", "cafe"]);

        let candidate = LyricsSearchResult {
            id: 1,
            track_name: "I\u{2019}m the Man".to_string(),
            artist_name: "50 Cent".to_string(),
            album_name: None,
            duration: 210.0,
            instrumental: false,
            plain_lyrics: Some("Some words".to_string()),
            synced_lyrics: Some("[00:10.00] Some words".to_string()),
        };
        let score = lyric_match_score("I'm The Man (Explicit)", "50 Cent; Sonny Digital", 210.0, &candidate);
        assert!(score > 0.8, "Expected strong match score, got {}", score);
    }
}
