use music_player_core::{
    parse_lrc, plain_text, EqualizerState, LibraryScanCoordinator, LibraryStore,
    QueueCoordinator, RepeatMode, RgbColor, Track, TrackAvailabilityService, TrackSort,
};
use chrono::Utc;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn sample_track(path: &str, title: &str, artist: &str, album: &str) -> Track {
    Track {
        path: path.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        album_artist: artist.to_string(),
        genre: "Rock".to_string(),
        year: 2024,
        track_number: 1,
        duration_ms: 180_000,
        file_size: 5_000_000,
        modified_utc: Utc::now(),
        added_utc: Utc::now(),
        favorite: false,
        rating: 0,
        play_count: 0,
        last_played_utc: None,
        artwork_path: None,
        has_lyrics: false,
        unavailable: false,
    }
}

#[test]
fn test_database_crud_and_fts5_search() {
    let store = LibraryStore::in_memory().expect("in-memory db failed");

    let t1 = sample_track("C:/Music/song1.mp3", "Bohemian Rhapsody", "Queen", "A Night at the Opera");
    let t2 = sample_track("C:/Music/song2.mp3", "Radio Ga Ga", "Queen", "The Works");
    let t3 = sample_track("C:/Music/song3.mp3", "Hotel California", "Eagles", "Hotel California");

    store.upsert_track(&t1).unwrap();
    store.upsert_track(&t2).unwrap();
    store.upsert_track(&t3).unwrap();

    // Verify all tracks loaded
    let all = store.get_all_tracks(TrackSort::Title, false).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].title, "Bohemian Rhapsody");

    // Toggle favorite
    store.set_favorite("C:/Music/song1.mp3", true).unwrap();
    let favs = store.get_favorites().unwrap();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].title, "Bohemian Rhapsody");

    // Record play
    store.record_play("C:/Music/song1.mp3").unwrap();
    let updated_t1 = store.get_track("C:/Music/song1.mp3").unwrap().unwrap();
    assert_eq!(updated_t1.play_count, 1);
    assert!(updated_t1.last_played_utc.is_some());

    // Test FTS5 Search
    let search_queen = store.search_tracks("Queen", 10).unwrap();
    assert_eq!(search_queen.len(), 2);

    let search_california = store.search_tracks("California", 10).unwrap();
    assert_eq!(search_california.len(), 1);
    assert_eq!(search_california[0].title, "Hotel California");

    // Aggregations
    let albums = store.get_album_groups().unwrap();
    assert_eq!(albums.len(), 3);

    let artists = store.get_artist_groups().unwrap();
    assert_eq!(artists.len(), 2);
}

#[test]
fn test_queue_coordinator_navigation() {
    let mut queue = QueueCoordinator::new();
    let paths = vec![
        "songA.mp3".to_string(),
        "songB.mp3".to_string(),
        "songC.mp3".to_string(),
    ];

    queue.replace(paths, Some(0));
    assert_eq!(queue.current_index(), Some(0));
    assert_eq!(queue.current_track_path(), Some("songA.mp3"));

    // Next
    assert_eq!(queue.next(false), Some("songB.mp3"));
    assert_eq!(queue.current_index(), Some(1));

    // Next again
    assert_eq!(queue.next(false), Some("songC.mp3"));
    assert_eq!(queue.current_index(), Some(2));

    // End of queue with RepeatMode::Off
    assert_eq!(queue.next(false), None);

    // RepeatMode::Queue
    queue.repeat_mode = RepeatMode::Queue;
    assert_eq!(queue.next(false), Some("songA.mp3"));
    assert_eq!(queue.current_index(), Some(0));

    // Previous
    assert_eq!(queue.previous(), Some("songA.mp3")); // At index 0, clamp to 0
}

#[test]
fn test_lyrics_lrc_parsing() {
    let lrc = r#"
[ti:Sample Title]
[ar:Sample Artist]
[00:12.50]Line one of song
[00:18.25]Line two of song
[00:24.00]Line three of song
"#;

    let doc = parse_lrc(lrc);
    assert_eq!(doc.lines.len(), 3);
    assert_eq!(doc.lines[0].time_ms, 12500);
    assert_eq!(doc.lines[0].text, "Line one of song");
    assert_eq!(doc.lines[1].time_ms, 18250);

    // Line lookup by timestamp
    assert_eq!(doc.at(10000), "");
    assert_eq!(doc.at(15000), "Line one of song");
    assert_eq!(doc.at(20000), "Line two of song");
    assert_eq!(doc.at(30000), "Line three of song");

    // Plain text extraction
    let plain = plain_text(lrc);
    assert!(plain.contains("Line one of song"));
    assert!(!plain.contains("[00:12.50]"));
}

