//! Ports of `MusicPlayer.Core` playback telemetry helpers: `PlayCompletion`,
//! `PlaybackListeningTracker`, and the stale-event policy from
//! `PlaybackEventPolicy` (kept beside its consumers here).

/// Port of `MusicPlayer.Core.PlayCompletion`.
pub struct PlayCompletion;

impl PlayCompletion {
    /// The existing half-duration play-count threshold: true once the credited
    /// position reaches at least half of a positive duration.
    pub fn has_reached_half(duration_ms: i64, position_ms: i64) -> bool {
        duration_ms > 0 && position_ms * 2 >= duration_ms
    }
}

/// Port of `MusicPlayer.Core.PlaybackListeningTracker`.
///
/// Tracks credited listening time for the current playback item without
/// depending on a UI framework, timer, playback engine, or persistence layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaybackListeningTracker {
    counted_current_play: bool,
    heard_milliseconds: i64,
    last_position_milliseconds: i64,
}

impl PlaybackListeningTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts tracking a newly selected track.
    pub fn reset_for_track(&mut self) {
        self.counted_current_play = false;
        self.heard_milliseconds = 0;
        self.last_position_milliseconds = 0;
    }

    /// Rebases position after resuming so paused time is not counted.
    pub fn mark_resumed(&mut self, position_milliseconds: i64) {
        self.last_position_milliseconds = position_milliseconds.max(0);
    }

    /// Starts a restored session paused at its saved position.
    pub fn restore_paused(&mut self, position_milliseconds: i64) {
        self.counted_current_play = false;
        self.heard_milliseconds = 0;
        self.last_position_milliseconds = position_milliseconds.max(0);
    }

    /// Updates credited listening time and returns true exactly once when this
    /// track reaches the half-duration play-count threshold.
    ///
    /// Only forward deltas of at most 3000 ms are credited, so seeks, restarts,
    /// and clock corrections do not inflate the credited time.
    pub fn observe(
        &mut self,
        has_track: bool,
        is_playing: bool,
        position_milliseconds: i64,
        duration_milliseconds: i64,
    ) -> bool {
        let position = position_milliseconds.max(0);
        let duration = duration_milliseconds.max(0);

        if has_track {
            if is_playing {
                let heard_now = position - self.last_position_milliseconds;
                if heard_now > 0 && heard_now <= 3000 {
                    self.heard_milliseconds += heard_now;
                }
            }

            self.last_position_milliseconds = position;
        }

        if self.counted_current_play
            || !has_track
            || !PlayCompletion::has_reached_half(duration, self.heard_milliseconds)
        {
            return false;
        }

        self.counted_current_play = true;
        true
    }

    pub fn heard_milliseconds(&self) -> i64 {
        self.heard_milliseconds
    }

    pub fn counted_current_play(&self) -> bool {
        self.counted_current_play
    }
}

/// Port of `MusicPlayer.Core.PlaybackEventPolicy`: rejects stale LibVLC
/// callbacks that arrive while a reused player is changing media.
pub struct PlaybackEventPolicy;

impl PlaybackEventPolicy {
    pub fn should_handle_end_reached(
        sender_is_active_player: bool,
        has_current_track: bool,
        current_state_is_ended: bool,
        crossfade_pending: bool,
    ) -> bool {
        sender_is_active_player
            && has_current_track
            && current_state_is_ended
            && !crossfade_pending
    }

    pub fn should_handle_active_error(
        sender_is_active_player: bool,
        has_current_track: bool,
        current_state_is_error: bool,
    ) -> bool {
        sender_is_active_player && has_current_track && current_state_is_error
    }

    pub fn should_handle_crossfade_error(
        sender_is_crossfade_player: bool,
        has_crossfade_target: bool,
        current_state_is_error: bool,
    ) -> bool {
        sender_is_crossfade_player && has_crossfade_target && current_state_is_error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_threshold_matches_csharp_ticks_math() {
        assert!(!PlayCompletion::has_reached_half(0, 90_000));
        assert!(!PlayCompletion::has_reached_half(-5, 90_000));
        assert!(!PlayCompletion::has_reached_half(3, 1));
        assert!(PlayCompletion::has_reached_half(3, 2));
        assert!(PlayCompletion::has_reached_half(180_000, 90_000));
        assert!(!PlayCompletion::has_reached_half(180_000, 89_999));
    }
}
