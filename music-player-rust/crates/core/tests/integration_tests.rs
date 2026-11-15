use music_player_core::{
    parse_lrc, plain_text, EqualizerState, LibraryStore, QueueCoordinator,
    RepeatMode, RgbColor, Track, TrackSort,
};
use chrono::Utc;

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