#[test]
fn test_equalizer_state() {
    let eq = EqualizerState::preset_rock();
    assert!(eq.enabled);
    assert_eq!(eq.band_gains_db.len(), 10);
    assert_eq!(eq.band_gains_db[0], 4.5); // Bass boost in Rock
}

#[test]
fn test_artwork_colors_contrast() {
    let dark = RgbColor { r: 20, g: 20, b: 20 };
    let bright = RgbColor { r: 240, g: 240, b: 240 };
    assert!(dark.luminance() < bright.luminance());
    assert!(dark.contrast_ratio(bright) >= 4.5);
}

#[test]
fn test_lyrics_query_normalization() {
    use music_player_core::lyrics::normalize_query;

    let (title, artist) = normalize_query("In The End (Official Music Video)", "Linkin Park");
    assert_eq!(title, "In The End");
    assert_eq!(artist, "Linkin Park");

    let (title2, artist2) = normalize_query("Artist Name - Track Title [Slowed + Reverb]", "Unknown");
    assert_eq!(title2, "Track Title");
    assert_eq!(artist2, "Artist Name");

    let (title3, artist3) = normalize_query("Song Name (Feat. Guest) - Remix", "Artist");
    assert_eq!(title3, "Song Name (Feat. Guest)");
    assert_eq!(artist3, "Artist");
}

