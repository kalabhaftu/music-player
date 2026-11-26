use music_player_core::{
    parse_lrc, plain_text, EqualizerState, LibraryFileWatcher, LibraryScanCoordinator, LibraryStore,
    QueueCoordinator, RepeatMode, RgbColor, TagEdit, TagEditor, Track, TrackAvailabilityService,
    TrackSort, inspect_file, read_tags, read_tags_in_cache,
};
use music_player_core::metadata::tag_reader::{
    read_additional_standard_fields, read_custom_fields,
};
use chrono::Utc;
use std::collections::HashMap;
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
fn grouped_search_uses_all_terms_and_reports_filtered_group_count() {
    let store = LibraryStore::in_memory().expect("in-memory db failed");
    let mut goodbye = sample_track(
        "C:/Music/goodbye.mp3",
        "Goodbye",
        "Elton John",
        "Goodbye Yellow Brick Road",
    );
    goodbye.album_artist = "Elton John".to_string();
    let mut live = sample_track("C:/Music/live.mp3", "Live", "Bryan Adams", "Live");
    live.album_artist = "Elton John".to_string();
    let other = sample_track("C:/Music/other.mp3", "Other", "Other", "Yellow Road");
    store.upsert_track(&goodbye).unwrap();
    store.upsert_track(&live).unwrap();
    store.upsert_track(&other).unwrap();

    let (albums, total) = store
        .query_group_page("album", Some("elton goodbye"), 0, 24, false)
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "Goodbye Yellow Brick Road");

    let (albums, total) = store
        .query_group_page("album", Some("elton"), 1, 1, true)
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "Live");
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
fn test_lyrics_match_scoring() {
    use music_player_core::lyrics::{select_lyrics_match, lyric_match_score, timing_matches, LyricsSearchResult};

    assert!(timing_matches(200.0, 204.0));
    assert!(!timing_matches(200.0, 220.0));

    let good_result = LyricsSearchResult {
        id: 1,
        track_name: "In The End".to_string(),
        artist_name: "Linkin Park".to_string(),
        album_name: Some("Hybrid Theory".to_string()),
        duration: 216.0,
        instrumental: false,
        plain_lyrics: Some("It starts with one thing".to_string()),
        synced_lyrics: Some("[00:10.50] It starts with one thing\n[00:13.20] I don't know why".to_string()),
    };

    let bad_result = LyricsSearchResult {
        id: 2,
        track_name: "Completely Different Song".to_string(),
        artist_name: "Other Artist".to_string(),
        album_name: None,
        duration: 216.0,
        instrumental: false,
        plain_lyrics: Some("different words".to_string()),
        synced_lyrics: Some("[00:10.00] different words".to_string()),
    };

    let score_good = lyric_match_score("In The End (Official)", "Linkin Park", 216.0, &good_result);
    assert!(score_good > 0.8, "Expected high match score, got {}", score_good);

    let score_bad = lyric_match_score("In The End (Official)", "Linkin Park", 216.0, &bad_result);
    assert!(score_bad < 0.0, "Expected negative match score, got {}", score_bad);

    let results = vec![bad_result, good_result];
    let selected = select_lyrics_match("In The End", "Linkin Park", 216.0, &results);
    assert!(selected.is_some());
    let m = selected.unwrap();
    assert_eq!(m.result.id, 1);
    assert!(m.timed);
    assert!(m.raw.contains("It starts with one thing"));

    // Verify preference for uncensored lyrics over censored ones
    let censored_result = LyricsSearchResult {
        id: 10,
        track_name: "A Tale Of 2 Citiez".to_string(),
        artist_name: "J. Cole".to_string(),
        album_name: Some("2014 Forest Hills Drive".to_string()),
        duration: 269.0,
        instrumental: false,
        plain_lyrics: None,
        synced_lyrics: Some("[00:10.00] Last night I saw a b**** and f*** that".to_string()),
    };
    let uncensored_result = LyricsSearchResult {
        id: 11,
        track_name: "A Tale Of 2 Citiez".to_string(),
        artist_name: "J. Cole".to_string(),
        album_name: Some("2014 Forest Hills Drive".to_string()),
        duration: 269.0,
        instrumental: false,
        plain_lyrics: None,
        synced_lyrics: Some("[00:10.00] Last night I saw a bitch and fuck that".to_string()),
    };
    let candidates = vec![censored_result, uncensored_result];
    let pick = select_lyrics_match("A Tale Of 2 Citiez", "J. Cole", 269.0, &candidates);
    assert!(pick.is_some());
    assert_eq!(pick.unwrap().result.id, 11, "Should select uncensored version");
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

// ---------------------------------------------------------------------------
// Tag writer round-trip tests (TagEditor::save for every container)
// ---------------------------------------------------------------------------

const ASF_TEST_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_TEST_FILE_PROPERTIES_GUID: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const ASF_TEST_CONTENT_DESCRIPTION_GUID: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const ASF_TEST_EXTENDED_CONTENT_GUID: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50,
];

fn synchsafe_size(size: usize) -> [u8; 4] {
    let v = size as u32;
    [
        ((v >> 21) & 0x7F) as u8,
        ((v >> 14) & 0x7F) as u8,
        ((v >> 7) & 0x7F) as u8,
        (v & 0x7F) as u8,
    ]
}

fn id3v23_tag(payload: &[u8]) -> Vec<u8> {
    let mut tag = Vec::with_capacity(10 + payload.len());
    tag.extend_from_slice(b"ID3");
    tag.push(3);
    tag.push(0);
    tag.push(0);
    tag.extend_from_slice(&synchsafe_size(payload.len()));
    tag.extend_from_slice(payload);
    tag
}

fn id3_frame(id: &str, content: &[u8]) -> Vec<u8> {
    let mut frame = id.as_bytes().to_vec();
    frame.extend_from_slice(&(content.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(content);
    frame
}

fn id3_text_frame(id: &str, values: &[&str]) -> Vec<u8> {
    let mut content = vec![3u8];
    for value in values {
        content.extend_from_slice(value.as_bytes());
        content.push(0);
    }
    id3_frame(id, &content)
}

fn id3_txxx_frame(description: &str, value: &str) -> Vec<u8> {
    let mut content = vec![3u8];
    content.extend_from_slice(description.as_bytes());
    content.push(0);
    content.extend_from_slice(value.as_bytes());
    content.push(0);
    id3_frame("TXXX", &content)
}

fn png_bytes() -> Vec<u8> {
    let mut bytes = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(b"PNGDATA");
    bytes
}

fn write_png(root: &std::path::Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, png_bytes()).unwrap();
    path
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn ape_item(key: &str, value: &[u8]) -> Vec<u8> {
    let mut item = ((key.len() + 1 + value.len()) as u32).to_le_bytes().to_vec();
    item.extend_from_slice(&0u32.to_le_bytes()); // flags
    item.extend_from_slice(key.as_bytes());
    item.push(0);
    item.extend_from_slice(value);
    item
}

fn ape_tag_bytes(items: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for item in items {
        body.extend_from_slice(item);
    }
    // APEv2 layout: header (flags 0xA0000000) + items + footer (0x80000000);
    // TagLib's size field counts the items plus the footer only.
    let tag_size = (body.len() + 32) as u32;
    let mut out = Vec::new();
    for (flags, is_footer) in [(0xA000_0000u32, false), (0x8000_0000u32, true)] {
        out.extend_from_slice(b"APETAGEX");
        out.extend_from_slice(&2000u32.to_le_bytes());
        out.extend_from_slice(&tag_size.to_le_bytes());
        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        if is_footer {
            out.extend_from_slice(&body);
        }
        out.extend_from_slice(&[0u8; 8]);
    }
    out
}

fn asf_wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect()
}

fn asf_object(guid: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let mut out = guid.to_vec();
    out.extend_from_slice(&((body.len() + 24) as u64).to_le_bytes());
    out.extend_from_slice(body);
    out
}

fn asf_file(cd: [&str; 5], ecd: &[(&str, &str)]) -> Vec<u8> {
    // File Properties: play duration lives at body offset 40 (100ns units).
    let mut file_props = vec![0u8; 48];
    file_props[40..48].copy_from_slice(&5_000_000u64.to_le_bytes());
    let mut cd_body = Vec::new();
    let texts: Vec<Vec<u8>> = cd.iter().map(|text| asf_wide(text)).collect();
    for text in &texts {
        cd_body.extend_from_slice(&(text.len() as u16).to_le_bytes());
    }
    for text in &texts {
        cd_body.extend_from_slice(text);
    }
    let mut ecd_body = Vec::new();
    ecd_body.extend_from_slice(&(ecd.len() as u16).to_le_bytes());
    for (name, value) in ecd {
        let name_bytes = asf_wide(name);
        let value_bytes = asf_wide(value);
        ecd_body.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        ecd_body.extend_from_slice(&name_bytes);
        ecd_body.extend_from_slice(&0u16.to_le_bytes()); // unicode type
        ecd_body.extend_from_slice(&(value_bytes.len() as u16).to_le_bytes());
        ecd_body.extend_from_slice(&value_bytes);
    }
    let mut header = Vec::new();
    header.extend_from_slice(&ASF_TEST_HEADER_GUID);
    header.extend_from_slice(&0u64.to_le_bytes());
    header.extend_from_slice(&[0x01, 0x02, 0xFF, 0x1B, 0x5A, 0x43]);
    header.extend_from_slice(&asf_object(&ASF_TEST_FILE_PROPERTIES_GUID, &file_props));
    header.extend_from_slice(&asf_object(&ASF_TEST_CONTENT_DESCRIPTION_GUID, &cd_body));
    header.extend_from_slice(&asf_object(&ASF_TEST_EXTENDED_CONTENT_GUID, &ecd_body));
    let header_size = header.len() as u64;
    header[16..24].copy_from_slice(&header_size.to_le_bytes());
    header.extend_from_slice(b"asf-media-payload");
    header
}

fn mp4_atom(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as u32 + 8).to_be_bytes().to_vec();
    out.extend_from_slice(name);
    out.extend_from_slice(body);
    out
}

