use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricsLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LyricsDocument {
    pub lines: Vec<LyricsLine>,
    pub offset_ms: i64,
    pub metadata: HashMap<String, String>,
}

impl LyricsDocument {
    pub fn new(lines: Vec<LyricsLine>, offset_ms: i64) -> Self {
        Self {
            lines,
            offset_ms,
            metadata: HashMap::new(),
        }
    }

    pub fn at(&self, position_ms: u64) -> &str {
        if self.lines.is_empty() {
            return "";
        }
        let adjusted_pos = if self.offset_ms < 0 {
            position_ms.saturating_add((-self.offset_ms) as u64)
        } else {
            position_ms.saturating_sub(self.offset_ms as u64)
        };

        for line in self.lines.iter().rev() {
            if line.time_ms <= adjusted_pos {
                return &line.text;
            }
        }
        ""
    }

    pub fn active_index(&self, position_ms: u64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let adjusted_pos = if self.offset_ms < 0 {
            position_ms.saturating_add((-self.offset_ms) as u64)
        } else {
            position_ms.saturating_sub(self.offset_ms as u64)
        };

        for (idx, line) in self.lines.iter().enumerate().rev() {
            if line.time_ms <= adjusted_pos {
                return Some(idx);
            }
        }
        None
    }
}
