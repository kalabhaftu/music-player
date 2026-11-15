slint::include_modules!();

use music_player_core::lyrics::{has_sidecar, parse_lrc, read_sidecar, LyricsDocument};
use music_player_core::{
    extract_palette, AudioEngine, LibraryIndexer, LibraryStore, QueueCoordinator, RgbColor,
    ScannerControl, Track, TrackSort,
};
use slint::{Model, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

fn rgb_to_slint(rgb: RgbColor) -> slint::Color {
    slint::Color::from_argb_u8(255, rgb.r, rgb.g, rgb.b)
}

fn track_to_ui(track: &Track, id: i32) -> TrackItem {
    let cover_img = track
        .artwork_path
        .as_ref()
        .and_then(|p| slint::Image::load_from_path(Path::new(p)).ok())
        .unwrap_or_default();

    let accent_color = track
        .artwork_path
        .as_ref()
        .map(|p| rgb_to_slint(extract_palette(Path::new(p)).accent))
        .unwrap_or_else(|| slint::Color::from_argb_u8(255, 183, 255, 45));

    TrackItem {
        id,
        title: track.title.clone().into(),
        artist: track.artist.clone().into(),
        album: track.album.clone().into(),
        duration: track.duration_display().into(),
        cover: cover_img,
        is_favorite: track.favorite,
        accent: accent_color,
    }
}

fn user_music_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.audio_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            directories::UserDirs::new()
                .map(|u| u.home_dir().join("Music"))
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // Open the authoritative SQLite database from AppData
    let store = match LibraryStore::in_app_data() {
        Ok(s) => s,
        Err(_) => LibraryStore::in_memory().expect("Failed to initialize database"),
    };

    // Load real tracks from SQLite
    let mut db_tracks = store
        .get_all_tracks(TrackSort::Title, false)
        .unwrap_or_default();

    // If database is empty, scan the user's Music folder on startup
    if db_tracks.is_empty() {
        let music_dir = user_music_dir();
        if music_dir.exists() {
            let indexer = LibraryIndexer::new(&store);
            let control = Arc::new(ScannerControl::new());
            let _ = indexer.index_folder(&music_dir, control, |_progress| {});
            db_tracks = store
                .get_all_tracks(TrackSort::Title, false)
                .unwrap_or_default();
        }
    }

    // Audio Engine and Queue Coordinator
    let audio_engine = Rc::new(RefCell::new(AudioEngine::new()));
    let queue = Rc::new(RefCell::new(QueueCoordinator::new()));

    let queue_paths: Vec<String> = db_tracks.iter().map(|t| t.path.clone()).collect();
    queue.borrow_mut().replace(queue_paths, Some(0));

    // Convert db tracks to UI models
    let ui_tracks: Vec<TrackItem> = db_tracks
        .iter()
        .enumerate()
        .map(|(idx, t)| track_to_ui(t, (idx + 1) as i32))
        .collect();

    let tracks_model = Rc::new(VecModel::from(ui_tracks.clone()));
    ui.set_tracks(tracks_model.clone().into());

    let lyrics_model = Rc::new(VecModel::<LyricLineItem>::default());
    ui.set_lyrics_items(lyrics_model.clone().into());

    let current_lyrics: Rc<RefCell<Option<LyricsDocument>>> = Rc::new(RefCell::new(None));
    let last_active_lyric: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

    // Set initial playing track if available
    if let Some(first) = ui_tracks.first() {
        ui.set_current_track_id(first.id);
        ui.set_current_title(first.title.clone());
        ui.set_current_artist(first.artist.clone());
        ui.set_current_cover(first.cover.clone());
        ui.set_theme_accent(first.accent);
        ui.set_total_time(first.duration.clone());
        ui.set_current_time("0:00".into());
        ui.set_progress(0.0);
        ui.set_is_playing(false);

        // Check for lyrics for the first track
        if let Some(first_db) = db_tracks.first() {
            if has_sidecar(&first_db.path) {
                if let Some(lrc) = read_sidecar(Path::new(&first_db.path)) {
                    let doc = parse_lrc(&lrc);
                    let ui_lyr: Vec<LyricLineItem> = doc
                        .lines
                        .iter()
                        .map(|l| LyricLineItem {
                            text: l.text.clone().into(),
                            is_active: false,
                        })
                        .collect();
                    lyrics_model.set_vec(ui_lyr);
                    *current_lyrics.borrow_mut() = Some(doc);
                }
            }
        }
    }

    // Helper closure to play a track by index
    let play_track_at_index = {
        let ui_handle = ui.as_weak();
        let store_clone = store.clone();
        let db_tracks_clone = db_tracks.clone();
        let ui_tracks_clone = ui_tracks.clone();
        let audio_engine_clone = audio_engine.clone();
        let queue_clone = queue.clone();
        let current_lyrics_clone = current_lyrics.clone();
        let lyrics_model_clone = lyrics_model.clone();
        let last_active_lyric_clone = last_active_lyric.clone();

        Rc::new(move |track_idx: usize| {
            if let Some(track) = db_tracks_clone.get(track_idx) {
                if let Some(ui) = ui_handle.upgrade() {
                    let track_id = (track_idx + 1) as i32;
                    if let Some(item) = ui_tracks_clone.iter().find(|t| t.id == track_id) {
                        ui.set_current_track_id(item.id);
                        ui.set_current_title(item.title.clone());
                        ui.set_current_artist(item.artist.clone());
                        ui.set_current_cover(item.cover.clone());
                        ui.set_theme_accent(item.accent);
                        ui.set_total_time(item.duration.clone());
                        ui.set_current_time("0:00".into());
                        ui.set_progress(0.0);
                        ui.set_is_playing(true);

                        // Start audio playback
                        let _ = audio_engine_clone.borrow_mut().play_track(&track.path);

                        // Update queue position
                        queue_clone.borrow_mut().set_current_index(Some(track_idx));

                        // Record play in SQLite
                        let _ = store_clone.record_play(&track.path);

                        // Load lyrics if available
                        *last_active_lyric_clone.borrow_mut() = None;
                        if has_sidecar(&track.path) {
                            if let Some(lrc) = read_sidecar(Path::new(&track.path)) {
                                let doc = parse_lrc(&lrc);
                                let ui_lyr: Vec<LyricLineItem> = doc
                                    .lines
                                    .iter()
                                    .map(|l| LyricLineItem {
                                        text: l.text.clone().into(),
                                        is_active: false,
                                    })
                                    .collect();
                                lyrics_model_clone.set_vec(ui_lyr);
                                *current_lyrics_clone.borrow_mut() = Some(doc);
                            } else {
                                lyrics_model_clone.set_vec(Vec::new());
                                *current_lyrics_clone.borrow_mut() = None;
                            }
                        } else {
                            lyrics_model_clone.set_vec(Vec::new());
                            *current_lyrics_clone.borrow_mut() = None;
                        }
                    }
                }
            }
        })
    };

    // --- Wire User Interactions ---

    // 1. Select Track to Play
    {
        let play_track = play_track_at_index.clone();
        ui.on_track_selected(move |track_id| {
            if track_id > 0 {
                play_track((track_id - 1) as usize);
            }
        });
    }

    // 2. Play / Pause Toggle
    {
        let ui_handle = ui.as_weak();
        let audio_engine_clone = audio_engine.clone();
        ui.on_play_pause_clicked(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let is_playing = ui.get_is_playing();
                if is_playing {
                    let _ = audio_engine_clone.borrow_mut().pause();
                    ui.set_is_playing(false);
                } else {
                    let _ = audio_engine_clone.borrow_mut().resume();
                    ui.set_is_playing(true);
                }
            }
        });
    }

    // 3. Skip Forward (Next)
    {
        let queue_clone = queue.clone();
        let play_track = play_track_at_index.clone();
        ui.on_next_clicked(move || {
            let mut q = queue_clone.borrow_mut();
            if q.next(false).is_some() {
                if let Some(next_idx) = q.current_index() {
                    drop(q);
                    play_track(next_idx);
                }
            }
        });
    }

    // 4. Skip Backward (Previous)
    {
        let queue_clone = queue.clone();
        let play_track = play_track_at_index.clone();
        ui.on_previous_clicked(move || {
            let mut q = queue_clone.borrow_mut();
            if q.previous().is_some() {
                if let Some(prev_idx) = q.current_index() {
                    drop(q);
                    play_track(prev_idx);
                }
            }
        });
    }

    // 5. Volume Changed
    {
        let audio_engine_clone = audio_engine.clone();
        ui.on_volume_changed(move |vol| {
            let vol_u32 = (vol.clamp(0.0, 1.0) * 100.0) as u32;
            let _ = audio_engine_clone.borrow_mut().set_volume(vol_u32);
        });
    }

    // 6. Toggle Favorite (persisted to SQLite immediately!)
    {
        let store_clone = store.clone();
        let db_tracks_clone = db_tracks.clone();
        let tracks_model_clone = tracks_model.clone();
        ui.on_favorite_toggled(move |track_id| {
            for i in 0..tracks_model_clone.row_count() {
                if let Some(mut item) = tracks_model_clone.row_data(i) {
                    if item.id == track_id {
                        item.is_favorite = !item.is_favorite;
                        tracks_model_clone.set_row_data(i, item.clone());

                        // Persist to database
                        if let Some(track) = db_tracks_clone.get((track_id - 1) as usize) {
                            let _ = store_clone.set_favorite(&track.path, item.is_favorite);
                        }
                        break;
                    }
                }
            }
        });
    }

    // 7. Scrub / Seek
    {
        let audio_engine_clone = audio_engine.clone();
        let ui_handle = ui.as_weak();
        ui.on_seek_requested(move |new_progress| {
            if let Some(ui) = ui_handle.upgrade() {
                let duration_ms = audio_engine_clone.borrow().duration_ms();
                if duration_ms > 0 {
                    let target_ms = (new_progress as f64 * duration_ms as f64) as u64;
                    let _ = audio_engine_clone.borrow_mut().seek(target_ms);
                }
                ui.set_progress(new_progress);
            }
        });
    }

    // 8. FTS5 Search Filtering
    {
        let store_clone = store.clone();
        let tracks_model_clone = tracks_model.clone();
        ui.on_search_query_changed(move |query| {
            let query_str = query.trim();
            let filtered_tracks = if query_str.is_empty() {
                store_clone
                    .get_all_tracks(TrackSort::Title, false)
                    .unwrap_or_default()
            } else {
                store_clone
                    .search_tracks(query_str, 200)
                    .unwrap_or_default()
            };

            let ui_items: Vec<TrackItem> = filtered_tracks
                .iter()
                .enumerate()
                .map(|(idx, t)| track_to_ui(t, (idx + 1) as i32))
                .collect();

            tracks_model_clone.set_vec(ui_items);
        });
    }

    // 9. Sidebar Navigation switching
    {
        let store_clone = store.clone();
        let tracks_model_clone = tracks_model.clone();
        ui.on_nav_clicked(move |nav| {
            let nav_str = nav.as_str();
            let list = match nav_str {
                "Favorites" => store_clone.get_favorites().unwrap_or_default(),
                "Recently Added" => store_clone
                    .get_all_tracks(TrackSort::Added, true)
                    .unwrap_or_default(),
                _ => store_clone
                    .get_all_tracks(TrackSort::Title, false)
                    .unwrap_or_default(),
            };

            let ui_items: Vec<TrackItem> = list
                .iter()
                .enumerate()
                .map(|(idx, t)| track_to_ui(t, (idx + 1) as i32))
                .collect();

            tracks_model_clone.set_vec(ui_items);
        });
    }

    // 10. Drawer & Fullscreen callbacks
    ui.on_queue_toggle_clicked(|| {});
    ui.on_lyrics_toggle_clicked(|| {});
    ui.on_immersive_toggle_clicked(|| {});
    ui.on_immersive_close_clicked(|| {});

    // 11. Playback Timeline & Synced Lyrics Synchronization Timer (250ms polling)
    {
        let ui_handle = ui.as_weak();
        let audio_engine_clone = audio_engine.clone();
        let queue_clone = queue.clone();
        let play_track = play_track_at_index.clone();
        let current_lyrics_clone = current_lyrics.clone();
        let lyrics_model_clone = lyrics_model.clone();
        let last_active_lyric_clone = last_active_lyric.clone();

        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(250),
            move || {
                if let Some(ui) = ui_handle.upgrade() {
                    let engine = audio_engine_clone.borrow();
                    if engine.is_playing() {
                        let pos_ms = engine.position_ms();
                        let dur_ms = engine.duration_ms();

                        if dur_ms > 0 {
                            let p = (pos_ms as f32 / dur_ms as f32).clamp(0.0, 1.0);
                            ui.set_progress(p);

                            let pos_secs = pos_ms / 1000;
                            let mins = pos_secs / 60;
                            let secs = pos_secs % 60;
                            ui.set_current_time(format!("{}:{:02}", mins, secs).into());

                            // Update synchronized lyrics active line
                            if let Some(ref doc) = *current_lyrics_clone.borrow() {
                                if let Some(active_idx) = doc.active_index(pos_ms) {
                                    if *last_active_lyric_clone.borrow() != Some(active_idx) {
                                        *last_active_lyric_clone.borrow_mut() = Some(active_idx);
                                        for i in 0..lyrics_model_clone.row_count() {
                                            if let Some(mut item) = lyrics_model_clone.row_data(i) {
                                                let should_be = i == active_idx;
                                                if item.is_active != should_be {
                                                    item.is_active = should_be;
                                                    lyrics_model_clone.set_row_data(i, item);
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if track reached end (within 300ms of end)
                            if pos_ms + 300 >= dur_ms {
                                drop(engine);
                                let mut q = queue_clone.borrow_mut();
                                if q.next(true).is_some() {
                                    if let Some(next_idx) = q.current_index() {
                                        drop(q);
                                        play_track(next_idx);
                                    }
                                }
                            }
                        }
                    }
                }
            },
        );

        ui.run()
    }
}
