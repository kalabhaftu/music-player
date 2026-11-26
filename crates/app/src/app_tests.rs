use crate::controllers::query_dispatcher::QueryDispatcher;
use crate::controllers::views::fmt_right_time;
use crate::native_io::{
    available_disk_space_bytes, bump_artwork_generation, extract_artwork_palette,
    load_artwork_image, poll_artwork_images, artwork_preview,
};
use crate::updates::ReleaseVersion;
use std::path::Path;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateComparison {
    NewerRelease,
    CurrentRelease,
    NoEligibleRelease,
    UnknownVersion,
}

fn compare_versions(current: &str, latest: Option<&str>) -> UpdateComparison {
    let Some(installed) = ReleaseVersion::parse(current) else {
        return UpdateComparison::UnknownVersion;
    };
    let Some(available) = latest.and_then(ReleaseVersion::parse) else {
        return UpdateComparison::NoEligibleRelease;
    };
    if available > installed {
        UpdateComparison::NewerRelease
    } else {
        UpdateComparison::CurrentRelease
    }
}

#[test]
fn test_compare_versions_newer() {
    assert_eq!(
        compare_versions("1.0.0", Some("1.0.1")),
        UpdateComparison::NewerRelease
    );
    assert_eq!(
        compare_versions("1.0.0", Some("2.0.0")),
        UpdateComparison::NewerRelease
    );
    assert_eq!(
        compare_versions("1.0.0-preview.1", Some("1.0.0")),
        UpdateComparison::NewerRelease
    );
}

#[test]
fn test_compare_versions_current_or_older() {
    assert_eq!(
        compare_versions("1.0.0", Some("1.0.0")),
        UpdateComparison::CurrentRelease
    );
    assert_eq!(
        compare_versions("1.1.0", Some("1.0.0")),
        UpdateComparison::CurrentRelease
    );
}

#[test]
fn test_compare_versions_no_eligible_or_invalid() {
    assert_eq!(
        compare_versions("1.0.0", None),
        UpdateComparison::NoEligibleRelease
    );
    assert_eq!(
        compare_versions("1.0.0", Some("invalid-version")),
        UpdateComparison::NoEligibleRelease
    );
    assert_eq!(
        compare_versions("invalid-installed", Some("1.0.0")),
        UpdateComparison::UnknownVersion
    );
}

#[test]
fn test_extract_artwork_palette_nonexistent() {
    assert!(extract_artwork_palette(Path::new("nonexistent_test_art.jpg")).is_none());
}

#[test]
fn test_available_disk_space_bytes() {
    let temp = std::env::temp_dir();
    let space = available_disk_space_bytes(&temp);
    #[cfg(windows)]
    {
        assert!(space.is_some(), "Disk space query should succeed on Windows");
        assert!(space.unwrap() > 0, "Available disk space must be greater than 0");
    }
}

