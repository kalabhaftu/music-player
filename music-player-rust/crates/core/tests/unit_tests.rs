use music_player_core::audio::equalizer::{
    canonical_preset_by_name, canonical_preset_names, EqualizerState, EQUALIZER_BANDS,
    EQUALIZER_BAND_LABELS,
};
use music_player_core::audio::listening::PlayCompletion;
use music_player_core::audio::volume::{
    amplitude_to_perceived_loudness, linear_to_perceived_volume, perceived_loudness_to_amplitude,
    perceived_to_linear_volume, quadratic_volume_curve,
};
use music_player_core::audio::{AudioEngine, CrossfadeOutcome};
use music_player_core::lyrics::lrclib::{
    is_censored_lyrics, lyric_match_score, normalize_query, normalized_words, primary_artist_name,
    strip_featured_and_edition, LyricsSearchResult,
};
use music_player_core::settings::{is_hex_color, keys};
use music_player_core::{LibraryStore, PlayerSettings};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn write_test_wav(path: &Path, duration_ms: u32) {
    let sample_rate: u32 = 44100;
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let num_samples = (sample_rate * duration_ms) / 1000;
    let data_bytes = num_samples * (channels as u32) * (bits_per_sample as u32 / 8);
    let mut buf = Vec::with_capacity(44 + data_bytes as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32 / 8);
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * (bits_per_sample / 8);
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    buf.resize(44 + data_bytes as usize, 0);
    std::fs::write(path, &buf).unwrap();
}

#[test]
fn test_quadratic_curve_boundary_values() {
    assert_eq!(quadratic_volume_curve(0.0), 0.0);
    assert_eq!(quadratic_volume_curve(100.0), 1.0);
    assert!((quadratic_volume_curve(50.0) - 0.25).abs() < 1e-6);
    assert!((quadratic_volume_curve(25.0) - 0.0625).abs() < 1e-6);
}

#[test]
fn test_volume_clamping() {
    assert_eq!(quadratic_volume_curve(-10.0), 0.0);
    assert_eq!(quadratic_volume_curve(150.0), 1.0);
}

#[test]
fn test_volume_round_trip() {
    for v in [0.0, 10.0, 25.0, 50.0, 75.0, 90.0, 100.0] {
        let amp = perceived_loudness_to_amplitude(v);
        let recovered = amplitude_to_perceived_loudness(amp);
        assert!((recovered - v).abs() < 1e-5);
    }
}

#[test]
fn test_volume_integer_mapping() {
    assert_eq!(perceived_to_linear_volume(0), 0);
    assert_eq!(perceived_to_linear_volume(100), 100);
    assert_eq!(perceived_to_linear_volume(50), 25);
    assert_eq!(linear_to_perceived_volume(25), 50);
}

#[test]
fn half_threshold_matches_csharp_ticks_math() {
    assert!(!PlayCompletion::has_reached_half(0, 90_000));
    assert!(!PlayCompletion::has_reached_half(-5, 90_000));
    assert!(!PlayCompletion::has_reached_half(3, 1));
    assert!(PlayCompletion::has_reached_half(3, 2));
    assert!(PlayCompletion::has_reached_half(180_000, 90_000));
    assert!(!PlayCompletion::has_reached_half(180_000, 89_999));
}

#[test]
fn test_all_20_canonical_presets_exist() {
    let names = canonical_preset_names();
    assert_eq!(names.len(), 20);
    for &name in names {
        let preset = canonical_preset_by_name(name);
        assert!(preset.is_some(), "Missing preset: {}", name);
        assert_eq!(preset.unwrap().len(), 10);
    }
}

