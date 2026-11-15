use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    Queue,
    Track,
}

impl RepeatMode {
    pub fn cycle(&self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RepeatMode::Off => "Off",
            RepeatMode::Queue => "Queue",
            RepeatMode::Track => "Track",
        }
    }

    pub fn from_str_lenient(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "queue" | "all" => RepeatMode::Queue,
            "track" | "one" => RepeatMode::Track,
            _ => RepeatMode::Off,
        }
    }
}

pub fn next_index(
    count: usize,
    current_index: Option<usize>,
    automatic: bool,
    repeat_mode: RepeatMode,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    if automatic && repeat_mode == RepeatMode::Track {
        return Some(current_index.unwrap_or(0).min(count - 1));
    }
    let cur = match current_index {
        Some(idx) => idx,
        None => return Some(0),
    };
    if cur + 1 < count {
        Some(cur + 1)
    } else if repeat_mode == RepeatMode::Queue {
        Some(0)
    } else {
        None
    }
}

pub fn previous_index(count: usize, current_index: Option<usize>) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let cur = current_index.unwrap_or(0);
    if cur > 0 {
        Some(cur - 1)
    } else {
        Some(0)
    }
}

pub fn shuffle_upcoming<T>(queue: &mut [T], start_index: usize) {
    if start_index >= queue.len() {
        return;
    }
    let slice = &mut queue[start_index..];
    let len = slice.len();
    if len <= 1 {
        return;
    }

    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    for i in (1..len).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (seed >> 33) as usize % (i + 1);
        slice.swap(i, j);
    }
}

#[derive(Debug, Clone, Default)]
pub struct QueueCoordinator {
    entries: Vec<String>,
    current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub repeat_a_ms: Option<u64>,
    pub repeat_b_ms: Option<u64>,
    pub revision: u64,
}

impl QueueCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn set_current_index(&mut self, idx: Option<usize>) {
        self.current_index = idx;
    }

    pub fn current_track_path(&self) -> Option<&str> {
        self.current_index
            .and_then(|idx| self.entries.get(idx))
            .map(|s| s.as_str())
    }

    pub fn replace(&mut self, paths: Vec<String>, current_index: Option<usize>) {
        self.entries = paths;
        self.current_index = if self.entries.is_empty() {
            None
        } else {
            current_index.map(|idx| idx.min(self.entries.len() - 1))
        };
        self.revision += 1;
    }

    pub fn select_track(&mut self, context_paths: Vec<String>, track_path: &str) {
        let same_path = |a: &str, b: &str| a.eq_ignore_ascii_case(b);

        let found_pos = context_paths.iter().position(|p| same_path(p, track_path));
        self.entries = context_paths;

        if let Some(pos) = found_pos {
            self.current_index = Some(pos);
        } else {
            self.entries.insert(0, track_path.to_string());
            self.current_index = Some(0);
        }

        if self.shuffle {
            let start = self.current_index.map_or(0, |i| i + 1);
            shuffle_upcoming(&mut self.entries, start);
        }
        self.revision += 1;
    }

    pub fn next(&mut self, automatic: bool) -> Option<&str> {
        let next_idx = next_index(self.entries.len(), self.current_index, automatic, self.repeat_mode);
        self.current_index = next_idx;
        self.current_track_path()
    }

    pub fn previous(&mut self) -> Option<&str> {
        let prev_idx = previous_index(self.entries.len(), self.current_index);
        self.current_index = prev_idx;
        self.current_track_path()
    }

    pub fn insert_next(&mut self, path: String) {
        let insert_at = match self.current_index {
            Some(idx) => (idx + 1).min(self.entries.len()),
            None => 0,
        };
        self.entries.insert(insert_at, path);
        self.revision += 1;
    }

    pub fn append(&mut self, path: String) {
        self.entries.push(path);
        self.revision += 1;
    }

    pub fn remove_entry(&mut self, index: usize) -> bool {
        if index >= self.entries.len() || Some(index) == self.current_index {
            return false;
        }
        self.entries.remove(index);
        if let Some(ref mut cur) = self.current_index {
            if index < *cur {
                *cur -= 1;
            }
        }
        self.revision += 1;
        true
    }

    pub fn clear_upcoming(&mut self) {
        let current = self.current_track_path().map(|s| s.to_string());
        self.entries.clear();
        if let Some(cur) = current {
            self.entries.push(cur);
            self.current_index = Some(0);
        } else {
            self.current_index = None;
        }
        self.revision += 1;
    }

    pub fn set_ab_repeat_points(&mut self, a_ms: Option<u64>, b_ms: Option<u64>) {
        if let (Some(a), Some(b)) = (a_ms, b_ms) {
            if b > a {
                self.repeat_a_ms = Some(a);
                self.repeat_b_ms = Some(b);
                return;
            }
        }
        self.repeat_a_ms = a_ms;
        self.repeat_b_ms = b_ms;
    }

    pub fn clear_ab_repeat(&mut self) {
        self.repeat_a_ms = None;
        self.repeat_b_ms = None;
    }
}