#[test]
fn test_artwork_loader_bounded_pool_and_generation() {
    let gen1 = bump_artwork_generation();
    let gen2 = bump_artwork_generation();
    assert!(gen2 > gen1);

    let temp_dir = std::env::temp_dir().join(format!("art_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_art = temp_dir.join("test_cover.png");

    let img = image::RgbaImage::from_fn(100, 100, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
    });
    img.save_with_format(&test_art, image::ImageFormat::Png).unwrap();

    let first = artwork_preview(&test_art);
    assert!(first.is_none());

    let second = artwork_preview(&test_art);
    assert!(second.is_none());

    std::thread::sleep(std::time::Duration::from_millis(150));

    let polled = poll_artwork_images();
    assert!(polled.contains(&test_art));

    let third = artwork_preview(&test_art);
    assert!(third.is_some(), "Artwork should be cached and available");

    let detail = load_artwork_image(&test_art);
    assert!(detail.is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dispatcher_lifecycle_and_superseding() {
    let dispatcher = QueryDispatcher::new();
    assert_eq!(dispatcher.latest_generation.load(Ordering::SeqCst), 0);

    dispatcher.cancel_below(5);
    assert_eq!(dispatcher.latest_generation.load(Ordering::SeqCst), 5);

    dispatcher.cancel_below(10);
    assert_eq!(dispatcher.latest_generation.load(Ordering::SeqCst), 10);
}

#[test]
fn formats_total_and_remaining_time() {
    assert_eq!(fmt_right_time(38_000, 225_000, false), "3:45");
    assert_eq!(fmt_right_time(38_000, 225_000, true), "-3:07");
    assert_eq!(fmt_right_time(250_000, 225_000, true), "-0:00");
    assert_eq!(fmt_right_time(10_000, 0, true), "0:00");
}

#[test]
fn test_setting_matches_query() {
    use crate::ui::callbacks::navigation::setting_matches_query;
    assert!(!setting_matches_query("Theme appearance dark", ""));
    assert!(!setting_matches_query("Theme appearance dark", "   "));
    assert!(setting_matches_query("Theme dark mode", "DARK theme"));
    assert!(setting_matches_query(
        "Crossfade blend into the next track",
        "crossfade track"
    ));
    assert!(!setting_matches_query(
        "Tag backups size cap cleanup",
        "tag lyrics"
    ));
}

#[test]
fn test_player_settings_persistence_and_backup_cap_clamping() {
    let store = music_player_core::LibraryStore::in_memory().expect("in-memory db");
    let settings = music_player_core::PlayerSettings::new(store.clone());

    assert_eq!(
        settings.update_web_setting("theme", &serde_json::json!("Dark")),
        Ok(music_player_core::settings::keys::THEME.to_string())
    );
    assert_eq!(
        settings.raw(music_player_core::settings::keys::THEME).as_deref(),
        Some("Dark")
    );

    assert_eq!(
        settings.update_web_setting("tagBackupMaxMb", &serde_json::json!("250 MB")),
        Ok(music_player_core::settings::keys::TAG_BACKUP_MAX_MB.to_string())
    );
    assert_eq!(settings.tag_backup_max_mb(), 250);

    settings.set_raw(music_player_core::settings::keys::TAG_BACKUP_MAX_MB, "5");
    assert_eq!(settings.tag_backup_max_mb(), 25);

    settings.set_raw(music_player_core::settings::keys::TAG_BACKUP_MAX_MB, "9999");
    assert_eq!(settings.tag_backup_max_mb(), 2000);

    settings.set_raw(music_player_core::settings::keys::CROSSFADE_SECONDS, "5");
    assert_eq!(settings.crossfade_seconds(), 5);
    settings.set_raw(music_player_core::settings::keys::CROSSFADE_SECONDS, "6");
    assert_eq!(settings.crossfade_seconds(), 0);
}

#[test]
fn test_queue_select_and_shuffle_preserves_current_track() {
    let mut queue = music_player_core::QueueCoordinator::new();
    let paths = vec![
        r"C:\Music\Alb\01.mp3".to_string(),
        r"C:\Music\Alb\02.mp3".to_string(),
        r"C:\Music\Alb\03.mp3".to_string(),
        r"C:\Music\Alb\04.mp3".to_string(),
    ];

    // Normal context selection retains full album order and sets current_index to clicked track.
    queue.select_track(paths.clone(), r"C:\Music\Alb\03.mp3");
    assert_eq!(queue.entries(), paths.as_slice());
    assert_eq!(queue.current_index(), Some(2));
    assert!(!queue.move_entry_to(2, 0));

    // Upcoming entries (>= current_index + 1) can be reordered.
    queue.replace(paths.clone(), Some(0));
    assert!(queue.move_entry_to(2, 1));
    assert_eq!(queue.entries()[1], r"C:\Music\Alb\03.mp3");

    // Shuffled context selection puts clicked track at index 0 and shuffles remaining tracks.
    let mut reordered = paths.clone();
    let pos = reordered
        .iter()
        .position(|p| p.eq_ignore_ascii_case(r"C:\Music\Alb\03.mp3"))
        .unwrap();
    let clicked = reordered.remove(pos);
    reordered.insert(0, clicked);
    queue.set_shuffle(true);
    queue.select_track(reordered, r"C:\Music\Alb\03.mp3");
    assert_eq!(queue.current_index(), Some(0));
    assert_eq!(queue.entries()[0], r"C:\Music\Alb\03.mp3");
    assert_eq!(queue.entries().len(), 4);
}