#[test]
fn test_exact_gain_values() {
    assert_eq!(
        canonical_preset_by_name("Acoustic").unwrap(),
        [5.0, 5.0, 4.0, 1.0, 1.5, 1.5, 3.5, 4.0, 3.5, 2.0]
    );
    assert_eq!(
        canonical_preset_by_name("Bass Boost").unwrap(),
        [6.0, 5.0, 4.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(
        canonical_preset_by_name("Bass Reducer").unwrap(),
        [-5.5, -4.5, -4.0, -2.5, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(
        canonical_preset_by_name("Rock").unwrap(),
        [4.5, 3.0, 2.0, 0.0, -1.0, -0.5, 1.5, 3.0, 4.0, 4.5]
    );
    assert_eq!(
        canonical_preset_by_name("Vocal Boost").unwrap(),
        [-1.5, -2.0, -1.0, 1.0, 3.5, 3.5, 2.0, 1.0, 0.0, -1.0]
    );
    assert_eq!(
        canonical_preset_by_name("Small Speakers").unwrap(),
        [5.5, 4.0, 3.5, 2.5, 1.0, 0.0, -1.0, -2.0, -3.5, -4.0]
    );
    assert_eq!(
        canonical_preset_by_name("Spoken Word").unwrap(),
        [-3.5, -0.5, 0.0, 0.5, 3.5, 4.5, 5.0, 4.5, 2.5, 0.0]
    );
}

#[test]
fn test_preamp_clamping() {
    let mut state = EqualizerState::default();
    state.set_preamp(-25.0);
    assert_eq!(state.preamp_db, -20.0);
    state.set_preamp(35.0);
    assert_eq!(state.preamp_db, 20.0);
    state.set_preamp(12.0412);
    assert_eq!(state.preamp_db, 12.0412);
}

#[test]
fn test_band_frequencies() {
    assert_eq!(EQUALIZER_BANDS, [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]);
    assert_eq!(
        EQUALIZER_BAND_LABELS,
        ["32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"]
    );
}

#[test]
fn test_is_hex_color_validation() {
    assert!(is_hex_color("#b7ff2d"));
    assert!(is_hex_color("#59D7FF"));
    assert!(is_hex_color("59d7ff"));
    assert!(!is_hex_color("#xyz123"));
    assert!(!is_hex_color(""));
    assert!(!is_hex_color("#12345"));
    assert!(!is_hex_color("#1234567"));
}

#[test]
fn test_update_web_setting_numeric_and_hex() {
    let store = LibraryStore::in_memory().unwrap();
    let settings = PlayerSettings::new(store);

    assert!(settings.update_web_setting("navigationWidth", &json!(250)).is_ok());
    assert_eq!(settings.int_or(keys::NAVIGATION_WIDTH, 0), 250);

    assert!(settings.update_web_setting("navigationWidth", &json!(280.0)).is_ok());
    assert_eq!(settings.int_or(keys::NAVIGATION_WIDTH, 0), 280);

    assert!(settings.update_web_setting("browseWidth", &json!("320.0")).is_ok());
    assert_eq!(settings.int_or(keys::BROWSE_WIDTH, 0), 320);

    assert!(settings.update_web_setting("windowTransparency", &json!(35)).is_ok());
    assert_eq!(settings.int_or(keys::WINDOW_TRANSPARENCY, 0), 35);

    assert!(settings.update_web_setting("accentColor", &json!("59d7ff")).is_ok());
    assert_eq!(settings.raw(keys::ACCENT_COLOR).as_deref(), Some("#59d7ff"));
}

#[test]
fn test_censored_lyrics_detection() {
    assert!(is_censored_lyrics("What the f*** is going on"));
    assert!(is_censored_lyrics("You little b****"));
    assert!(is_censored_lyrics("They are all **** here"));
    assert!(is_censored_lyrics("Look at that f---ing car"));
    assert!(!is_censored_lyrics("What the fuck is going on"));
    assert!(!is_censored_lyrics("You little bitch"));
    assert!(!is_censored_lyrics("Clean lyrics without any curses"));
}

#[test]
fn test_normalize_and_primary_artist_and_match() {
    assert_eq!(
        strip_featured_and_edition(
            &normalize_query(
                "3500 (feat. Future & 2 Chainz) [Official Audio]",
                "Travis Scott; Future"
            )
            .0
        ),
        "3500".to_string()
    );
    assert_eq!(primary_artist_name("50 Cent; Sonny Digital"), "50 Cent");
    assert_eq!(primary_artist_name("GloRilla/Sexyy Red"), "GloRilla");
    assert_eq!(
        normalized_words("I\u{2019}m The Man (Caf\u{00e9})"),
        vec!["im", "the", "man", "cafe"]
    );

    let candidate = LyricsSearchResult {
        id: 1,
        track_name: "I\u{2019}m the Man".to_string(),
        artist_name: "50 Cent".to_string(),
        album_name: None,
        duration: 210.0,
        instrumental: false,
        plain_lyrics: Some("Some words".to_string()),
        synced_lyrics: Some("[00:10.00] Some words".to_string()),
    };
    let score = lyric_match_score(
        "I'm The Man (Explicit)",
        "50 Cent; Sonny Digital",
        210.0,
        &candidate,
    );
    assert!(score > 0.8, "Expected strong match score, got {}", score);
}

#[test]
fn test_vlc_initializes() {
    let engine = AudioEngine::new();
    assert!(
        engine.vlc_is_available(),
        "AudioEngine must successfully initialize LibVLC"
    );
}

#[test]
fn test_vlc_runtime_eq_and_dual_volume() {
    let mut engine = AudioEngine::new();
    assert!(engine.vlc_is_available());

    let club = engine.preset_curve("Club");
    assert!(club.is_some(), "VLC Club preset should resolve bands and preamp");
    let (club_bands, _club_preamp) = club.unwrap();
    assert!(club_bands.iter().any(|&v| v != 0.0));

    let temp_dir = std::env::temp_dir();
    let wav1 = temp_dir.join("mp_test_vlc_1.wav");
    let wav2 = temp_dir.join("mp_test_vlc_2.wav");
    write_test_wav(&wav1, 4000);
    write_test_wav(&wav2, 4000);

    engine.set_volume(75).unwrap();
    engine.apply_equalizer(
        Some("Rock"),
        Some([4.5, 3.0, 2.0, 0.0, -1.0, -0.5, 1.5, 3.0, 4.0, 4.5]),
        12.0,
    );
    engine.play_track(&wav1).unwrap();

    std::thread::sleep(Duration::from_millis(250));
    assert!((engine.perceived_volume() - 0.5625).abs() < 1e-6);
    assert!(engine.is_playing() || engine.position_ms() >= 0);

    let out = engine.begin_crossfade(&wav2.to_string_lossy(), 800);
    assert!(
        !matches!(out, CrossfadeOutcome::Failed(_)),
        "Unexpected crossfade failure: {:?}",
        out
    );

    let _ = engine.stop();
    let _ = std::fs::remove_file(&wav1);
    let _ = std::fs::remove_file(&wav2);
}
