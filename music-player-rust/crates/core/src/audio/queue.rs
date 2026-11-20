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

/// Builds the queue state for externally opened files, mirroring
/// `QueueNavigation.BuildExternalOpenQueue`.
///
/// The opened paths come first (deduplicated, case-insensitive, blanks dropped),
/// then the currently playing track if it is not among them, then every
/// still-unplayed existing entry that was not opened.
pub fn build_external_open_queue(
    opened_paths: &[String],
    current_track_path: Option<&str>,
    existing_queue: &[String],
    current_index: i64,
) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut newly_opened: Vec<String> = Vec::new();
    for path in opened_paths {
        if path.trim().is_empty() {
            continue;
        }
        if newly_opened.iter().any(|seen| seen.eq_ignore_ascii_case(path)) {
            continue;
        }
        newly_opened.push(path.clone());
        result.push(path.clone());
    }

    if let Some(current) = current_track_path {
        if !current.trim().is_empty()
            && !result.iter().any(|seen| seen.eq_ignore_ascii_case(current))
        {
            result.push(current.to_string());
        }
    }

    let first_upcoming = (current_index + 1).clamp(0, existing_queue.len() as i64) as usize;
    for entry in existing_queue.iter().skip(first_upcoming) {
        if !newly_opened.iter().any(|seen| seen.eq_ignore_ascii_case(entry)) {
            result.push(entry.clone());
        }
    }

    result
}