#[test]
fn test_tag_editor_backup_history() {
    use music_player_core::metadata::TagEditor;
    use music_player_core::models::TagEdit;
    use std::fs;

    let temp_dir = std::env::temp_dir().join(format!("mp_tag_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let backup_dir = temp_dir.join("backups");
    fs::create_dir_all(&backup_dir).unwrap();

    let test_file = temp_dir.join("test_song.mp3");
    fs::write(&test_file, b"DUMMY_MP3_CONTENT").unwrap();

    let editor = TagEditor::new(&backup_dir);
    let edit = TagEdit {
        title: Some("New Title".to_string()),
        artist: Some("New Artist".to_string()),
        ..Default::default()
    };

    let backup = editor.save(&test_file, &edit).expect("Save should succeed");
    assert!(std::path::Path::new(&backup.backup_path).exists());

    let backups = editor.list_backups(&test_file);
    assert_eq!(backups.len(), 1);

    // Test restore
    let undo = editor.restore(&backups[0]).expect("Restore should succeed");
    assert!(std::path::Path::new(&undo.backup_path).exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_watcher_batch_deduplication() {
    use music_player_core::scanner::{DrainResult, LibraryWatchBatch};
    use std::path::PathBuf;
    use std::time::Instant;

    let mut batch = LibraryWatchBatch::new();
    batch.add_directory(PathBuf::from(r"C:\Music"));
    batch.add_directory(PathBuf::from(r"C:\Music\Rock"));
    batch.add_directory(PathBuf::from(r"C:\Music\Rock\Queen"));
    batch.add_directory(PathBuf::from(r"C:\Podcasts"));

    let DrainResult { directories, full_scan, retry_after } = batch.drain(Instant::now());
    assert!(!full_scan);
    assert!(retry_after.is_none());
    assert_eq!(directories.len(), 2);
    assert!(directories.contains(&PathBuf::from(r"C:\Music")));
    assert!(directories.contains(&PathBuf::from(r"C:\Podcasts")));
}

// ---------------------------------------------------------------------------
// Playback coordinator / navigation / listening parity tests (additive)
// ---------------------------------------------------------------------------

use music_player_core::audio::coordinator::{
    NoticeSeverity, PlaybackCommandKind, PlaybackCoordinator,
};
use music_player_core::audio::listening::{PlayCompletion, PlaybackListeningTracker};
use music_player_core::audio::queue::{build_external_open_queue, QueueCoordinator as NavQueue};

fn coordinator_track(path: &str) -> Track {
    let mut track = sample_track(path, "T", "A", "Al");
    track.duration_ms = 180_000;
    track
}

/// Writes empty files so `File.Exists`-style checks behave like the C# tests.
fn coordinator_files(names: &[&str]) -> Vec<(String, std::path::PathBuf)> {
    let dir = std::env::temp_dir().join(format!(
        "mp_coord_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    names
        .iter()
        .map(|name| {
            let file = dir.join(name);
            std::fs::write(&file, b"").unwrap();
            (file.to_string_lossy().into_owned(), file)
        })
        .collect()
}

fn any_resolver(path: &str) -> Option<Track> {
    Some(coordinator_track(path))
}

#[test]
fn coordinator_advance_skips_missing_entries_and_repeat_modes() {
    // [a(exists), gone(missing), b(exists)], automatic advance must skip gone.
    let files = coordinator_files(&["a.mp3", "b.mp3"]);
    let (a, _) = (&files[0].0, &files[0].1);
    let (b, _) = (&files[1].0, &files[1].1);
    let gone = files[0].0.replace("a.mp3", "gone_missing.mp3");

    let mut queue = NavQueue::new();
    queue.set_repeat_mode(music_player_core::RepeatMode::Off);
    queue.replace(vec![a.clone(), gone.clone(), b.clone()], Some(0));
    let mut coordinator = PlaybackCoordinator::new(queue);

    let result = coordinator.advance(true, &any_resolver, None);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(b.as_str()));
    assert_eq!(coordinator.queue().current_index(), Some(2));
    assert!(result.persist_session);
    assert!(result.queue_changed);

    // changedWhileSkipping: with Track repeat and the CURRENT entry missing,
    // the first attempt re-selects the current (repeat wrap), the skip flag
    // flips to manual advance, and the next playable entry wins.
    let mut queue = NavQueue::new();
    queue.set_repeat_mode(music_player_core::RepeatMode::Track);
    queue.replace(vec![gone.clone(), b.clone()], Some(0));
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.advance(true, &any_resolver, None);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(b.as_str()));
    assert_eq!(coordinator.queue().current_index(), Some(1));

    // Track repeat with a playable current entry repeats it.
    let mut queue = NavQueue::new();
    queue.set_repeat_mode(music_player_core::RepeatMode::Track);
    queue.replace(vec![a.clone(), b.clone()], Some(0));
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.advance(true, &any_resolver, None);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(a.as_str()));
    assert_eq!(coordinator.queue().current_index(), Some(0));

    // Queue repeat wraps to the first entry.
    let mut queue = NavQueue::new();
    queue.set_repeat_mode(music_player_core::RepeatMode::Queue);
    queue.replace(vec![a.clone(), b.clone()], Some(1));
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.advance(true, &any_resolver, None);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(a.as_str()));
    assert_eq!(coordinator.queue().current_index(), Some(0));

    // Repeat Off at the end stops; nothing changed while skipping.
    let mut queue = NavQueue::new();
    queue.set_repeat_mode(music_player_core::RepeatMode::Off);
    queue.replace(vec![a.clone(), b.clone()], Some(1));
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.advance(true, &any_resolver, None);
    assert_eq!(result.kind, PlaybackCommandKind::StopPlayback);
    assert!(!result.queue_changed);

    // An empty queue is rebuilt from the autoplay continuation paths.
    let mut coordinator = PlaybackCoordinator::new(NavQueue::new());
    let result = coordinator.advance(
        true,
        &any_resolver,
        Some(vec![a.clone(), b.clone()]),
    );
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(a.as_str()));
    assert_eq!(coordinator.queue().entries().len(), 2);

    // A resolver that cannot resolve the entry behaves like a missing file.
    let mut queue = NavQueue::new();
    queue.replace(vec![a.clone(), b.clone()], Some(0));
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.advance(true, &|_| None, None);
    assert_eq!(result.kind, PlaybackCommandKind::StopPlayback);
    // The unresolvable current entry was marked skipped (manual advance ran).
    assert!(result.queue_changed);
    assert_eq!(coordinator.queue().current_index(), Some(1));
}

#[test]
fn coordinator_start_toggle_previous_play_queue_entry() {
    let files = coordinator_files(&["a.mp3", "b.mp3"]);
    let (a, _) = (&files[0].0, &files[0].1);
    let (b, _) = (&files[1].0, &files[1].1);
    let missing = files[0].0.replace("a.mp3", "not_there.mp3");

    // Missing file at its saved location.
    let mut coordinator = PlaybackCoordinator::new(NavQueue::new());
    let result = coordinator.start_track(&coordinator_track(&missing), false, None, &[], &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::TrackUnavailable);
    let notice = result.notice.expect("notice");
    assert!(notice.message.contains(&missing));
    assert_eq!(notice.severity, NoticeSeverity::Warning);

    // Reset queue from view paths, honoring a matching requested index.
    let mut coordinator = PlaybackCoordinator::new(NavQueue::new());
    coordinator.queue_mut().set_shuffle(true);
    let result = coordinator.start_track(
        &coordinator_track(b),
        true,
        Some(1),
        &[a.clone(), b.clone()],
        &any_resolver,
    );
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(coordinator.queue().entries(), &[a.clone(), b.clone()]);
    assert_eq!(coordinator.queue().current_index(), Some(1));
    // AB repeat is cleared on every start.
    coordinator.queue_mut().set_ab_repeat_points(Some(10), Some(20));
    let _ = coordinator.start_track(&coordinator_track(a), false, None, &[], &any_resolver);
    assert!(coordinator.queue().repeat_a_ms().is_none());
    assert!(coordinator.queue().repeat_b_ms().is_none());

    // Toggle decisions: paused -> Resumed, near end -> restart, no track -> NoChange.
    let mut coordinator = PlaybackCoordinator::new(NavQueue::new());
    coordinator.set_current_track_path(Some(a.clone()));
    let result = coordinator.toggle(Some(&coordinator_track(a)), true, false, 0, 180_000, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::Paused);
    let result = coordinator.toggle(Some(&coordinator_track(a)), false, false, 0, 180_000, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::Resumed);
    let result = coordinator.toggle(Some(&coordinator_track(a)), false, true, 180_000, 180_000, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(a.as_str()));
    let result = coordinator.toggle(None, false, false, 0, 0, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::NoChange);
    // Resume of a vanished file is unavailable.
    let result = coordinator.toggle(Some(&coordinator_track(&missing)), false, false, 0, 180_000, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::TrackUnavailable);

    // Previous within 3 s walks back; beyond 3 s it is a position reset.
    let mut queue = NavQueue::new();
    queue.replace(vec![a.clone(), b.clone()], Some(1));
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.previous(Some(&coordinator_track(b)), 5_000, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::PositionReset);
    let result = coordinator.previous(Some(&coordinator_track(b)), 2_000, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    assert_eq!(result.track_path.as_deref(), Some(a.as_str()));
    let result = coordinator.previous(Some(&coordinator_track(a)), 0, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::PositionReset);

    // Queue entries that no longer resolve report unavailable.
    let mut queue = NavQueue::new();
    queue.replace(vec![a.clone(), b.clone()], None);
    let mut coordinator = PlaybackCoordinator::new(queue);
    let result = coordinator.play_queue_entry(0, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::TrackStarted);
    let result = coordinator.play_queue_entry(7, &any_resolver);
    assert_eq!(result.kind, PlaybackCommandKind::TrackUnavailable);
    assert!(result.notice.is_some());

    // Prepare crossfade requires a resolvable target.
    let mut queue = NavQueue::new();
    queue.replace(vec![a.clone(), b.clone()], Some(0));
    let mut coordinator = PlaybackCoordinator::with_crossfade_ms(queue, 1_500);
    let result = coordinator.advance(true, &any_resolver, None);
    assert_eq!(result.kind, PlaybackCommandKind::CrossfadeRequested);
    let request = result.crossfade.expect("crossfade request");
    assert_eq!(request.target_index, 1);
    assert_eq!(request.source_index, 0);
    assert_eq!(request.duration_ms, 1_500);
    assert_eq!(coordinator.queue().current_index(), Some(1));
}

#[test]
fn listening_tracker_credit_rules() {
    // Forward deltas beyond 3000 ms are never credited.
    let mut tracker = PlaybackListeningTracker::new();
    tracker.reset_for_track();
    assert!(!tracker.observe(true, true, 10_000, 180_000));
    assert_eq!(tracker.heard_milliseconds(), 0);
    assert!(!tracker.observe(true, true, 11_000, 180_000));
    assert_eq!(tracker.heard_milliseconds(), 1_000);
    // Paused observations update the position without credit.
    assert!(!tracker.observe(true, false, 11_500, 180_000));
    assert_eq!(tracker.heard_milliseconds(), 1_000);
    assert!(!tracker.observe(true, true, 12_000, 180_000));
    assert_eq!(tracker.heard_milliseconds(), 1_500);
    // Backwards deltas (seeks) are not credited either.
    assert!(!tracker.observe(true, true, 500, 180_000));
    assert_eq!(tracker.heard_milliseconds(), 1_500);
    // No track means no credit and no threshold.
    tracker.reset_for_track();
    assert!(!tracker.observe(false, true, 90_000, 180_000));

    // The half-duration threshold fires exactly once per track.
    tracker.reset_for_track();
    let mut fired = 0;
    for position in 0..=180_000i64 {
        if tracker.observe(true, true, position, 180_000) {
            fired += 1;
        }
    }
    // 1 ms steps: each delta is 1 ms, so the half point (90_000 ms) is hit once.
    assert_eq!(fired, 1);
    assert!(tracker.counted_current_play());
    assert!(tracker.heard_milliseconds() >= 90_000);
    assert!(!tracker.observe(true, true, 180_000, 180_000));

    // RestorePaused starts a restored session without credit.
    tracker.restore_paused(60_000);
    assert_eq!(tracker.heard_milliseconds(), 0);
    assert!(!tracker.counted_current_play());
    // MarkResumed rebases after a pause.
    tracker.mark_resumed(70_000);
    assert!(!tracker.observe(true, true, 71_000, 180_000));
    assert_eq!(tracker.heard_milliseconds(), 1_000);

    // PlayCompletion threshold mirrors the C# tick math (fractional halves).
    assert!(PlayCompletion::has_reached_half(180_000, 90_000));
    assert!(!PlayCompletion::has_reached_half(180_000, 89_999));
    assert!(!PlayCompletion::has_reached_half(3, 1));
    assert!(PlayCompletion::has_reached_half(3, 2));
    assert!(!PlayCompletion::has_reached_half(0, 1_000));
}

#[test]
fn build_external_open_queue_semantics() {
    // Opened first, current kept, unplayed remainder appended.
    let opened = vec!["b.mp3".to_string(), "c.mp3".to_string()];
    let existing = vec![
        "a.mp3".to_string(),
        "x.mp3".to_string(),
        "b.mp3".to_string(),
        "y.mp3".to_string(),
    ];
    let result = build_external_open_queue(&opened, Some("a.mp3"), &existing, 0);
    assert_eq!(
        result,
        vec!["b.mp3", "c.mp3", "a.mp3", "x.mp3", "y.mp3"]
    );

    // Case-insensitive dedup and no duplicate current.
    let opened = vec!["A.mp3".to_string(), "a.mp3".to_string()];
    let result = build_external_open_queue(&opened, Some("a.mp3"), &["z.mp3".to_string()], -1);
    assert_eq!(result, vec!["A.mp3", "z.mp3"]);

    // Blank opened paths are dropped; nothing opens from an empty list.
    let opened = vec![String::new(), "  ".to_string()];
    let result = build_external_open_queue(&opened, None, &[], 0);
    assert!(result.is_empty());

    // A current index past the end clamps past the queue: nothing is appended.
    let opened = vec!["b.mp3".to_string()];
    let existing = vec!["a.mp3".to_string(), "b.mp3".to_string(), "c.mp3".to_string()];
    let result = build_external_open_queue(&opened, None, &existing, 99);
    assert_eq!(result, vec!["b.mp3"]);
}

#[test]
fn queue_move_and_remove_constraints() {
    let mut queue = NavQueue::new();
    let paths = vec![
        "a.mp3".to_string(),
        "b.mp3".to_string(),
        "c.mp3".to_string(),
        "d.mp3".to_string(),
    ];
    queue.replace(paths.clone(), Some(1));

    // Only upcoming entries (after the current one) are movable.
    assert!(!queue.move_entry(1, 1));
    assert!(!queue.move_entry(1, -1));
    assert!(!queue.move_entry_to(2, 0));
    assert!(!queue.move_entry_to(2, 2)); // target == index
    assert!(!queue.move_entry(3, 1)); // target out of range
    assert!(queue.move_entry(2, 1));
    assert_eq!(queue.entries(), &["a.mp3", "b.mp3", "d.mp3", "c.mp3"]);
    assert!(queue.move_entry(3, -1));
    assert_eq!(queue.entries(), &paths);
    assert_eq!(queue.content_revision(), 3);

    // Removing the current entry forces manual advance for the next call,
    // so a Track repeat cannot wrap onto a vanished entry.
    let mut queue = NavQueue::new();
    queue.set_repeat_mode(music_player_core::RepeatMode::Track);
    queue.replace(vec!["a.mp3".to_string(), "b.mp3".to_string(), "c.mp3".to_string()], Some(1));
    assert!(queue.remove_paths(&["b.mp3".to_string()]));
    assert_eq!(queue.entries(), &["a.mp3".to_string(), "c.mp3".to_string()]);
    assert_eq!(queue.current_index(), Some(0));
    assert!(queue.current_entry_removed());
    // With the flag set, even Track repeat advances to the next entry.
    assert_eq!(queue.next_index(true), Some(1));
    // An explicit selection clears the flag again.
    queue.set_current_index(Some(0));
    assert!(!queue.current_entry_removed());
    assert_eq!(queue.next_index(true), Some(0));

    // Removing nothing reports false and keeps the revision.
    let revision = queue.content_revision();
    assert!(!queue.remove_paths(&["zzz.mp3".to_string()]));
    assert_eq!(queue.content_revision(), revision);

    // ensure_queue only fills an empty queue and keeps the current track.
    let mut queue = NavQueue::new();
    queue.ensure_queue(&paths, Some("c.mp3"));
    assert_eq!(queue.current_index(), Some(2));
    queue.ensure_queue(&["e.mp3".to_string()], None);
    assert_eq!(queue.entries().len(), 4);

    // insert_next_range lands right after the current entry.
    let mut queue = NavQueue::new();
    queue.replace(vec!["a.mp3".to_string(), "d.mp3".to_string()], Some(0));
    queue.insert_next_range(
        &["b.mp3".to_string(), "c.mp3".to_string()],
        Some("a.mp3"),
    );
    assert_eq!(
        queue.entries(),
        &["a.mp3", "b.mp3", "c.mp3", "d.mp3"]
    );
    assert_eq!(queue.current_index(), Some(0));

    // ensure_current_track inserts after the current entry when unknown.
    let mut queue = NavQueue::new();
    queue.replace(vec!["a.mp3".to_string(), "b.mp3".to_string()], Some(0));
    queue.ensure_current_track(Some("x.mp3"));
    assert_eq!(queue.entries(), &["a.mp3", "x.mp3", "b.mp3"]);
    assert_eq!(queue.current_index(), Some(1));

    // Shuffle-upcoming only touches indices at or after the start.
    let mut queue = NavQueue::new();
    queue.replace(vec!["a.mp3".to_string(), "b.mp3".to_string()], Some(0));
    let before = queue.entries().to_vec();
    queue.shuffle_upcoming(1);
    assert_eq!(queue.entries()[0], before[0]);
    assert_eq!(queue.content_revision(), 1); // one upcoming entry: no bump
    queue.replace(paths, Some(0));
    queue.shuffle_upcoming(1);
    assert_eq!(queue.entries()[0], "a.mp3");
    let mut upcoming: Vec<String> = queue.entries()[1..].to_vec();
    upcoming.sort();
    assert_eq!(
        upcoming,
        vec!["b.mp3", "c.mp3", "d.mp3"]
    );
    assert!(queue.content_revision() > 1);

    // Cycle repeat mode mirrors QueueNavigation.CycleRepeatMode.
    let mut queue = NavQueue::new();
    assert_eq!(queue.repeat_mode, music_player_core::RepeatMode::Off);
    queue.cycle_repeat_mode();
    assert_eq!(queue.repeat_mode, music_player_core::RepeatMode::Queue);
    queue.cycle_repeat_mode();
    assert_eq!(queue.repeat_mode, music_player_core::RepeatMode::Track);
    queue.cycle_repeat_mode();
    assert_eq!(queue.repeat_mode, music_player_core::RepeatMode::Off);
}

#[test]
fn crossfade_volume_curve_matches_csharp() {
    use music_player_core::audio::crossfade_volumes;

    assert_eq!(crossfade_volumes(100, 0.0), (100, 0));
    assert_eq!(crossfade_volumes(100, 1.0), (0, 100));
    // Equal-power midpoint: cube-law conversion keeps both halves equal.
    let (out, inc) = crossfade_volumes(100, 0.5);
    assert_eq!(out, inc);
    assert_eq!(out, 89);
    assert_eq!(crossfade_volumes(0, 0.5), (0, 0));
    // Inputs are clamped like the C# implementation.
    assert_eq!(crossfade_volumes(150, 2.0), (0, 100));
    assert_eq!(crossfade_volumes(75, -1.0), (75, 0));
    // Monotonic handover around the midpoint.
    let (out_a, inc_a) = crossfade_volumes(100, 0.4);
    let (out_b, inc_b) = crossfade_volumes(100, 0.6);
    assert!(out_a > out_b);
    assert!(inc_a < inc_b);
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mpcore-{}-{}-{}",
        name,
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn availability_reconcile_marks_and_purges() {
    let root = temp_dir("availability");
    let file_a = root.join("a.mp3");
    let file_b = root.join("b.mp3");
    std::fs::write(&file_a, b"audio").unwrap();
    std::fs::write(&file_b, b"audio").unwrap();
    let path_a = file_a.to_string_lossy().to_string();
    let path_b = file_b.to_string_lossy().to_string();
    let never_indexed = root.join("missing.mp3").to_string_lossy().to_string();

    let store = LibraryStore::in_memory().expect("in-memory db failed");
    store.upsert_track(&sample_track(&path_a, "A", "Artist", "Album")).unwrap();
    store.upsert_track(&sample_track(&path_b, "B", "Artist", "Album")).unwrap();
    store.upsert_track(&sample_track(&never_indexed, "Missing", "Artist", "Album")).unwrap();

    let service = TrackAvailabilityService::new(&store);
    // A row whose file is gone while its folder exists is a confirmed
    // deletion: the track is purged, not flagged unavailable.
    let result = service.reconcile_indexed_tracks(&[], &[], None);
    assert_eq!(result.removed_paths, vec![never_indexed.clone()]);
    assert_eq!(result.unavailable_count, 0);
    assert!(store.get_track(&never_indexed).unwrap().is_none());
    assert!(!store.get_track(&path_a).unwrap().unwrap().unavailable);

    // Tracks on an unreachable volume stay flagged unavailable instead.
    let ghost_drive = unavailable_drive_letter();
    let ghost_track = format!("{}\\Remote\\song.mp3", ghost_drive);
    store.upsert_track(&sample_track(&ghost_track, "Offline", "Artist", "Album")).unwrap();
    let result = service.reconcile_indexed_tracks(&[], &[], None);
    assert_eq!(result.unavailable_count, 1);
    assert!(result.removed_paths.is_empty());
    assert!(store.get_track(&ghost_track).unwrap().unwrap().unavailable);
    let result = service.reconcile_indexed_tracks(&[], &[], None);
    assert_eq!(result.recovered_count, 0, "the file is still unreachable");

    // Deleting a file on disk purges the index row and its references.
    store.create_playlist("p1", "Mix").unwrap();
    store.add_tracks_to_playlist("p1", &[path_b.clone()]).unwrap();
    std::fs::remove_file(&file_b).unwrap();
    let result = service.reconcile_indexed_tracks(&[], &[], None);
    assert_eq!(result.removed_paths.len(), 1);
    assert_eq!(result.removed_paths[0], path_b);
    assert!(store.get_track(&path_b).unwrap().is_none());
    assert!(
        store.get_playlist_entries("p1").unwrap().is_empty(),
        "playlist reference to the purged track must go with it"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// Picks a drive letter whose root volume is not mounted, standing in for an
/// offline storage location.
fn unavailable_drive_letter() -> String {
    for code in (b'A'..=b'Z').rev() {
        let letter = code as char;
        if std::fs::metadata(format!("{}:\\", letter)).is_err() {
            return format!("{}:", letter);
        }
    }
    panic!("every drive letter is mounted; cannot simulate an offline volume");
}

#[test]
fn availability_reconcile_protected_prefixes_count_unavailable() {
    let root = temp_dir("availability-protected");
    let file_a = root.join("a.mp3");
    std::fs::write(&file_a, b"audio").unwrap();
    let path_a = file_a.to_string_lossy().to_string();
    let missing = root.join("missing.mp3").to_string_lossy().to_string();

    let store = LibraryStore::in_memory().expect("in-memory db failed");
    store.upsert_track(&sample_track(&path_a, "A", "Artist", "Album")).unwrap();
    store.upsert_track(&sample_track(&missing, "Missing", "Artist", "Album")).unwrap();

    let service = TrackAvailabilityService::new(&store);
    // An unavailable root protects every track beneath it: nothing is purged
    // or reinstated, and all rows count as unavailable.
    let result = service.reconcile_indexed_tracks(
        &[root.to_string_lossy().to_string()],
        &[],
        None,
    );
    assert_eq!(result.unavailable_count, 2);
    assert!(result.removed_paths.is_empty());
    assert!(store.get_track(&path_a).unwrap().is_some());
    assert!(store.get_track(&missing).unwrap().is_some());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn reconcile_removed_paths_purges_descendants_of_deleted_folder() {
    let root = temp_dir("removed-paths");
    let album = root.join("album");
    std::fs::create_dir_all(&album).unwrap();
    let file_1 = album.join("1.mp3");
    let file_2 = album.join("2.mp3");
    std::fs::write(&file_1, b"audio").unwrap();
    std::fs::write(&file_2, b"audio").unwrap();
    let path_1 = file_1.to_string_lossy().to_string();
    let path_2 = file_2.to_string_lossy().to_string();

    let store = LibraryStore::in_memory().expect("in-memory db failed");
    store.upsert_track(&sample_track(&path_1, "One", "Artist", "Album")).unwrap();
    store.upsert_track(&sample_track(&path_2, "Two", "Artist", "Album")).unwrap();
    store.create_playlist("p1", "Album").unwrap();
    store.add_tracks_to_playlist("p1", &[path_1.clone(), path_2.clone()]).unwrap();

    // Delete one file: the watcher reports the exact path.
    std::fs::remove_file(&file_1).unwrap();
    let service = TrackAvailabilityService::new(&store);
    let confirmed = service.reconcile_removed_paths(&[path_1.clone()]);
    assert_eq!(confirmed, vec![path_1.clone()]);
    assert!(store.get_track(&path_1).unwrap().is_none());
    let entries = store.get_playlist_entries("p1").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].position, 0);
    assert_eq!(entries[0].path, path_2);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn purge_deleted_tracks_compacts_playlists_and_queue() {
    let store = LibraryStore::in_memory().expect("in-memory db failed");
    let paths: Vec<String> = (1..=4)
        .map(|i| format!("C:/Music/track{}.mp3", i))
        .collect();
    for (index, path) in paths.iter().enumerate() {
        store
            .upsert_track(&sample_track(path, &format!("T{}", index), "Artist", "Album"))
            .unwrap();
    }
    store.create_playlist("p1", "Mix").unwrap();
    store.add_tracks_to_playlist("p1", &paths).unwrap();
    store.save_queue(&paths).unwrap();

    // Purge two tracks in one pass.
    store.purge_deleted_tracks(&[paths[1].clone(), paths[3].clone()]).unwrap();

    let entries = store.get_playlist_entries("p1").unwrap();
    let compacted: Vec<(u32, String)> = entries
        .iter()
        .map(|entry| (entry.position, entry.path.clone()))
        .collect();
    assert_eq!(
        compacted,
        vec![(0, paths[0].clone()), (1, paths[2].clone())],
        "surviving playlist rows resequence from zero"
    );

    let queue = store.load_queue().unwrap();
    assert_eq!(queue, vec![paths[0].clone(), paths[2].clone()]);

    // Saved references with no index row are discoverable for orphan cleanup.
    let referenced = store.get_referenced_paths().unwrap();
    assert!(referenced.is_empty());
}

#[test]
fn scan_coordinator_lifecycle_and_empty_start() {
    let store = LibraryStore::in_memory().expect("in-memory db failed");
    let mut coordinator = LibraryScanCoordinator::new(&store);

    // No roots and no force refresh: StartAsync returns null.
    assert!(!coordinator.start(&[], &[], false, None, None));
    assert!(!coordinator.is_running());
    assert!(!coordinator.toggle_pause());
    assert!(!coordinator.cancel());
    assert_eq!(
        coordinator.get_status(),
        music_player_core::ScanStatus {
            active: false,
            paused: false,
            cancelling: false
        }
    );

    let root = temp_dir("coordinator");
    std::fs::write(root.join("x.mp3"), b"not really an mp3").unwrap();
    assert!(coordinator.start(&[root.clone()], &[], false, None, None));
    assert!(coordinator.is_running());

    // Pause, then cancel: the worker finishes promptly with a result.
    assert!(coordinator.toggle_pause());
    assert!(coordinator.get_status().paused);
    assert!(coordinator.cancel());
    assert!(coordinator.get_status().cancelling);
    assert!(!coordinator.toggle_pause(), "pause is rejected while cancelling");

    let deadline = Instant::now() + Duration::from_secs(15);
    while coordinator.is_running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!coordinator.is_running(), "cancelled scan must finish");
    let result = coordinator.poll_result();
    assert!(result.is_some(), "cancelled scan still yields a result");
    if let Some(Ok(index_result)) = result {
        // The fake file is skipped; nothing should crash the run.
        let _ = index_result.indexed;
    }
    assert!(!coordinator.is_running());

    std::fs::remove_dir_all(&root).ok();
}
