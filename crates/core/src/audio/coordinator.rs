//! Port of `MusicPlayer.App.PlaybackCommandCoordinator`.
//!
//! The coordinator is pure queue-state logic: it computes the next queue state,
//! the command outcome, and (when crossfade is enabled) a crossfade request.
//! Performing the actual engine action (play, pause, seek, crossfade) is the
//! caller's job, because the engine lives behind the caller on the app thread.

use std::path::Path;

use crate::models::Track;

use super::queue::QueueCoordinator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommandKind {
    NoChange,
    TrackStarted,
    Paused,
    Resumed,
    PositionReset,
    CrossfadeRequested,
    StopPlayback,
    TrackUnavailable,
    AdvanceFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeSeverity {
    Informational,
    Warning,
    Error,
    Success,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub message: String,
    pub severity: NoticeSeverity,
}

impl Notice {
    pub fn new(message: impl Into<String>, severity: NoticeSeverity) -> Self {
        Self {
            message: message.into(),
            severity,
        }
    }

    pub fn informational(message: impl Into<String>) -> Self {
        Self::new(message, NoticeSeverity::Informational)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, NoticeSeverity::Warning)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackCrossfadeRequest {
    pub target_index: usize,
    pub source_index: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PlaybackCommandResult {
    pub kind: PlaybackCommandKind,
    pub track_path: Option<String>,
    pub queue_changed: bool,
    pub persist_session: bool,
    pub notice: Option<Notice>,
    pub crossfade: Option<PlaybackCrossfadeRequest>,
}

impl PlaybackCommandResult {
    pub fn no_change() -> Self {
        Self {
            kind: PlaybackCommandKind::NoChange,
            track_path: None,
            queue_changed: false,
            persist_session: false,
            notice: None,
            crossfade: None,
        }
    }
}

/// Port of `PlaybackCommandCoordinator`. Resolves tracks through a caller
/// supplied resolver; queue mutations happen immediately, engine actions do
/// not happen here.
pub struct PlaybackCoordinator {
    queue: QueueCoordinator,
    crossfade_ms: u64,
    /// The path of the track the engine currently has loaded. The pure
    /// coordinator cannot ask the engine, so callers keep this in sync (the
    /// coordinator updates it itself for every started track).
    current_track_path: Option<String>,
}

impl PlaybackCoordinator {
    pub fn new(queue: QueueCoordinator) -> Self {
        Self {
            queue,
            crossfade_ms: 0,
            current_track_path: None,
        }
    }

    pub fn with_crossfade_ms(queue: QueueCoordinator, crossfade_ms: u64) -> Self {
        Self {
            queue,
            crossfade_ms,
            current_track_path: None,
        }
    }

    pub fn queue(&self) -> &QueueCoordinator {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut QueueCoordinator {
        &mut self.queue
    }

    pub fn crossfade_ms(&self) -> u64 {
        self.crossfade_ms
    }

    pub fn set_crossfade_ms(&mut self, crossfade_ms: u64) {
        self.crossfade_ms = crossfade_ms;
    }

    pub fn current_track_path(&self) -> Option<&str> {
        self.current_track_path.as_deref()
    }

    pub fn set_current_track_path(&mut self, path: Option<String>) {
        self.current_track_path = path;
    }

    /// Port of `StartTrack`. The resolver is kept in the signature for
    /// parity with the C# coordinator (existence is checked directly).
    #[allow(unused_variables)]
    pub fn start_track(
        &mut self,
        track: &Track,
        reset_queue: bool,
        queue_index: Option<usize>,
        view_paths: &[String],
        resolver: &dyn Fn(&str) -> Option<Track>,
    ) -> PlaybackCommandResult {
        if !Path::new(&track.path).exists() {
            return PlaybackCommandResult {
                kind: PlaybackCommandKind::TrackUnavailable,
                track_path: Some(track.path.clone()),
                notice: Some(Notice::warning(format!(
                    "File not found at its saved location: {}",
                    track.path
                ))),
                ..PlaybackCommandResult::no_change()
            };
        }

        if reset_queue {
            self.queue.replace(view_paths.to_vec(), None);
            let selected_index = match queue_index {
                Some(requested)
                    if requested < self.queue.entries().len()
                        && self.queue.entries()[requested].eq_ignore_ascii_case(&track.path) =>
                {
                    Some(requested)
                }
                _ => self.queue.find_index(Some(&track.path)),
            };
            self.queue.set_current_index(selected_index);
            if self.queue.shuffle {
                let start = (self.queue.current_index().map_or(0, |idx| idx + 1))
                    .min(self.queue.entries().len());
                self.queue.shuffle_upcoming(start);
            }
        } else if let Some(requested_index) =
            queue_index.filter(|requested| *requested < self.queue.entries().len())
        {
            self.queue.set_current_index(Some(requested_index));
        } else if let Some(matching_index) = self.queue.find_index(Some(&track.path)) {
            self.queue.set_current_index(Some(matching_index));
        }

        // The caller performs playback.Play(track) against the engine.
        self.current_track_path = Some(track.path.clone());
        self.queue.clear_ab_repeat();
        PlaybackCommandResult {
            kind: PlaybackCommandKind::TrackStarted,
            track_path: Some(track.path.clone()),
            queue_changed: true,
            persist_session: true,
            ..PlaybackCommandResult::no_change()
        }
    }

    /// Port of `Toggle`. `is_playing` and `has_ended` come from the engine; the
    /// caller performs the resulting pause, restart, or resume action.
    #[allow(clippy::too_many_arguments)]
    pub fn toggle(
        &mut self,
        current: Option<&Track>,
        is_playing: bool,
        has_ended: bool,
        position_ms: u64,
        duration_ms: u64,
        resolver: &dyn Fn(&str) -> Option<Track>,
    ) -> PlaybackCommandResult {
        let Some(current) = current else {
            return PlaybackCommandResult::no_change();
        };
        if is_playing {
            return self.pause(Some(current));
        }
        if has_ended || (duration_ms > 0 && position_ms + 250 >= duration_ms) {
            let queue_index = if self.queue.current_index().is_some() {
                self.queue.current_index()
            } else {
                None
            };
            return self.start_track(current, false, queue_index, &[], resolver);
        }
        self.resume(Some(current))
    }

    /// Port of `Pause` (decision only; the caller pauses the engine).
    pub fn pause(&self, current: Option<&Track>) -> PlaybackCommandResult {
        PlaybackCommandResult {
            kind: PlaybackCommandKind::Paused,
            track_path: current.map(|track| track.path.clone()),
            ..PlaybackCommandResult::no_change()
        }
    }

    /// Port of `Resume` (decision only; the caller resumes the engine).
    pub fn resume(&self, current: Option<&Track>) -> PlaybackCommandResult {
        let Some(current) = current else {
            return PlaybackCommandResult::no_change();
        };
        if !Path::new(&current.path).exists() {
            return PlaybackCommandResult {
                kind: PlaybackCommandKind::TrackUnavailable,
                track_path: Some(current.path.clone()),
                notice: Some(Notice::warning(format!(
                    "File not found at its saved location: {}",
                    current.path
                ))),
                ..PlaybackCommandResult::no_change()
            };
        }
        PlaybackCommandResult {
            kind: PlaybackCommandKind::Resumed,
            track_path: Some(current.path.clone()),
            ..PlaybackCommandResult::no_change()
        }
    }

    /// Port of `Previous`.
    pub fn previous(
        &mut self,
        current: Option<&Track>,
        position_ms: u64,
        resolver: &dyn Fn(&str) -> Option<Track>,
    ) -> PlaybackCommandResult {
        let Some(current) = current else {
            return PlaybackCommandResult::no_change();
        };
        if position_ms > 3000 {
            // The caller seeks the engine back to zero.
            return PlaybackCommandResult {
                kind: PlaybackCommandKind::PositionReset,
                track_path: Some(current.path.clone()),
                ..PlaybackCommandResult::no_change()
            };
        }

        if let Some(start) = self.queue.current_index() {
            let mut index = start;
            while index > 0 {
                index -= 1;
                if let Some(previous) = self.resolve_queue_track(index, resolver) {
                    return self.start_track(&previous, false, Some(index), &[], resolver);
                }
            }
        }

        // Previous at the first playable queue entry restarts it instead of
        // doing nothing. The caller seeks the engine back to zero.
        PlaybackCommandResult {
            kind: PlaybackCommandKind::PositionReset,
            track_path: Some(current.path.clone()),
            ..PlaybackCommandResult::no_change()
        }
    }

    /// Port of `Advance(automatic, crossfadeMilliseconds, crossfadeBlocked,
    /// existingCrossfadeSourceIndex)`.
    ///
    /// `autoplay_append` supplies the fallback view paths that build a queue
    /// when the current queue is empty (autoplay continuation). Crossfade
    /// timing comes from the coordinator's `crossfade_ms`.
    pub fn advance(
        &mut self,
        automatic: bool,
        resolver: &dyn Fn(&str) -> Option<Track>,
        autoplay_append: Option<Vec<String>>,
    ) -> PlaybackCommandResult {
        self.advance_with_options(automatic, false, None, resolver, autoplay_append)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn advance_with_options(
        &mut self,
        automatic: bool,
        crossfade_blocked: bool,
        existing_crossfade_source_index: Option<usize>,
        resolver: &dyn Fn(&str) -> Option<Track>,
        autoplay_append: Option<Vec<String>>,
    ) -> PlaybackCommandResult {
        let was_empty = self.queue.entries().is_empty();
        if was_empty {
            let fallback_paths = autoplay_append.unwrap_or_default();
            self.queue
                .ensure_queue(&fallback_paths, self.current_track_path.clone().as_deref());
        }
        if was_empty && self.queue.shuffle {
            let start = (self.queue.current_index().map_or(0, |idx| idx + 1))
                .min(self.queue.entries().len());
            self.queue.shuffle_upcoming(start);
        }
        if self.queue.entries().is_empty() {
            return PlaybackCommandResult::no_change();
        }

        let mut changed_while_skipping = false;
        for _attempt in 0..=self.queue.entries().len() {
            let next = self.queue.next_index(automatic && !changed_while_skipping);
            let Some(next) = next else { break };

            match self.resolve_queue_track(next, resolver) {
                None => {
                    if Some(next) == self.queue.current_index() {
                        changed_while_skipping = true;
                        continue;
                    }
                    self.queue.set_current_index(Some(next));
                    changed_while_skipping = true;
                    continue;
                }
                Some(track) => {
                    if automatic && !crossfade_blocked && self.crossfade_ms > 0 {
                        return self.prepare_crossfade_result(
                            track,
                            next,
                            self.crossfade_ms,
                            existing_crossfade_source_index,
                        );
                    }
                    return self.start_track(&track, false, Some(next), &[], resolver);
                }
            }
        }

        PlaybackCommandResult {
            kind: PlaybackCommandKind::StopPlayback,
            queue_changed: changed_while_skipping,
            ..PlaybackCommandResult::no_change()
        }
    }

    /// Port of `PrepareCrossfade(int targetIndex, ...)`.
    pub fn prepare_crossfade(
        &mut self,
        target_index: usize,
        duration_ms: u64,
        existing_crossfade_source_index: Option<usize>,
        resolver: &dyn Fn(&str) -> Option<Track>,
    ) -> PlaybackCommandResult {
        match self.resolve_queue_track(target_index, resolver) {
            Some(track) => self.prepare_crossfade_result(
                track,
                target_index,
                duration_ms,
                existing_crossfade_source_index,
            ),
            None => PlaybackCommandResult {
                kind: PlaybackCommandKind::AdvanceFallback,
                ..PlaybackCommandResult::no_change()
            },
        }
    }

    /// Port of `PlayQueueEntry`.
    pub fn play_queue_entry(
        &mut self,
        index: usize,
        resolver: &dyn Fn(&str) -> Option<Track>,
    ) -> PlaybackCommandResult {
        match self.resolve_queue_track(index, resolver) {
            Some(track) => self.start_track(&track, false, Some(index), &[], resolver),
            None => PlaybackCommandResult {
                kind: PlaybackCommandKind::TrackUnavailable,
                notice: Some(Notice::warning(
                    "That queued file is no longer available.",
                )),
                ..PlaybackCommandResult::no_change()
            },
        }
    }

    fn prepare_crossfade_result(
        &mut self,
        track: Track,
        target_index: usize,
        duration_ms: u64,
        existing_crossfade_source_index: Option<usize>,
    ) -> PlaybackCommandResult {
        let source_index = existing_crossfade_source_index
            .or(self.queue.current_index())
            .unwrap_or(0);
        self.queue.set_current_index(Some(target_index));
        self.current_track_path = Some(track.path.clone());
        PlaybackCommandResult {
            kind: PlaybackCommandKind::CrossfadeRequested,
            track_path: Some(track.path.clone()),
            crossfade: Some(PlaybackCrossfadeRequest {
                target_index,
                source_index,
                duration_ms,
            }),
            ..PlaybackCommandResult::no_change()
        }
    }

    /// Port of `ResolveQueueTrack`: the queued file must still exist on disk
    /// and the resolver must know it.
    fn resolve_queue_track(
        &self,
        index: usize,
        resolver: &dyn Fn(&str) -> Option<Track>,
    ) -> Option<Track> {
        let path = self.queue.entries().get(index)?;
        if !Path::new(path).exists() {
            return None;
        }
        resolver(path)
    }
}