fn mp4_data_atom(data_type: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() as u32) + 16).to_be_bytes().to_vec();
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_type.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // locale
    out.extend_from_slice(payload);
    out
}

fn mp4_freeform_child(key: &str, value: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&mp4_atom(
        b"mean",
        &[&[0u8; 4][..], b"com.apple.iTunes"].concat(),
    ));
    body.extend_from_slice(&mp4_atom(b"name", &[&[0u8; 4][..], key.as_bytes()].concat()));
    body.extend_from_slice(&mp4_data_atom(1, value.as_bytes()));
    mp4_atom(b"----", &body)
}

fn mp4_seed() -> (Vec<u8>, usize) {
    let ftyp = mp4_atom(b"ftyp", b"isom");
    let mp4a = mp4_atom(b"mp4a", &{
        let mut body = vec![0u8; 6]; // reserved
        body.extend_from_slice(&[0, 1]); // data reference index
        body.extend_from_slice(&[0, 0]); // version
        body.extend_from_slice(&[0, 0]); // revision
        body.extend_from_slice(&[0u8; 4]); // vendor
        body.extend_from_slice(&[0, 2]); // channels
        body.extend_from_slice(&[0, 16]); // sample size
        body.extend_from_slice(&[0, 0]); // compression id
        body.extend_from_slice(&[0, 0]); // packet size
        body.extend_from_slice(&[0xAC, 0x44, 0, 0]); // 44100 (16.16 fixed)
        body
    });
    let stsd = mp4_atom(b"stsd", &{
        let mut body = vec![0u8; 4]; // version/flags
        body.extend_from_slice(&1u32.to_be_bytes()); // entry count
        body.extend_from_slice(&mp4a);
        body
    });
    let stco = mp4_atom(b"stco", &{
        let mut body = vec![0u8; 4]; // version/flags
        body.extend_from_slice(&1u32.to_be_bytes()); // entry count
        body.extend_from_slice(&0u32.to_be_bytes()); // patched after assembly
        body
    });
    let stbl = mp4_atom(b"stbl", &[&stsd[..], &stco[..]].concat());
    let minf = mp4_atom(b"minf", &stbl);
    let mdia = mp4_atom(b"mdia", &[&mp4_atom(b"mdhd", &vec![0u8; 24])[..], &minf[..]].concat());
    let trak = mp4_atom(b"trak", &[&mp4_atom(b"tkhd", &vec![0u8; 84])[..], &mdia[..]].concat());
    let mvhd = mp4_atom(b"mvhd", &{
        let mut body = vec![0u8; 100];
        body[12..16].copy_from_slice(&1000u32.to_be_bytes()); // timescale
        body[16..20].copy_from_slice(&1000u32.to_be_bytes()); // duration
        body
    });
    let hdlr = mp4_atom(b"hdlr", &{
        let mut body = Vec::new();
        body.extend_from_slice(&[0u8; 8]); // version/flags + pre_defined
        body.extend_from_slice(b"mdir");
        body.extend_from_slice(b"appl");
        body.extend_from_slice(&[0u8; 9]); // reserved + empty name
        body
    });
    let ilst = mp4_atom(
        b"ilst",
        &[
            &mp4_atom(b"zzzz", &mp4_data_atom(1, b"KeepMe"))[..],
            &mp4_freeform_child("FOO", "OldFoo")[..],
        ]
        .concat(),
    );
    let meta = mp4_atom(b"meta", &[&[0u8; 4][..], &hdlr[..], &ilst[..]].concat());
    let udta = mp4_atom(b"udta", &meta);
    let moov = mp4_atom(b"moov", &[&mvhd[..], &trak[..], &udta[..]].concat());
    let mdat_payload = b"music-payload-bytes";
    // The mdat atom directly follows the moov atom.
    let mdat_offset = ftyp.len() + moov.len();
    let mdat = mp4_atom(b"mdat", mdat_payload);
    let mut seed = Vec::new();
    seed.extend_from_slice(&ftyp);
    seed.extend_from_slice(&moov);
    seed.extend_from_slice(&mdat);
    let stco_pos = find_bytes(&seed, b"stco").unwrap();
    // The chunk-offset table starts after the 8-byte header plus the 4-byte
    // version/flags and 4-byte entry count: name_pos + 12.
    seed[stco_pos + 12..stco_pos + 16].copy_from_slice(&(mdat_offset as u32).to_be_bytes());
    (seed, mdat_offset)
}

fn vorbis_comment_block(vendor: &str, entries: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    let vendor_bytes = vendor.as_bytes();
    body.extend_from_slice(&(vendor_bytes.len() as u32).to_le_bytes());
    body.extend_from_slice(vendor_bytes);
    body.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (key, value) in entries {
        let comment = format!("{}={}", key, value);
        body.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        body.extend_from_slice(comment.as_bytes());
    }
    body
}

fn flac_block(kind: u8, body: &[u8], is_last: bool) -> Vec<u8> {
    let mut out = vec![if is_last { kind | 0x80 } else { kind }];
    out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..4]);
    out.extend_from_slice(body);
    out
}

fn flac_seed() -> Vec<u8> {
    // STREAMINFO with 44100 Hz, 2 channels, 16 bits, 44100 samples.
    let mut streaminfo = vec![0u8; 34];
    streaminfo[10] = 0x0A; // sample rate 44100 >> 12
    streaminfo[11] = 0xC4; // (44100 >> 4) & 0xFF
    streaminfo[12] = 0x4B; // low rate nibble + channels-1 (1) + bps-1 (15)
    streaminfo[14..18].copy_from_slice(&44100u32.to_be_bytes()); // total samples
    let mut out = Vec::new();
    out.extend_from_slice(b"fLaC");
    out.extend_from_slice(&flac_block(0, &streaminfo, false));
    out.extend_from_slice(&flac_block(1, b"padpadpadpad", false));
    out.extend_from_slice(&flac_block(
        4,
        &vorbis_comment_block(
            "test-vendor",
            &[
                ("TITLE", "Old Title"),
                ("ARTIST", "Old Artist"),
                ("SOURCE", "Old Source"),
                ("REPLAYGAIN_TRACK_GAIN", "old"),
            ],
        ),
        true,
    ));
    out
}

