slint::include_modules!();

use music_player_core::lyrics::{has_sidecar, parse_lrc, read_sidecar, LyricsDocument};
use music_player_core::{
    extract_palette, AudioEngine, LibraryIndexer, LibraryStore, QueueCoordinator, RgbColor,
    ScannerControl, Track, TrackSort,
};
use slint::{Model, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

fn rgb_to_slint(rgb: RgbColor) -> slint::Color {
    slint::Color::from_argb_u8(255, rgb.r, rgb.g, rgb.b)
}

fn load_artwork_safe(path: &Path) -> Option<slint::Image> {
    if !path.exists() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let dynamic_img = image::load_from_memory(&bytes).ok()?;
    let rgba = dynamic_img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixel_buffer = SharedPixelBuffer::clone_from_slice(rgba.as_raw(), width, height);
    Some(slint::Image::from_rgba8(pixel_buffer))
}

fn track_initials(track: &Track) -> String {
    let val = if !track.album.is_empty() {
        &track.album
    } else if !track.artist.is_empty() {
        &track.artist
    } else {
        &track.title
    };
    let words: Vec<&str> = val.split_whitespace().collect();
    if words.is_empty() {
        return "MP".to_string();
    }
    if words.len() == 1 {
        words[0].chars().take(2).collect::<String>().to_uppercase()
    } else {
        format!(
            "{}{}",
            words[0].chars().next().unwrap_or('M'),
            words[1].chars().next().unwrap_or('P')
        )
        .to_uppercase()
    }
}

fn track_badge_color(track: &Track) -> slint::Color {
    let seed = if !track.album.is_empty() {
        format!("{}:{}", track.album_artist, track.album)
    } else if !track.artist.is_empty() {
        track.artist.clone()
    } else {
        track.title.clone()
    };
    let mut hash: u32 = 2166136261;
    for b in seed.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let colors = [
        slint::Color::from_rgb_u8(42, 47, 63), // c1
        slint::Color::from_rgb_u8(33, 51, 55), // c2
        slint::Color::from_rgb_u8(56, 39, 43), // c3
        slint::Color::from_rgb_u8(36, 51, 40), // c4
        slint::Color::from_rgb_u8(53, 48, 37), // c5
        slint::Color::from_rgb_u8(42, 42, 50), // c6
    ];
    colors[(hash % 6) as usize]
}

fn track_to_ui(track: &Track, id: i32) -> TrackItem {
    TrackItem {
        id,
        title: track.title.clone().into(),
        artist: track.artist.clone().into(),
        album: track.album.clone().into(),
        duration: track.duration_display().into(),
        initials: track_initials(track).into(),
        badge_bg: track_badge_color(track),
        is_favorite: track.favorite,
        accent: slint::Color::from_argb_u8(255, 183, 255, 45),
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

    // Convert db tracks to lightweight UI models
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

    // Set initial track metadata and load its artwork
    if let Some(first_db) = db_tracks.first() {
        ui.set_current_track_id(1);
        ui.set_current_title(first_db.title.clone().into());
        ui.set_current_artist(first_db.artist.clone().into());
        ui.set_total_time(first_db.duration_display().into());
        ui.set_current_time("0:00".into());
        ui.set_progress(0.0);
        ui.set_is_playing(false);

        // Load artwork safely for the initial track
        if let Some(ref art_path) = first_db.artwork_path {
            let p = Path::new(art_path);
            if let Some(img) = load_artwork_safe(p) {
                ui.set_current_cover(img);
                let palette = extract_palette(p);
                ui.set_theme_accent(rgb_to_slint(palette.accent));
            }
        }

        // Check for sidecar lyrics
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

    // Helper closure to play a track by index
    let play_track_at_index = {
        let ui_handle = ui.as_weak();
        let store_clone = store.clone();
        let db_tracks_clone = db_tracks.clone();
        let audio_engine_clone = audio_engine.clone();
        let queue_clone = queue.clone();
        let current_lyrics_clone = current_lyrics.clone();
        let lyrics_model_clone = lyrics_model.clone();
        let last_active_lyric_clone = last_active_lyric.clone();

        Rc::new(move |track_idx: usize| {
            if let Some(track) = db_tracks_clone.get(track_idx) {
                if let Some(ui) = ui_handle.upgrade() {
                    let track_id = (track_idx + 1) as i32;
                    ui.set_current_track_id(track_id);
                    ui.set_current_title(track.title.clone().into());
                    ui.set_current_artist(track.artist.clone().into());
                    ui.set_total_time(track.duration_display().into());
                    ui.set_current_time("0:00".into());
                    ui.set_progress(0.0);
                    ui.set_is_playing(true);

                    // Load artwork safely for playing track
                    if let Some(ref art_path) = track.artwork_path {
                        let p = Path::new(art_path);
                        if let Some(img) = load_artwork_safe(p) {
                            ui.set_current_cover(img);
                            let palette = extract_palette(p);
                            ui.set_theme_accent(rgb_to_slint(palette.accent));
                        } else {
                            ui.set_current_cover(slint::Image::default());
                        }
                    } else {
                        ui.set_current_cover(slint::Image::default());
                    }

                    // Start hardware audio playback
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

    // 11. Playback Timeline & Synced Lyrics Synchronization Timer (200ms polling)
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
            std::time::Duration::from_millis(200),
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

                            let dur_secs = dur_ms / 1000;
                            let d_mins = dur_secs / 60;
                            let d_secs = dur_secs % 60;
                            ui.set_total_time(format!("{}:{:02}", d_mins, d_secs).into());

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
                            if pos_ms > 0 && pos_ms + 300 >= dur_ms {
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