fn same_path(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[derive(Debug, Clone, Default)]
pub struct QueueCoordinator {
    entries: Vec<String>,
    current_index: Option<usize>,
    /// Set when the entry at `current_index` was removed from the queue. While
    /// it is set, automatic advance behaves like a manual advance so a
    /// track-repeat wrap cannot restart an entry that no longer exists.
    /// Mirrors `PlaybackQueueCoordinator._currentEntryRemoved`.
    current_entry_removed: bool,
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
        // Mirrors SetCurrentIndex: an out-of-range index becomes "no current"
        // and every explicit selection clears the removed-entry flag.
        self.current_index = match idx {
            Some(i) if i < self.entries.len() => Some(i),
            _ => None,
        };
        self.current_entry_removed = false;
    }

    pub fn current_entry_removed(&self) -> bool {
        self.current_entry_removed
    }

    pub fn content_revision(&self) -> u64 {
        self.revision
    }

    pub fn set_shuffle(&mut self, enabled: bool) {
        self.shuffle = enabled;
    }

    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.repeat_mode = mode;
    }

    pub fn repeat_a_ms(&self) -> Option<u64> {
        self.repeat_a_ms
    }

    pub fn repeat_b_ms(&self) -> Option<u64> {
        self.repeat_b_ms
    }

    pub fn cycle_repeat_mode(&mut self) {
        self.repeat_mode = self.repeat_mode.cycle();
    }

    /// Finds the first entry matching `path` case-insensitively; `None`
    /// mirrors the C# `-1` sentinel for blank or unknown paths.
    pub fn find_index(&self, path: Option<&str>) -> Option<usize> {
        let path = path?;
        if path.trim().is_empty() {
            return None;
        }
        self.entries.iter().position(|entry| same_path(entry, path))
    }

    pub fn current_track_path(&self) -> Option<&str> {
        self.current_index
            .and_then(|idx| self.entries.get(idx))
            .map(|s| s.as_str())
    }

    pub fn replace(&mut self, paths: Vec<String>, current_index: Option<usize>) {
        self.entries = paths;
        self.current_entry_removed = false;
        self.revision += 1;
        self.current_index = if self.entries.is_empty() {
            None
        } else {
            current_index.filter(|idx| *idx < self.entries.len())
        };
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
        self.current_entry_removed = false;
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

    /// Mirrors `InsertNext`: ensures the current track is present, inserts the
    /// path immediately after the current entry, and re-shuffles the upcoming
    /// tail when shuffle is active so the inserted item stays next.
    pub fn insert_next(&mut self, path: String, current_track_path: Option<&str>) {
        if self.entries.is_empty() {
            if let Some(current) = current_track_path {
                if !current.trim().is_empty() {
                    self.entries.push(current.to_string());
                    self.current_index = Some(0);
                }
            }
        }
        let insert_at = self
            .current_index
            .map_or(0, |idx| idx + 1)
            .min(self.entries.len());
        self.entries.insert(insert_at, path);
        self.revision += 1;
        if let Some(current) = self.current_index {
            if insert_at <= current {
                self.current_index = Some(current + 1);
            }
        }
        if self.shuffle {
            self.shuffle_upcoming(insert_at + 1);
        }
    }

    /// Mirrors single-item `Append`: ensures the current track is present, then
    /// appends one path to the end of the queue.
    pub fn append_one(&mut self, path: String, current_track_path: Option<&str>) {
        self.ensure_current_track(current_track_path);
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

    /// Mirrors `PlaybackQueueCoordinator.NextIndex`: the removed-entry flag
    /// downgrades an automatic advance to a manual one.
    pub fn next_index(&self, automatic: bool) -> Option<usize> {
        next_index(
            self.entries.len(),
            self.current_index,
            automatic && !self.current_entry_removed,
            self.repeat_mode,
        )
    }

    /// Mirrors `PlaybackQueueCoordinator.ShuffleUpcoming`.
    pub fn shuffle_upcoming(&mut self, start_index: usize) {
        shuffle_upcoming(&mut self.entries, start_index);
        let clamped = start_index.min(self.entries.len());
        if self.entries.len() - clamped > 1 {
            self.revision += 1;
        }
    }

    /// Mirrors `MoveEntry`: `direction` may be negative (up) or positive (down).
    pub fn move_entry(&mut self, index: usize, direction: isize) -> bool {
        match (index as i64).checked_add(direction as i64) {
            Some(target) if target >= 0 => self.move_entry_to(index, target as usize),
            _ => false,
        }
    }

    /// Mirrors `MoveEntryTo`: only upcoming entries (after the current one)
    /// may be reordered.
    pub fn move_entry_to(&mut self, index: usize, target_index: usize) -> bool {
        let first_upcoming_index = self.current_index.map_or(0, |idx| idx + 1);
        if index < first_upcoming_index
            || index >= self.entries.len()
            || target_index < first_upcoming_index
            || target_index >= self.entries.len()
            || target_index == index
        {
            return false;
        }
        let entry = self.entries.remove(index);
        self.entries.insert(target_index, entry);
        self.revision += 1;
        true
    }

    /// Mirrors `RemovePaths`, including the manual-advance rule for the
    /// currently playing entry.
    pub fn remove_paths(&mut self, paths: &[String]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let removed = |candidate: &str| {
            paths
                .iter()
                .any(|path| same_path(path, candidate))
        };
        let old_index = self.current_index;
        if let Some(old) = old_index {
            if old < self.entries.len() && removed(&self.entries[old]) {
                self.current_entry_removed = true;
            }
        }
        let retained_before_current = self
            .entries
            .iter()
            .take(old_index.map_or(0, |old| (old + 1).min(self.entries.len())))
            .filter(|entry| !removed(entry))
            .count();
        let before = self.entries.len();
        self.entries.retain(|entry| !removed(entry));
        if self.entries.len() == before {
            return false;
        }
        let new_index = retained_before_current as i64 - 1;
        self.current_index = if new_index >= 0 && (new_index as usize) < self.entries.len() {
            Some(new_index as usize)
        } else {
            None
        };
        self.revision += 1;
        true
    }

    /// Mirrors `EnsureCurrentTrack`.
    pub fn ensure_current_track(&mut self, current_track_path: Option<&str>) {
        let Some(current) = current_track_path else {
            return;
        };
        if current.trim().is_empty() {
            return;
        }
        if self.entries.is_empty() {
            self.entries.push(current.to_string());
            self.revision += 1;
            self.current_index = Some(0);
            return;
        }
        if self
            .current_index
            .and_then(|idx| self.entries.get(idx))
            .map(|entry| same_path(entry, current))
            .unwrap_or(false)
        {
            return;
        }
        if let Some(existing) = self.find_index(Some(current)) {
            self.current_index = Some(existing);
            return;
        }
        let insert_at = self.current_index.map_or(0, |idx| idx + 1).min(self.entries.len());
        self.entries.insert(insert_at, current.to_string());
        self.revision += 1;
        self.current_index = Some(insert_at);
    }

    /// Mirrors `Append`.
    pub fn append(&mut self, paths: &[String], current_track_path: Option<&str>) {
        self.ensure_current_track(current_track_path);
        let previous_count = self.entries.len();
        self.entries.extend(paths.iter().cloned());
        if self.entries.len() != previous_count {
            self.revision += 1;
        }
    }

    /// Mirrors `InsertNextRange`.
    pub fn insert_next_range(&mut self, paths: &[String], current_track_path: Option<&str>) {
        if paths.is_empty() {
            return;
        }
        self.ensure_current_track(current_track_path);
        let insert_at = self
            .current_index
            .map_or(0, |idx| idx + 1)
            .min(self.entries.len());
        let position = insert_at;
        for (offset, path) in paths.iter().enumerate() {
            self.entries.insert(position + offset, path.clone());
        }
        self.revision += 1;
    }

    /// Mirrors `EnsureQueue`: fills an empty queue from the current view.
    pub fn ensure_queue(&mut self, view_paths: &[String], current_track_path: Option<&str>) {
        if !self.entries.is_empty() {
            return;
        }
        self.replace(view_paths.to_vec(), None);
        let Some(current) = current_track_path else {
            return;
        };
        if current.trim().is_empty() {
            return;
        }
        self.current_index = self.find_index(Some(current));
        if self.current_index.is_none() {
            self.entries.insert(0, current.to_string());
            self.current_index = Some(0);
        }
    }

    /// Mirrors `ToggleShuffle`.
    pub fn toggle_shuffle(&mut self, view_paths: &[String], current_track_path: Option<&str>) {
        self.shuffle = !self.shuffle;
        if self.entries.is_empty() {
            self.replace(view_paths.to_vec(), None);
            if let Some(current) = current_track_path {
                if !current.trim().is_empty() {
                    self.current_index = self.find_index(Some(current));
                    if self.current_index.is_none() {
                        self.entries.insert(0, current.to_string());
                        self.current_index = Some(0);
                    }
                }
            }
        }
        if self.shuffle {
            let start = self.current_index.map_or(0, |idx| idx + 1).min(self.entries.len());
            self.shuffle_upcoming(start);
        }
    }
}