#[test]
fn writer_mp3_standard_composites_custom_and_preservation() {
    let root = temp_dir("writer-mp3");
    let mut payload = Vec::new();
    payload.extend(id3_text_frame("TIT2", &["Old Title"]));
    payload.extend(id3_text_frame("TRCK", &["7/12"]));
    payload.extend(id3_text_frame("TMED", &["CD"]));
    payload.extend(id3_frame("PRIV", b"owner.example\x00\x01\x02\x03"));
    let mut sylt = vec![3u8, b'e', b'n', b'g', 0];
    sylt.extend_from_slice(b"Sync text");
    payload.extend(id3_frame("SYLT", &sylt));
    let mut seed = id3v23_tag(&payload);
    seed.extend_from_slice(b"audio-bytes-here");
    let file = root.join("song.mp3");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));
    let png = write_png(&root, "cover.png");

    // Save 1: standard fields + artwork; custom fields untouched (None).
    let edit1 = TagEdit {
        title: Some("New Title".to_string()),
        artist: Some("Artist One; Artist Two".to_string()),
        album: Some("New Album".to_string()),
        album_artist: Some("Album Artist".to_string()),
        genre: Some("Rock".to_string()),
        year: Some(1985),
        track_number: Some(3),
        lyrics: Some("Some lyrics".to_string()),
        artwork_path: Some(png.to_string_lossy().into_owned()),
        ..Default::default()
    };
    editor.save(&file, &edit1).unwrap();

    let cache = root.join("cache");
    let tags = read_tags_in_cache(&file, &cache);
    assert_eq!(tags.title.as_deref(), Some("New Title"));
    assert_eq!(tags.artist.as_deref(), Some("Artist One; Artist Two"));
    assert_eq!(tags.album.as_deref(), Some("New Album"));
    assert_eq!(tags.album_artist.as_deref(), Some("Album Artist"));
    assert_eq!(tags.genre.as_deref(), Some("Rock"));
    assert_eq!(tags.year, 1985);
    assert_eq!(tags.track_number, 3);
    assert!(tags.has_lyrics);
    assert_eq!(tags.lyrics.as_deref(), Some("Some lyrics"));

    // Untouched custom frames survive a custom_fields = None edit.
    let custom1 = read_custom_fields(&file);
    assert_eq!(custom1.get("ID3:TMED").map(String::as_str), Some("CD"));
    let saved = std::fs::read(&file).unwrap();
    assert!(find_bytes(&saved, b"PRIV").is_some());
    assert!(find_bytes(&saved, b"owner.example").is_some());
    // The lyrics edit removes the competing SYLT frame.
    assert!(find_bytes(&saved, b"SYLT").is_none());
    assert!(find_bytes(&saved, b"image/png").is_some());

    // Artwork round-trips through a cache directory.
    let artwork_path = tags.artwork_path.expect("artwork saved");
    assert_eq!(std::fs::read(&artwork_path).unwrap(), png_bytes());

    // The track composite kept its total ("3/12").
    let additional1 = read_additional_standard_fields(&file);
    assert_eq!(additional1.get("TRACK_COUNT").map(String::as_str), Some("12"));

    // Save 2: custom reconcile (TMED drops), lyrics cleared, total updated.
    let edit2 = TagEdit {
        lyrics: Some(String::new()),
        custom_fields: Some(HashMap::from([("SOURCE".to_string(), "CD".to_string())])),
        additional_fields: Some(HashMap::from([("TRACK_COUNT".to_string(), "14".to_string())])),
        ..Default::default()
    };
    editor.save(&file, &edit2).unwrap();

    let custom2 = read_custom_fields(&file);
    assert_eq!(custom2.get("SOURCE").map(String::as_str), Some("CD"));
    assert!(custom2.get("ID3:TMED").is_none());
    let tags2 = read_tags(&file);
    assert!(!tags2.has_lyrics);
    assert_eq!(tags2.track_number, 3);
    let additional2 = read_additional_standard_fields(&file);
    assert_eq!(additional2.get("TRACK_COUNT").map(String::as_str), Some("14"));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_flac_roundtrip_with_artwork_and_aliases() {
    let root = temp_dir("writer-flac");
    let seed = flac_seed();
    let file = root.join("song.flac");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));
    let png = write_png(&root, "cover.png");

    let edit = TagEdit {
        title: Some("Flac Title".to_string()),
        artist: Some("Flac Artist".to_string()),
        album: Some("Flac Album".to_string()),
        genre: Some("Rock".to_string()),
        year: Some(2011),
        track_number: Some(4),
        lyrics: Some("Flac lyric".to_string()),
        artwork_path: Some(png.to_string_lossy().into_owned()),
        custom_fields: Some(HashMap::from([("SOURCE".to_string(), "CD".to_string())])),
        additional_fields: Some(HashMap::from([(
            "REPLAYGAIN_TRACK_GAIN".to_string(),
            "-7.5".to_string(),
        )])),
        ..Default::default()
    };
    editor.save(&file, &edit).unwrap();

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Flac Title"));
    assert_eq!(tags.artist.as_deref(), Some("Flac Artist"));
    assert_eq!(tags.album.as_deref(), Some("Flac Album"));
    assert_eq!(tags.genre.as_deref(), Some("Rock"));
    assert_eq!(tags.year, 2011);
    assert_eq!(tags.track_number, 4);
    assert!(tags.has_lyrics);
    // STREAMINFO duration: 44100 samples at 44100 Hz.
    assert_eq!(tags.duration_ms, 1000);

    let cache = root.join("cache");
    let tags_cached = read_tags_in_cache(&file, &cache);
    let artwork_path = tags_cached.artwork_path.expect("artwork saved");
    assert_eq!(std::fs::read(&artwork_path).unwrap(), png_bytes());

    let custom = read_custom_fields(&file);
    assert_eq!(custom.get("SOURCE").map(String::as_str), Some("CD"));
    let additional = read_additional_standard_fields(&file);
    assert_eq!(
        additional.get("REPLAYGAIN_TRACK_GAIN").map(String::as_str),
        Some("-7.5")
    );

    let saved = std::fs::read(&file).unwrap();
    assert_eq!(find_bytes(&saved, b"fLaC"), Some(0));
    // The STREAMINFO block must remain the first metadata block.
    assert_eq!(saved[4] & 0x7F, 0);
    // Padding block and the vendor string are preserved; the replaced
    // custom value is gone.
    assert!(find_bytes(&saved, b"padpadpadpad").is_some());
    assert!(find_bytes(&saved, b"test-vendor").is_some());
    assert!(find_bytes(&saved, b"Old Source").is_none());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_wma_roundtrip_cd_ecd_and_additional() {
    let root = temp_dir("writer-wma");
    let seed = asf_file(
        ["OldTitle", "OldAuthor", "(c) Old", "Old description", "5"],
        &[
            ("WM/AlbumTitle", "OldAlbum"),
            ("WM/Genre", "OldGenre"),
            ("WM/Year", "1999"),
            ("WM/TrackNumber", "5"),
            ("WM/Mood", "Blue"),
        ],
    );
    let file = root.join("song.wma");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    let edit = TagEdit {
        title: Some("Wma Title".to_string()),
        artist: Some("Wma Artist".to_string()),
        album: Some("Wma Album".to_string()),
        genre: Some("Rock".to_string()),
        year: Some(2001),
        track_number: Some(2),
        lyrics: Some("Wma lyric".to_string()),
        custom_fields: Some(HashMap::from([("MOOD".to_string(), "Happy".to_string())])),
        additional_fields: Some(HashMap::from([
            ("BPM".to_string(), "123".to_string()),
            ("DISC".to_string(), "2".to_string()),
        ])),
        ..Default::default()
    };
    editor.save(&file, &edit).unwrap();

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Wma Title"));
    assert_eq!(tags.artist.as_deref(), Some("Wma Artist"));
    assert_eq!(tags.album.as_deref(), Some("Wma Album"));
    assert_eq!(tags.genre.as_deref(), Some("Rock"));
    assert_eq!(tags.year, 2001);
    assert_eq!(tags.track_number, 2);
    assert!(tags.has_lyrics);
    // File Properties play duration: 5_000_000 * 100ns = 500 ms.
    assert_eq!(tags.duration_ms, 500);

    let custom = read_custom_fields(&file);
    // The reconcile removed WM/Mood in favor of the desired MOOD key.
    assert!(custom.get("WM/Mood").is_none());
    assert_eq!(custom.get("MOOD").map(String::as_str), Some("Happy"));

    let additional = read_additional_standard_fields(&file);
    assert_eq!(additional.get("BPM").map(String::as_str), Some("123"));
    assert_eq!(additional.get("DISC").map(String::as_str), Some("2"));
    assert_eq!(additional.get("TITLE_SORT").map(String::as_str), Some(""));

    // Reserved header bytes and trailing data are preserved.
    let saved = std::fs::read(&file).unwrap();
    assert_eq!(&saved[24..30], &[0x01, 0x02, 0xFF, 0x1B, 0x5A, 0x43]);
    assert!(find_bytes(&saved, b"asf-media-payload").is_some());
    // CD keeps copyright/description/rating when only title/author change.
    assert!(find_bytes(&saved, &asf_wide("Old description")).is_some());
    assert!(find_bytes(&saved, &asf_wide("OldAlbum")).is_none());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_wav_fixes_container_size_and_replaces_id3_chunk() {
    let root = temp_dir("writer-wav");
    let audio = b"audiodata-payload";
    let mut seed = Vec::new();
    seed.extend_from_slice(b"RIFF");
    seed.extend_from_slice(&0u32.to_le_bytes()); // deliberately wrong size
    seed.extend_from_slice(b"WAVE");
    seed.extend_from_slice(b"fmt ");
    seed.extend_from_slice(&16u32.to_le_bytes());
    seed.extend_from_slice(&[1, 0, 1, 0, 0x80, 0x1F, 0, 0, 0x80, 0x1F, 0, 0, 1, 0, 8, 0]);
    seed.extend_from_slice(b"data");
    seed.extend_from_slice(&(audio.len() as u32).to_le_bytes());
    seed.extend_from_slice(audio);
    seed.push(0); // chunk pad
    let mut id3_payload = id3_text_frame("TALB", &["Old Album"]);
    id3_payload.extend(id3_txxx_frame("SOURCE", "Old Source"));
    let tag = id3v23_tag(&id3_payload);
    seed.extend_from_slice(b"ID3 ");
    seed.extend_from_slice(&(tag.len() as u32).to_le_bytes());
    seed.extend_from_slice(&tag);
    let file = root.join("song.wav");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    let edit = TagEdit {
        title: Some("Wav Title".to_string()),
        custom_fields: Some(HashMap::from([("SOURCE".to_string(), "CD".to_string())])),
        ..Default::default()
    };
    editor.save(&file, &edit).unwrap();

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Wav Title"));
    assert_eq!(tags.album.as_deref(), Some("Old Album"));
    let custom = read_custom_fields(&file);
    assert_eq!(custom.get("SOURCE").map(String::as_str), Some("CD"));

    let saved = std::fs::read(&file).unwrap();
    let size = u32::from_le_bytes([saved[4], saved[5], saved[6], saved[7]]);
    assert_eq!(size as usize, saved.len() - 8);
    assert!(find_bytes(&saved, b"audiodata-payload").is_some());
    // The tag chunk is normalized to uppercase "ID3 ".
    assert!(find_bytes(&saved, b"id3 ").is_none());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_aiff_roundtrip_comm_and_custom() {
    let root = temp_dir("writer-aiff");
    let audio = b"aiff-audio-data";
    let mut seed = Vec::new();
    seed.extend_from_slice(b"FORM");
    seed.extend_from_slice(&0u32.to_be_bytes());
    seed.extend_from_slice(b"AIFF");
    seed.extend_from_slice(b"COMM");
    seed.extend_from_slice(&18u32.to_be_bytes());
    seed.extend_from_slice(&2u16.to_be_bytes()); // channels
    seed.extend_from_slice(&100_000u32.to_be_bytes()); // frames
    seed.extend_from_slice(&16u16.to_be_bytes()); // bits
    seed.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]); // 44100
    seed.extend_from_slice(b"SSND");
    seed.extend_from_slice(&((audio.len() as u32) + 8).to_be_bytes());
    seed.extend_from_slice(&0u32.to_be_bytes()); // offset
    seed.extend_from_slice(&0u32.to_be_bytes()); // block size
    seed.extend_from_slice(audio);
    let tag = id3v23_tag(&id3_text_frame("TIT2", &["Old Title"]));
    seed.extend_from_slice(b"ID3 ");
    seed.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    seed.extend_from_slice(&tag);
    let file = root.join("song.aiff");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    let edit = TagEdit {
        title: Some("Aiff Title".to_string()),
        custom_fields: Some(HashMap::from([(
            "ID3:TMED".to_string(),
            "Vinyl".to_string(),
        )])),
        ..Default::default()
    };
    editor.save(&file, &edit).unwrap();

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Aiff Title"));
    // COMM duration: 100 frames at 44100 Hz.
    assert_eq!(tags.duration_ms, 2267);
    let custom = read_custom_fields(&file);
    assert_eq!(custom.get("ID3:TMED").map(String::as_str), Some("Vinyl"));

    let saved = std::fs::read(&file).unwrap();
    let size = u32::from_be_bytes([saved[4], saved[5], saved[6], saved[7]]);
    assert_eq!(size as usize, saved.len() - 8);
    assert!(find_bytes(&saved, b"aiff-audio-data").is_some());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_dsf_updates_pointer_and_size() {
    let root = temp_dir("writer-dsf");
    let audio = b"dsd-audio-payload";
    let mut seed = Vec::new();
    seed.extend_from_slice(b"DSD ");
    seed.extend_from_slice(&28u64.to_le_bytes()); // DSD chunk size
    seed.extend_from_slice(&0u64.to_le_bytes()); // total file size
    seed.extend_from_slice(&0u64.to_le_bytes()); // metadata pointer
    seed.extend_from_slice(b"fmt ");
    seed.extend_from_slice(&52u64.to_le_bytes());
    seed.extend_from_slice(&1u32.to_le_bytes()); // version
    seed.extend_from_slice(&0u32.to_le_bytes()); // format id (DSD raw)
    seed.extend_from_slice(&2u32.to_le_bytes()); // channel type
    seed.extend_from_slice(&2u32.to_le_bytes()); // channel num
    seed.extend_from_slice(&2822400u32.to_le_bytes()); // sampling freq
    seed.extend_from_slice(&1u32.to_le_bytes()); // bits per sample
    seed.extend_from_slice(&2822400u64.to_le_bytes()); // sample count
    seed.extend_from_slice(&4096u32.to_le_bytes()); // block size
    seed.extend_from_slice(b"data");
    seed.extend_from_slice(&((audio.len() as u64) + 8).to_le_bytes());
    seed.extend_from_slice(audio);
    let file = root.join("song.dsf");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    let edit = TagEdit {
        title: Some("Dsf Title".to_string()),
        ..Default::default()
    };
    editor.save(&file, &edit).unwrap();

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Dsf Title"));
    // fmt duration: 2822400 samples at 2822400 Hz.
    assert_eq!(tags.duration_ms, 1000);

    let saved = std::fs::read(&file).unwrap();
    let pointer = u64::from_le_bytes(saved[20..28].try_into().unwrap());
    assert_eq!(pointer as usize, seed.len());
    let total = u64::from_le_bytes(saved[12..20].try_into().unwrap());
    assert_eq!(total as usize, saved.len());

    // A second save replaces the tail tag and keeps the pointer.
    let edit2 = TagEdit {
        custom_fields: Some(HashMap::from([(
            "ID3:TMED".to_string(),
            "SACD".to_string(),
        )])),
        ..Default::default()
    };
    editor.save(&file, &edit2).unwrap();
    let custom = read_custom_fields(&file);
    assert_eq!(custom.get("ID3:TMED").map(String::as_str), Some("SACD"));
    let saved2 = std::fs::read(&file).unwrap();
    let pointer2 = u64::from_le_bytes(saved2[20..28].try_into().unwrap());
    assert_eq!(pointer2, pointer);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_mp4_grow_patch_shrink_free_and_errors() {
    let root = temp_dir("writer-mp4");
    let (seed, mdat_offset) = mp4_seed();
    let file = root.join("song.m4a");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    // Grow: the fresh ilst extends moov; chunk offsets shift with it.
    let edit1 = TagEdit {
        title: Some("M4a Title".to_string()),
        genre: Some("Rock".to_string()),
        additional_fields: Some(HashMap::from([(
            "COMMENT".to_string(),
            "Hi".to_string(),
        )])),
        ..Default::default()
    };
    editor.save(&file, &edit1).unwrap();
    let out1 = std::fs::read(&file).unwrap();
    assert!(out1.len() > seed.len());
    let delta1 = (out1.len() - seed.len()) as u32;
    let stco1 = find_bytes(&out1, b"stco").unwrap();
    let entry1 = u32::from_be_bytes(out1[stco1 + 12..stco1 + 16].try_into().unwrap());
    assert_eq!(entry1, mdat_offset as u32 + delta1);
    // Unknown ilst children and freeform atoms survive.
    assert!(find_bytes(&out1, b"zzzz").is_some());
    assert!(find_bytes(&out1, b"KeepMe").is_some());
    assert!(find_bytes(&out1, b"com.apple.iTunes").is_some());

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("M4a Title"));
    assert_eq!(tags.genre.as_deref(), Some("Rock"));
    assert_eq!(tags.duration_ms, 1000);
    let details = inspect_file(&file).expect("mp4 details");
    assert_eq!(details.sample_rate_hz, 44100);
    assert_eq!(details.bits_per_sample, 16);
    let additional = read_additional_standard_fields(&file);
    assert_eq!(additional.get("COMMENT").map(String::as_str), Some("Hi"));
    // The freeform seed value is not surfaced as a catalog field.
    assert_eq!(additional.get("BPM").map(String::as_str), Some(""));

    // Shrink: clearing the genre pads with `free` so offsets stay stable.
    let edit2 = TagEdit {
        genre: Some(String::new()),
        ..Default::default()
    };
    editor.save(&file, &edit2).unwrap();
    let out2 = std::fs::read(&file).unwrap();
    assert_eq!(out2.len(), out1.len());
    let stco2 = find_bytes(&out2, b"stco").unwrap();
    let entry2 = u32::from_be_bytes(out2[stco2 + 12..stco2 + 16].try_into().unwrap());
    assert_eq!(entry2, entry1);
    assert!(find_bytes(&out2, b"free").is_some());
    let tags2 = read_tags(&file);
    assert!(tags2.genre.is_none());
    assert_eq!(tags2.title.as_deref(), Some("M4a Title"));
    assert_eq!(
        read_additional_standard_fields(&file)
            .get("COMMENT")
            .map(String::as_str),
        Some("Hi")
    );

    // Custom fields are rejected for MP4.
    let edit3 = TagEdit {
        custom_fields: Some(HashMap::from([("X".to_string(), "1".to_string())])),
        ..Default::default()
    };
    let err = editor.save(&file, &edit3).unwrap_err().to_string();
    assert!(
        err.contains("Custom tags are not supported for this file format."),
        "{}",
        err
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_ape_roundtrip_preserves_binary_and_id3v1() {
    let root = temp_dir("writer-ape");
    let audio = b"wv-audio-bytes";
    let mut seed = Vec::new();
    seed.extend_from_slice(audio);
    seed.extend_from_slice(&ape_tag_bytes(&[
        ape_item("Title", b"Old Title"),
        ape_item("Cover Art (front)", &png_bytes()),
    ]));
    // Trailing ID3v1 tag (128 bytes).
    let mut id3v1 = vec![0u8; 128];
    id3v1[..3].copy_from_slice(b"TAG");
    seed.extend_from_slice(&id3v1);
    let file = root.join("song.wv");
    std::fs::write(&file, &seed).unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    let edit = TagEdit {
        title: Some("Ape Title".to_string()),
        artist: Some("Ape Artist".to_string()),
        custom_fields: Some(HashMap::from([("MOOD".to_string(), "Chill".to_string())])),
        ..Default::default()
    };
    editor.save(&file, &edit).unwrap();

    let tags = read_tags(&file);
    assert_eq!(tags.title.as_deref(), Some("Ape Title"));
    assert_eq!(tags.artist.as_deref(), Some("Ape Artist"));
    let custom = read_custom_fields(&file);
    assert_eq!(custom.get("MOOD").map(String::as_str), Some("Chill"));

    let saved = std::fs::read(&file).unwrap();
    // The binary artwork item and the trailing ID3v1 tag are preserved.
    assert_eq!(find_bytes(&saved, &png_bytes()).is_some(), true);
    assert_eq!(&saved[saved.len() - 128..saved.len() - 125], b"TAG");
    assert!(find_bytes(&saved, b"APETAGEX").is_some());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_ogg_copy_only_and_unsupported_formats() {
    let root = temp_dir("writer-ogg-unsupported");
    let editor = TagEditor::new(&root.join("backups"));

    // The OGG family is copy-only: the file bytes stay identical.
    let ogg = root.join("song.ogg");
    std::fs::write(&ogg, b"OggS-fake-ogg-payload").unwrap();
    let edit = TagEdit {
        title: Some("Ogg Title".to_string()),
        ..Default::default()
    };
    editor.save(&ogg, &edit).unwrap();
    assert_eq!(std::fs::read(&ogg).unwrap(), b"OggS-fake-ogg-payload");

    // DFF, TTA and unknown extensions report TagLib's error.
    for (name, bytes) in [
        ("song.dff", b"DSD ".as_slice()),
        ("song.tta", b"TTA1".as_slice()),
        ("song.xyz", b"data".as_slice()),
    ] {
        let file = root.join(name);
        std::fs::write(&file, bytes).unwrap();
        let err = editor.save(&file, &edit).unwrap_err().to_string();
        assert!(err.contains("unsupported format"), "{}: {}", name, err);
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_validation_error_strings() {
    let root = temp_dir("writer-validation");
    let mp3 = root.join("song.mp3");
    std::fs::write(&mp3, b"audio").unwrap();
    let flac = root.join("song.flac");
    std::fs::write(&flac, b"fLaC-payload").unwrap();
    let m4a = root.join("song.m4a");
    std::fs::write(&m4a, b"mdat").unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    let save_err = |file: &std::path::Path, edit: &TagEdit| {
        editor.save(file, edit).unwrap_err().to_string()
    };

    // Additional fields.
    let err = save_err(
        &mp3,
        &TagEdit {
            additional_fields: Some(HashMap::from([("TRACK".to_string(), "5".to_string())])),
            ..Default::default()
        },
    );
    assert!(err.contains("Unknown standard tag field 'TRACK'."), "{}", err);

    let err = save_err(
        &mp3,
        &TagEdit {
            additional_fields: Some(HashMap::from([("BPM".to_string(), "abc".to_string())])),
            ..Default::default()
        },
    );
    assert!(err.contains("'BPM' must be a whole number."), "{}", err);

    let err = save_err(
        &mp3,
        &TagEdit {
            additional_fields: Some(HashMap::from([(
                "REPLAYGAIN_TRACK_GAIN".to_string(),
                "abc".to_string(),
            )])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("'REPLAYGAIN_TRACK_GAIN' must be a finite number."),
        "{}",
        err
    );

    let err = save_err(
        &mp3,
        &TagEdit {
            additional_fields: Some(HashMap::from([(
                "DATE_TAGGED".to_string(),
                "not-a-date".to_string(),
            )])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("'Date tagged' must use an ISO 8601 date and time."),
        "{}",
        err
    );

    // Custom fields.
    let err = save_err(
        &mp3,
        &TagEdit {
            custom_fields: Some(HashMap::from([("A=B".to_string(), "x".to_string())])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("Custom tag names cannot be empty or contain '=', or line breaks."),
        "{}",
        err
    );

    let err = save_err(
        &flac,
        &TagEdit {
            custom_fields: Some(HashMap::from([(
                "HAS SPACE".to_string(),
                "x".to_string(),
            )])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("Xiph field names can contain only ASCII letters, digits, and underscores."),
        "{}",
        err
    );

    let err = save_err(
        &flac,
        &TagEdit {
            custom_fields: Some(HashMap::from([("ARTIST".to_string(), "x".to_string())])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("'ARTIST' is already edited in a standard tag field."),
        "{}",
        err
    );

    let err = save_err(
        &mp3,
        &TagEdit {
            custom_fields: Some(HashMap::from([("ID3:XX".to_string(), "x".to_string())])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("Use ID3: followed by a non-standard four-character text frame identifier."),
        "{}",
        err
    );

    let err = save_err(
        &m4a,
        &TagEdit {
            custom_fields: Some(HashMap::from([("X".to_string(), "1".to_string())])),
            ..Default::default()
        },
    );
    assert!(
        err.contains("Custom tags are not supported for this file format."),
        "{}",
        err
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn writer_backup_prune_keeps_five_and_restore_roundtrip() {
    let root = temp_dir("writer-prune");
    let file = root.join("song.mp3");
    std::fs::write(&file, b"audio").unwrap();
    let editor = TagEditor::new(&root.join("backups"));

    for index in 1..=6 {
        let edit = TagEdit {
            title: Some(format!("T{}", index)),
            ..Default::default()
        };
        editor.save(&file, &edit).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(editor.list_backups(&file).len(), 5);

    // Restoring the newest snapshot rolls back to the state before save 6.
    let newest = editor.list_backups(&file)[0].clone();
    editor.restore(&newest).unwrap();
    assert_eq!(read_tags(&file).title.as_deref(), Some("T5"));
    // The restore created an undo snapshot; retention still caps at 5.
    assert_eq!(editor.list_backups(&file).len(), 5);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn opening_pre_v11_database_adds_unavailable_column_before_indexes() {
    let dir = std::env::temp_dir().join(format!("mp-legacy-schema-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("legacy.db");
    let _ = std::fs::remove_file(&path);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (
                path TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                artist TEXT NOT NULL DEFAULT '',
                album TEXT NOT NULL DEFAULT '',
                album_artist TEXT NOT NULL DEFAULT '',
                genre TEXT NOT NULL DEFAULT '',
                year INTEGER NOT NULL DEFAULT 0,
                track_number INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                file_size INTEGER NOT NULL DEFAULT 0,
                modified_utc TEXT NOT NULL,
                added_utc TEXT NOT NULL,
                favorite INTEGER NOT NULL DEFAULT 0,
                rating INTEGER NOT NULL DEFAULT 0,
                play_count INTEGER NOT NULL DEFAULT 0,
                last_played_utc TEXT NULL
            );",
        )
        .unwrap();
    }
    let store = LibraryStore::open(&path).unwrap();
    assert!(store.get_all_tracks(TrackSort::Title, false).unwrap().is_empty());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('tracks') WHERE name = 'unavailable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_lofty_metadata_adapter_and_hybrid_engine() {
    use music_player_core::metadata::{
        HybridMetadataEngine, LoftyAdapter, MetadataAdapter,
    };
    use std::path::Path;

    let lofty = LoftyAdapter::new();
    let hybrid = HybridMetadataEngine::new();

    // Verify behavior on non-existent file: returns clean Err, does not panic
    let non_existent = Path::new("non_existent_track.mp3");
    assert!(lofty.read_metadata(non_existent, None).is_err());
    assert!(lofty.inspect_file(non_existent).is_err());
    assert!(hybrid.inspect_file(non_existent).is_err());

    // Verify on real audio file if present in library
    let music_dir = std::env::var_os("MUSIC_PLAYER_TEST_MUSIC_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join("Music"))
        })
        .unwrap_or_default();
    if let Ok(entries) = std::fs::read_dir(&music_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("mp3")).unwrap_or(false) {
                let tags_lofty = lofty.read_metadata(&p, None).expect("lofty read failed");
                let tags_hybrid = hybrid.read_metadata(&p, None).expect("hybrid read failed");
                assert_eq!(tags_lofty.title, tags_hybrid.title);
                assert_eq!(tags_lofty.artist, tags_hybrid.artist);
                assert_eq!(tags_lofty.album, tags_hybrid.album);
                assert!(tags_lofty.duration_ms > 0);

                let details = hybrid.inspect_file(&p).expect("inspect failed");
                assert!(details.file_size > 0);
                assert!(details.duration_ms > 0);
                break;
            }
        }
    }
}

#[test]
fn test_stage1_controller_workflows_and_hybrid_engine() {
    use music_player_core::metadata::{
        read_tags, read_tags_in_cache, HybridMetadataEngine, TagEditor,
    };
    use music_player_core::TagEdit;

    let hybrid = HybridMetadataEngine::new();
    let root = temp_dir("stage1-test");
    let cache_dir = root.join("artwork_cache");
    std::fs::create_dir_all(&cache_dir).unwrap();

    // 1. Synthesize an MP3 file with ID3 tags and lyrics
    let mut payload = Vec::new();
    payload.extend(id3_text_frame("TIT2", &["Stage 1 Parity Track"]));
    payload.extend(id3_text_frame("TPE1", &["Parity Artist"]));
    payload.extend(id3_text_frame("TALB", &["Parity Album"]));
    payload.extend(id3_text_frame("TPE2", &["Album Artist"]));
    payload.extend(id3_text_frame("TCON", &["Synthwave"]));
    payload.extend(id3_text_frame("TRCK", &["5/12"]));
    payload.extend(id3_text_frame("TYER", &["2024"]));
    let mut uslt = vec![3u8, b'e', b'n', b'g', 0];
    uslt.extend_from_slice(b"Stage 1 Embedded Lyrics Line 1\nLine 2");
    payload.extend(id3_frame("USLT", &uslt));
    let mut seed = id3v23_tag(&payload);
    seed.extend_from_slice(b"fake-audio-payload-for-testing");
    let test_mp3 = root.join("stage1_track.mp3");
    std::fs::write(&test_mp3, &seed).unwrap();

    // Add cover image using TagEditor
    let png = write_png(&root, "cover.png");
    let editor = TagEditor::new(&root.join("backups"));
    let edit = TagEdit {
        artwork_path: Some(png.to_string_lossy().into_owned()),
        ..Default::default()
    };
    editor.save(&test_mp3, &edit).unwrap();

    // Workflow A: modal/details inspection (inspect_file) - as in modals.rs and views.rs
    let inspection = hybrid.inspect_file(&test_mp3).expect("inspect_file failed");
    assert!(inspection.file_size > 0);
    assert!(!inspection.container.is_empty());

    // Workflow B: tag reading for modal dialog (open_tags) with artwork cache
    let tags_modal = hybrid.read_metadata(&test_mp3, Some(&cache_dir)).expect("read_metadata with cache failed");
    assert_eq!(tags_modal.title.as_deref(), Some("Stage 1 Parity Track"));
    assert_eq!(tags_modal.artist.as_deref(), Some("Parity Artist"));
    assert_eq!(tags_modal.album.as_deref(), Some("Parity Album"));
    assert_eq!(tags_modal.album_artist.as_deref(), Some("Album Artist"));
    assert_eq!(tags_modal.genre.as_deref(), Some("Synthwave"));
    assert_eq!(tags_modal.track_number, 5);
    assert!(tags_modal.artwork_path.is_some());
    let art_path = tags_modal.artwork_path.as_ref().unwrap();
    assert!(std::path::Path::new(art_path).exists(), "Extracted artwork must exist in cache");

    // Workflow C: lyrics loading without artwork decode overhead (None passed for artwork_dir)
    // as in playback.rs line 86, views.rs line 1806, modals.rs line 619
    let tags_lyrics = hybrid.read_metadata(&test_mp3, None).expect("read_metadata without artwork failed");
    assert!(tags_lyrics.lyrics.is_some());
    assert!(tags_lyrics.lyrics.as_ref().unwrap().contains("Stage 1 Embedded Lyrics"));
    assert!(tags_lyrics.artwork_path.is_none(), "When artwork_dir is None, no artwork should be decoded/saved");

    // Workflow D: legacy entrypoint compatibility (read_tags, read_tags_in_cache)
    let legacy_tags = read_tags(&test_mp3);
    assert_eq!(legacy_tags.title.as_deref(), Some("Stage 1 Parity Track"));
    assert_eq!(legacy_tags.artist.as_deref(), Some("Parity Artist"));
    let legacy_cached_tags = read_tags_in_cache(&test_mp3, &cache_dir);
    assert_eq!(legacy_cached_tags.title.as_deref(), Some("Stage 1 Parity Track"));
    assert!(legacy_cached_tags.artwork_path.is_some());

    // Workflow E: verification against real library files if present
    let music_dir = std::env::var_os("MUSIC_PLAYER_TEST_MUSIC_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join("Music"))
        })
        .unwrap_or_default();
    if let Ok(entries) = std::fs::read_dir(&music_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("mp3")).unwrap_or(false) {
                // Read via hybrid streaming
                let tags = hybrid.read_metadata(&p, Some(&cache_dir)).expect("hybrid read failed on real track");
                assert!(!tags.title.as_deref().unwrap_or("").is_empty() || !tags.artist.as_deref().unwrap_or("").is_empty());

                // Read lyrics only (no artwork decode)
                let lyrics_only = hybrid.read_metadata(&p, None).expect("lyrics only read failed");
                assert_eq!(lyrics_only.title, tags.title);
                assert!(lyrics_only.artwork_path.is_none());

                // Inspect properties
                let details = hybrid.inspect_file(&p).expect("inspect failed on real track");
                assert!(details.duration_ms > 0);
                assert!(details.file_size > 0);
                break;
            }
        }
    }

    // Workflow F: FLAC format coverage with HybridMetadataEngine
    let flac_file = root.join("stage1_track.flac");
    std::fs::write(&flac_file, flac_seed()).unwrap();
    let flac_tags = hybrid.read_metadata(&flac_file, None).expect("hybrid flac read failed");
    assert_eq!(flac_tags.title.as_deref(), Some("Old Title"));
    assert_eq!(flac_tags.artist.as_deref(), Some("Old Artist"));
    let flac_details = hybrid.inspect_file(&flac_file).expect("hybrid flac inspect failed");
    assert_eq!(flac_details.sample_rate_hz, 44100);

    // Workflow G: MP4 format coverage with HybridMetadataEngine
    let (mp4_bytes, _) = mp4_seed();
    let mp4_file = root.join("stage1_track.m4a");
    std::fs::write(&mp4_file, &mp4_bytes).unwrap();
    let mp4_details = hybrid.inspect_file(&mp4_file).expect("hybrid mp4 inspect failed");
    assert!(mp4_details.file_size > 0);
}

#[test]
fn test_stage2_startup_discovery_fingerprints_and_threshold_behavior() {
    use music_player_core::IndexResult;
    use std::path::Path;

    let test_dir = temp_dir("startup-discovery-test");
    let music_folder = test_dir.join("music");
    let artwork_cache = test_dir.join("artwork");
    let db_path = test_dir.join("library.db");
    std::fs::create_dir_all(&music_folder).unwrap();
    std::fs::create_dir_all(&artwork_cache).unwrap();

    let store = LibraryStore::open(&db_path).expect("open store failed");

    // Helper closure to create synthetic mp3
    let create_mp3 = |path: &Path, title: &str, artist: &str| {
        let mut payload = Vec::new();
        payload.extend(id3_text_frame("TIT2", &[title]));
        payload.extend(id3_text_frame("TPE1", &[artist]));
        payload.extend(id3_text_frame("TALB", &["Startup Album"]));
        let mut seed = id3v23_tag(&payload);
        seed.extend_from_slice(b"fake-audio-payload-for-discovery");
        std::fs::write(path, &seed).unwrap();
    };

    // Helper closure to run coordinator scan synchronously
    let run_scan = |store: &LibraryStore, roots: &[PathBuf], cache: &Path| -> IndexResult {
        let mut coordinator = LibraryScanCoordinator::new(store).with_artwork_cache(cache.to_path_buf());
        assert!(coordinator.start(roots, &[], false, None, None));
        let deadline = Instant::now() + Duration::from_secs(15);
        while coordinator.is_running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!coordinator.is_running(), "scan timed out");
        coordinator.poll_result().expect("poll_result missing").expect("scan failed")
    };

    // 1. Initial State: Track 1 exists
    let song1 = music_folder.join("song1.mp3");
    create_mp3(&song1, "Song One", "Artist One");

    let result1 = run_scan(&store, &[music_folder.clone()], &artwork_cache);
    assert_eq!(result1.files_found, 1);
    assert_eq!(result1.indexed, 1);
    assert_eq!(result1.removed, 0);
    assert_eq!(store.get_library_stats().unwrap().total_tracks, 1);

    // 2. Simulate recent scan (< 12 hours ago, e.g. 5 minutes ago)
    let five_minutes_ago = Utc::now() - chrono::Duration::minutes(5);
    store
        .set_setting(
            music_player_core::settings::keys::LIBRARY_LAST_SCAN_UTC,
            &five_minutes_ago.to_rfc3339(),
        )
        .unwrap();

    // 3. User adds a new track (Song 2) while application is closed
    let song2 = music_folder.join("song2.mp3");
    create_mp3(&song2, "Song Two", "Artist Two");

    // 4. Startup discovery scan runs (force: false)
    // Must discover Song 2 even though last scan was only 5 minutes ago!
    let result2 = run_scan(&store, &[music_folder.clone()], &artwork_cache);
    assert_eq!(result2.files_found, 2);
    // Song 1 must be skipped via fingerprint (is_unchanged = true), only Song 2 is indexed:
    assert_eq!(result2.indexed, 1, "Only newly added Song 2 should be indexed");
    assert_eq!(result2.removed, 0);
    assert_eq!(store.get_library_stats().unwrap().total_tracks, 2);

    // 5. Subsequent run with zero changes
    // All 2 files must be skipped via fingerprint without re-reading metadata
    let result3 = run_scan(&store, &[music_folder.clone()], &artwork_cache);
    assert_eq!(result3.files_found, 2);
    assert_eq!(result3.indexed, 0, "No files should be indexed when unchanged");
    assert_eq!(result3.removed, 0);
    assert_eq!(store.get_library_stats().unwrap().total_tracks, 2);

    // 6. Delete Song 1 and verify deletion discovery on startup scan
    std::fs::remove_file(&song1).unwrap();
    let result4 = run_scan(&store, &[music_folder.clone()], &artwork_cache);
    assert_eq!(result4.files_found, 1);
    assert_eq!(result4.indexed, 0);
    assert_eq!(result4.removed, 1, "Deleted Song 1 must be removed from index");
    assert_eq!(store.get_library_stats().unwrap().total_tracks, 1);

    // Clean up
    drop(store);
    std::thread::sleep(Duration::from_millis(50));
    std::fs::remove_dir_all(&test_dir).ok();
}

#[test]
fn test_stage3_background_query_cancellation_and_epoch_invalidation() {
    use music_player_core::TrackQuery;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let test_dir = temp_dir("stage3-query-test");
    let db_path = test_dir.join("library.db");
    let store = LibraryStore::open(&db_path).expect("open store failed");

    // 1. Insert seed tracks
    let mut tracks = Vec::new();
    for i in 1..=50 {
        let mut t = sample_track(
            &format!("C:/Music/Track_{:03}.mp3", i),
            &format!("Title {:03}", i),
            &format!("Artist {}", i % 5),
            &format!("Album {}", i % 10),
        );
        t.duration_ms = 180_000;
        t.file_size = 1024 * 1024;
        t.year = 2020 + (i % 4);
        tracks.push(t);
    }
    store.upsert_tracks(&tracks).expect("upsert failed");

    // 2. Simulate query generation cancellation model:
    // User rapidly navigates: Gen 1 (Songs) -> Gen 2 (Albums) -> Gen 3 (Search)
    let active_generation = Arc::new(AtomicU64::new(1));
    let library_epoch = Arc::new(AtomicU64::new(10));

    // Spawn reader pool queries in parallel
    let store_clone1 = store.clone();
    let gen_clone1 = active_generation.clone();
    let epoch_clone1 = library_epoch.clone();
    let h1 = std::thread::spawn(move || {
        let task_gen = 1u64;
        let task_epoch = 10u64;
        let (tracks, total) = store_clone1
            .query_track_page(&TrackQuery {
                sort: TrackSort::Title,
                page_size: 20,
                ..TrackQuery::default()
            })
            .unwrap_or_default();
        let superseded = task_gen < gen_clone1.load(Ordering::Acquire)
            || task_epoch != epoch_clone1.load(Ordering::Acquire);
        (task_gen, superseded, tracks.len(), total)
    });

    // Advance generation to 2 immediately (simulating user clicking another view)
    active_generation.store(2, Ordering::Release);

    let store_clone2 = store.clone();
    let gen_clone2 = active_generation.clone();
    let epoch_clone2 = library_epoch.clone();
    let h2 = std::thread::spawn(move || {
        let task_gen = 2u64;
        let task_epoch = 10u64;
        let (groups, total) = store_clone2
            .query_group_page("album", None, 0, 10, false)
            .unwrap_or_default();
        let superseded = task_gen < gen_clone2.load(Ordering::Acquire)
            || task_epoch != epoch_clone2.load(Ordering::Acquire);
        (task_gen, superseded, groups.len(), total)
    });

    // Advance generation to 3 and advance library epoch (simulating cache invalidation)
    active_generation.store(3, Ordering::Release);
    library_epoch.store(11, Ordering::Release);

    let store_clone3 = store.clone();
    let gen_clone3 = active_generation.clone();
    let epoch_clone3 = library_epoch.clone();
    let h3 = std::thread::spawn(move || {
        let task_gen = 3u64;
        let task_epoch = 11u64;
        let (tracks, total) = store_clone3
            .query_track_page(&TrackQuery {
                search: Some("Title 00".into()),
                page_size: 10,
                ..TrackQuery::default()
            })
            .unwrap_or_default();
        let superseded = task_gen < gen_clone3.load(Ordering::Acquire)
            || task_epoch != epoch_clone3.load(Ordering::Acquire);
        (task_gen, superseded, tracks.len(), total)
    });

    let (g1, sup1, count1, total1) = h1.join().unwrap();
    let (g2, sup2, count2, total2) = h2.join().unwrap();
    let (g3, sup3, count3, total3) = h3.join().unwrap();

    // Gen 1 was superseded:
    assert_eq!(g1, 1);
    assert!(sup1, "Gen 1 should be superseded by newer generation");
    assert_eq!(count1, 20);
    assert_eq!(total1, 50);

    // Gen 2 was superseded by Gen 3 and epoch invalidation:
    assert_eq!(g2, 2);
    assert!(sup2, "Gen 2 should be superseded by Gen 3 and epoch invalidation");
    assert_eq!(count2, 10);
    assert_eq!(total2, 10);

    // Gen 3 was current and matches current epoch:
    assert_eq!(g3, 3);
    assert!(!sup3, "Gen 3 matches latest generation and epoch");
    assert_eq!(count3, 9);
    assert_eq!(total3, 9);

    drop(store);
    std::thread::sleep(Duration::from_millis(50));
    std::fs::remove_dir_all(&test_dir).ok();
}

#[test]
fn test_artwork_reliability_lifecycle_and_pruning_guards() {
    use music_player_core::metadata::{
        prune_unreferenced_artwork, save_artwork_to_cache_in, ArtworkOperationGuard,
    };

    let dir = std::env::temp_dir().join(format!("mp_art_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Valid PNG artwork
    let fake_png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00";
    let saved_path = save_artwork_to_cache_in(&dir, fake_png).unwrap();
    assert!(saved_path.is_file());
    assert!(saved_path.to_string_lossy().ends_with(".png"));

    let store = LibraryStore::in_memory().unwrap();

    // 2. Prune while in-flight operation guard is active MUST abort (return 0, keep file)
    {
        let _guard = ArtworkOperationGuard::new();
        let pruned = prune_unreferenced_artwork(&dir, &store);
        assert_eq!(pruned, 0);
        assert!(saved_path.is_file());
    }

    // 3. Prune when DB has 0 references MUST abort as safety guard (never wipe cache)
    let pruned = prune_unreferenced_artwork(&dir, &store);
    assert_eq!(pruned, 0);
    assert!(saved_path.is_file());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_sqlite_concurrency_single_writer_multi_reader() {
    let test_dir = std::env::temp_dir().join(format!("mp_concurrency_test_{}_{}", std::process::id(), fastrand()));
    std::fs::create_dir_all(&test_dir).unwrap();
    let db_path = test_dir.join("concurrency.db");

    let store = LibraryStore::open(&db_path).expect("failed to open disk store");

    // Spawn 4 reader threads querying concurrently
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut reader_handles = Vec::new();

    for _r_id in 0..4 {
        let store_clone = store.clone();
        let stop_clone = std::sync::Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let mut read_iterations = 0;
            while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = store_clone.get_all_tracks(TrackSort::Title, false);
                let _ = store_clone.get_library_stats();
                let _ = store_clone.search("test", 50);
                read_iterations += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            read_iterations
        });
        reader_handles.push(handle);
    }

    // Spawn 1 writer inserting batches
    let mut total_inserted = 0;
    for batch_idx in 0..10 {
        let mut batch = Vec::new();
        for item_idx in 0..20 {
            let id = batch_idx * 20 + item_idx;
            batch.push(sample_track(
                &format!("C:/Music/test_track_{}.mp3", id),
                &format!("Track {}", id),
                "Concurrency Artist",
                "Concurrency Album",
            ));
        }
        let inserted = store.upsert_tracks(&batch).expect("batch insert failed");
        total_inserted += inserted;
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    assert_eq!(total_inserted, 200);

    // Stop readers and gather stats
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for handle in reader_handles {
        let iterations = handle.join().expect("reader thread panicked");
        assert!(iterations > 0, "Reader thread did not execute queries");
    }

    let stats = store.get_library_stats().expect("failed to get library stats");
    assert_eq!(stats.total_tracks, 200);

    // Clean up
    drop(store);
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::remove_dir_all(&test_dir).ok();
}

#[test]
fn test_metadata_edit_history_indefinite_persistence() {
    let test_dir = std::env::temp_dir().join(format!("mp_history_test_{}_{}", std::process::id(), fastrand()));
    std::fs::create_dir_all(&test_dir).unwrap();
    let db_path = test_dir.join("history.db");

    let store = LibraryStore::open(&db_path).expect("failed to open disk store");
    let track_path = "C:/Music/Test/Song.mp3";

    // 1. Initial edit
    let edit_id1 = store
        .record_metadata_edit(
            track_path,
            "title",
            Some("Original Title"),
            Some("Edited Title 1"),
            r#"{"title":"Edited Title 1","artist":"Artist"}"#,
            Some("C:/Backups/song-1.bak"),
        )
        .expect("record edit 1 failed");
    assert!(edit_id1 > 0);

    // 2. Second edit
    let edit_id2 = store
        .record_metadata_edit(
            track_path,
            "artist",
            Some("Artist"),
            Some("New Artist"),
            r#"{"title":"Edited Title 1","artist":"New Artist"}"#,
            None, // full audio file backup optional/omitted
        )
        .expect("record edit 2 failed");
    assert!(edit_id2 > edit_id1);

    // 3. Query history: must return both entries, newest first
    let history = store
        .get_metadata_edit_history(track_path)
        .expect("get history failed");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, edit_id2);
    assert_eq!(history[0].field_name, "artist");
    assert_eq!(history[0].new_value.as_deref(), Some("New Artist"));
    assert_eq!(history[0].backup_file_path, None);

    assert_eq!(history[1].id, edit_id1);
    assert_eq!(history[1].field_name, "title");
    assert_eq!(history[1].old_value.as_deref(), Some("Original Title"));
    assert_eq!(history[1].new_value.as_deref(), Some("Edited Title 1"));
    assert_eq!(history[1].backup_file_path.as_deref(), Some("C:/Backups/song-1.bak"));

    // Clean up
    drop(store);
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::remove_dir_all(&test_dir).ok();
}

#[test]
fn test_stage5_event_driven_dispatch_and_polling_reduction() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    let test_dir = std::env::temp_dir().join(format!("mp_stage5_{}", fastrand()));
    std::fs::create_dir_all(&test_dir).unwrap();
    let db_path = test_dir.join("test.db");
    let store = LibraryStore::open(&db_path).expect("open db");

    // 1. Verify LibraryScanCoordinator completion notifier
    let scan_notified = Arc::new(AtomicBool::new(false));
    let scan_notified_clone = scan_notified.clone();

    let mut coordinator = LibraryScanCoordinator::new(&store)
        .with_completion_notifier(move || {
            scan_notified_clone.store(true, Ordering::SeqCst);
        });

    let scan_root = test_dir.join("music");
    std::fs::create_dir_all(&scan_root).unwrap();
    std::fs::write(scan_root.join("test.mp3"), b"ID3\x03\0\0\0\0\0\0").unwrap();

    let started = coordinator.start(&[scan_root.clone()], &[], false, None, None);
    assert!(started, "scan coordinator must start");

    // Wait for the completion notifier to trigger without requiring periodic polling
    let deadline = Instant::now() + Duration::from_secs(10);
    while !scan_notified.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        scan_notified.load(Ordering::SeqCst),
        "completion notifier must be called immediately when scan worker completes"
    );

    let result = coordinator.poll_result();
    assert!(result.is_some(), "poll_result must return completed result");

    // 2. Verify LibraryFileWatcher wake notifier and debounced batching
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_count_clone = wake_count.clone();

    let mut watcher = LibraryFileWatcher::new().with_wake_notifier(move || {
        wake_count_clone.fetch_add(1, Ordering::SeqCst);
    });

    let watch_root = test_dir.join("watch_music");
    std::fs::create_dir_all(&watch_root).unwrap();
    watcher.set_roots(&[watch_root.clone()], &[], None);

    // Give OS watcher a moment to register
    std::thread::sleep(Duration::from_millis(100));

    // Create a new audio file to trigger notify
    let new_file = watch_root.join("incoming.mp3");
    std::fs::write(&new_file, b"ID3\x03\0\0\0\0\0\0").unwrap();

    // Verify wake notifier is called on filesystem change
    let watch_deadline = Instant::now() + Duration::from_secs(5);
    while wake_count.load(Ordering::SeqCst) == 0 && Instant::now() < watch_deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        wake_count.load(Ordering::SeqCst) > 0,
        "wake notifier must be called immediately when filesystem events arrive"
    );

    // Pump watcher with event callbacks
    let mut rescanned = Vec::new();
    let mut on_rescan = |paths: &[std::path::PathBuf], _full: bool| {
        rescanned.extend_from_slice(paths);
    };
    watcher.pump(Instant::now(), None, Some(&mut on_rescan));

    // Clean up
    drop(store);
    std::thread::sleep(Duration::from_millis(50));
    std::fs::remove_dir_all(&test_dir).ok();
}

fn fastrand() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[test]
fn test_backup_failure_injection_phases_and_byte_for_byte_restoration() {
    use music_player_core::metadata::clean_stale_temp_files;

    let root = temp_dir("backup-failure-injection");
    let backups = root.join("backups");
    let editor = TagEditor::new(&backups);

    let song = root.join("original_track.mp3");
    let initial_bytes = b"ID3\x03\0\0\0\0\0\0MPEG_AUDIO_TEST_PAYLOAD_BYTE_EXACT_12345";
    std::fs::write(&song, initial_bytes).unwrap();

    // 1. Phase 4 Failure: Validation/format rejection (invalid custom ID3 frame)
    let mut custom = std::collections::HashMap::new();
    custom.insert("ID3:INVALID_LONG_TAG".to_string(), "Val".to_string());
    let invalid_edit = TagEdit {
        title: Some("New Title".to_string()),
        custom_fields: Some(custom),
        ..Default::default()
    };
    let err = editor.save(&song, &invalid_edit).unwrap_err();
    assert!(err.to_string().contains("four-character") || err.to_string().contains("ID3"));
    // Verify original file is 100% untouched byte-for-byte
    assert_eq!(std::fs::read(&song).unwrap(), initial_bytes);
    // Verify no stray stage files were left behind
    assert_eq!(editor.list_backups(&song).len(), 0);

    // 2. Crash Recovery: Clean leftover temp staging files
    let stray_stage = root.join(".musicplayer-stage-1234.tmp");
    let stray_bak = root.join(".musicplayer-backup_stage-5678.tmp");
    std::fs::write(&stray_stage, b"stray-stage-data").unwrap();
    std::fs::write(&stray_bak, b"stray-backup-data").unwrap();
    assert!(stray_stage.exists());
    assert!(stray_bak.exists());
    clean_stale_temp_files(&root);
    assert!(!stray_stage.exists(), "stale stage file must be purged");
    assert!(!stray_bak.exists(), "stale backup stage file must be purged");

    // 3. Successful Edit and Byte-for-Byte Restoration
    let valid_edit = TagEdit {
        title: Some("Modified Title".to_string()),
        artist: Some("Modified Artist".to_string()),
        ..Default::default()
    };
    let backup_snapshot = editor.save(&song, &valid_edit).expect("Valid save must succeed");
    let modified_bytes = std::fs::read(&song).unwrap();
    assert_ne!(modified_bytes, initial_bytes, "Bytes must change after tag save");
    assert_eq!(editor.list_backups(&song).len(), 1);

    // Restore from backup snapshot
    editor.restore(&backup_snapshot).expect("Restore must succeed");
    let restored_bytes = std::fs::read(&song).unwrap();
    assert_eq!(
        restored_bytes, initial_bytes,
        "Restored file must be 100% byte-for-byte identical to the original prior to edit"
    );

    // 4. Fixed 5-backup Retention Cap
    for i in 1..=8 {
        let edit = TagEdit {
            title: Some(format!("Edit Version {}", i)),
            ..Default::default()
        };
        editor.save(&song, &edit).unwrap();
    }
    let retained = editor.list_backups(&song);
    assert_eq!(
        retained.len(),
        5,
        "Backup retention must cap strictly at 5 backups per track"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn test_tag_editor_read_only_file_handling() {
    let root = temp_dir("test_readonly_tags");
    std::fs::create_dir_all(&root).unwrap();
    let backups = root.join("backups");
    let song = root.join("readonly_track.mp3");

    let mut initial_bytes = Vec::new();
    initial_bytes.extend_from_slice(b"ID3\x04\x00\x00\x00\x00\x00\x00");
    initial_bytes.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
    initial_bytes.extend_from_slice(&[0; 1024]);
    std::fs::write(&song, &initial_bytes).unwrap();

    let mut perms = std::fs::metadata(&song).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&song, perms).unwrap();

    let editor = TagEditor::new(&backups);
    let edit = TagEdit {
        title: Some("Read Only Test".to_string()),
        ..Default::default()
    };
    let err = editor.save(&song, &edit).unwrap_err();
    assert!(
        err.to_string().contains("read-only"),
        "Error message must clearly state that file is marked read-only: {err}"
    );

    let mut perms = std::fs::metadata(&song).unwrap().permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&song, perms).unwrap();
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn test_deduplicate_verbatim_paths_merges_tracks_and_normalizes_state() {
    let dir = temp_dir("dedup_verbatim");
    let db_path = dir.join("library.db");
    let store = LibraryStore::open(&db_path).expect("open db");

    const CLEAN_A: &str = r"C:\Music\Iceman\SongA.mp3";
    const VERBATIM_A: &str = r"\\?\C:\Music\Iceman\SongA.mp3";
    const CLEAN_B: &str = r"C:\Music\Iceman\SongB.mp3";
    const VERBATIM_B: &str = r"\\?\C:\Music\Iceman\SongB.mp3";

    // Seed SongA via verbatim path first with interaction metadata we must not lose.
    let mut track_a_verbatim = sample_track(CLEAN_A, "Song A", "Artist One", "Iceman");
    track_a_verbatim.favorite = true;
    track_a_verbatim.rating = 5;
    track_a_verbatim.play_count = 4;
    track_a_verbatim.has_lyrics = true;
    track_a_verbatim.artwork_path = Some(r"\\?\C:\Cache\art_a.jpg".to_string());
    store.upsert_track(&track_a_verbatim).unwrap();
    store
        .write(move |conn| {
            conn.execute(
                "UPDATE tracks SET path = ?1 WHERE path = ?2",
                rusqlite::params![VERBATIM_A, CLEAN_A],
            )
        })
        .unwrap();

    // Seed clean duplicate of SongA with additional plays.
    let mut track_a_clean = sample_track(CLEAN_A, "Song A", "Artist One", "Iceman");
    track_a_clean.play_count = 3;
    store.upsert_track(&track_a_clean).unwrap();

    // Seed SongB solely via a verbatim path.
    let track_b = sample_track(CLEAN_B, "Song B", "Artist One", "Iceman");
    store.upsert_track(&track_b).unwrap();
    store
        .write(move |conn| {
            conn.execute(
                "UPDATE tracks SET path = ?1 WHERE path = ?2",
                rusqlite::params![VERBATIM_B, CLEAN_B],
            )
        })
        .unwrap();

    // Seed child references using verbatim paths.
    store.create_playlist("pl-1", "Test Playlist").unwrap();
    store
        .write(move |conn| {
            conn.execute(
                "INSERT INTO playlist_tracks (playlist_id, position, track_path) VALUES (?1, 0, ?2), (?1, 1, ?3)",
                rusqlite::params!["pl-1", VERBATIM_A, VERBATIM_B],
            )?;
            conn.execute(
                "INSERT INTO playback_queue (position, track_path) VALUES (0, ?1), (1, ?2)",
                rusqlite::params![VERBATIM_A, VERBATIM_B],
            )?;
            conn.execute(
                "INSERT INTO track_interaction_state (path, added_utc, favorite, rating, play_count, last_played_utc) VALUES (?1, '2026-01-01T00:00:00Z', 1, 5, 4, NULL)",
                rusqlite::params![VERBATIM_A],
            )?;
            Ok(())
        })
        .unwrap();

    let settings = music_player_core::PlayerSettings::new(store.clone());
    settings.set_library_roots(&[r"\\?\C:\Music".to_string(), r"C:\Music".to_string()]);
    settings.set_ignored_directories(&[r"\\?\C:\Music\Skipped".to_string()]);
    store
        .save_session(&music_player_core::PlaybackSession {
            track_path: Some(VERBATIM_A.to_string()),
            position_ms: 15_000,
            queue: vec![VERBATIM_A.to_string(), VERBATIM_B.to_string()],
            shuffle: false,
            repeat_mode: "None".to_string(),
            repeat_a_ms: None,
            repeat_b_ms: None,
            queue_index: 0,
        })
        .unwrap();

    let fixed = store
        .write(|conn| music_player_core::db::schema::deduplicate_verbatim_paths(conn))
        .unwrap();
    assert!(fixed > 0, "expected verbatim rows to be fixed");
    drop(store);

    // Reopen through LibraryStore::open (invokes initialize_schema automatically).
    let reopened = LibraryStore::open(&db_path).expect("reopen db");
    let all = reopened.get_all_tracks(TrackSort::Title, false).unwrap();
    assert_eq!(all.len(), 2, "duplicate verbatim track should merge cleanly");
    assert!(all.iter().all(|t| !t.path.starts_with(r"\\?\")));

    let merged_a = all.iter().find(|t| t.path == CLEAN_A).expect("CLEAN_A present");
    assert!(merged_a.favorite);
    assert_eq!(merged_a.rating, 5);
    assert_eq!(merged_a.play_count, 7);
    assert!(merged_a.has_lyrics);
    assert_eq!(
        merged_a.artwork_path.as_deref(),
        Some(r"\\?\C:\Cache\art_a.jpg")
    );

    let playlist_paths = reopened.get_playlist_paths("pl-1").unwrap();
    assert_eq!(playlist_paths, vec![CLEAN_A.to_string(), CLEAN_B.to_string()]);

    let queue_paths = reopened.load_queue().unwrap();
    assert_eq!(queue_paths, vec![CLEAN_A.to_string(), CLEAN_B.to_string()]);

    let session = reopened.load_session().unwrap().expect("session preserved");
    assert_eq!(session.track_path.as_deref(), Some(CLEAN_A));
    assert_eq!(session.queue, vec![CLEAN_A.to_string(), CLEAN_B.to_string()]);

    let reopened_settings = music_player_core::PlayerSettings::new(reopened.clone());
    assert_eq!(reopened_settings.library_roots(), vec![r"C:\Music".to_string()]);
    assert_eq!(
        reopened_settings.ignored_directories(),
        vec![r"C:\Music\Skipped".to_string()]
    );

    let hits = reopened.search_tracks("Iceman", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|t| !t.path.starts_with(r"\\?\")));

    assert_eq!(
        music_player_core::normalize_windows_path_str(r"\\?\C:\Music\Track.mp3"),
        r"C:\Music\Track.mp3"
    );
    assert_eq!(
        music_player_core::normalize_windows_path_str(r"\\?\UNC\server\share\Track.mp3"),
        r"\\server\share\Track.mp3"
    );
    assert_eq!(
        music_player_core::normalize_windows_path_str(r"C:\Music\Track.mp3"),
        r"C:\Music\Track.mp3"
    );

    drop(reopened);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_backup_directory_byte_cap_and_clear() {
    let dir = temp_dir("backup_cap");
    let files = [
        ("20260101-100000000-a.bak", vec![b'a'; 40]),
        ("20260102-100000000-b.bak", vec![b'b'; 40]),
        ("20260103-100000000-c.bak", vec![b'c'; 40]),
        ("20260104-100000000-d.bak", vec![b'd'; 40]),
    ];
    for (name, bytes) in &files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    std::fs::write(dir.join("notes.txt"), b"keep-me").unwrap();

    assert_eq!(music_player_core::backup_directory_stats(&dir), (4, 160));

    let evicted = music_player_core::prune_backup_directory_to_bytes(&dir, 90);
    assert_eq!(evicted, 2);
    assert_eq!(music_player_core::backup_directory_stats(&dir), (2, 80));
    assert!(!dir.join("20260101-100000000-a.bak").exists());
    assert!(!dir.join("20260102-100000000-b.bak").exists());
    assert!(dir.join("20260103-100000000-c.bak").exists());
    assert!(dir.join("20260104-100000000-d.bak").exists());
    assert!(dir.join("notes.txt").exists());

    // Even under a tiny byte budget, the single newest backup file stays retained.
    assert_eq!(music_player_core::prune_backup_directory_to_bytes(&dir, 10), 1);
    assert_eq!(music_player_core::backup_directory_stats(&dir), (1, 40));
    assert!(dir.join("20260104-100000000-d.bak").exists());

    assert_eq!(music_player_core::clear_backup_directory(&dir), 1);
    assert_eq!(music_player_core::backup_directory_stats(&dir), (0, 0));
    assert!(dir.join("notes.txt").exists());

    std::fs::remove_dir_all(&dir).ok();
}

