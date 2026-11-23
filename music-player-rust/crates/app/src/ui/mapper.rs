use crate::native_io;
use crate::ui::colors::gradient_slot;
use crate::{
    GroupItem, GroupRowItem, HomeCardItem, HomeCardRow, HomeTrackRow, PlaylistCardItem,
    PlaylistRowItem, TrackItem,
};
use music_player_core::TrackSort;
use slint::{Image, SharedString, VecModel};
use std::path::Path;
use std::rc::Rc;

pub fn ss(s: impl Into<String>) -> SharedString {
    SharedString::from(s.into())
}

pub fn ui_model<T: Clone + 'static>(items: Vec<T>) -> Rc<VecModel<T>> {
    Rc::new(VecModel::from(items))
}

pub fn initials(value: &str) -> String {
    let s = value.trim();
    if s.is_empty() {
        return "MP".to_string();
    }
    let mut clean = String::with_capacity(s.len());
    let mut depth_paren = 0;
    let mut depth_bracket = 0;
    let mut depth_brace = 0;
    for ch in s.chars() {
        match ch {
            '(' => depth_paren += 1,
            ')' => {
                if depth_paren > 0 {
                    depth_paren -= 1;
                }
            }
            '[' => depth_bracket += 1,
            ']' => {
                if depth_bracket > 0 {
                    depth_bracket -= 1;
                }
            }
            '{' => depth_brace += 1,
            '}' => {
                if depth_brace > 0 {
                    depth_brace -= 1;
                }
            }
            _ => {
                if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 {
                    if ch.is_alphanumeric() || ch.is_whitespace() {
                        clean.push(ch);
                    } else {
                        clean.push(' ');
                    }
                }
            }
        }
    }
    let words: Vec<&str> = clean.split_whitespace().collect();
    if words.is_empty() {
        let fallback: String = s.chars().filter(|c| c.is_alphanumeric()).take(2).collect();
        return if fallback.is_empty() {
            "MP".to_string()
        } else {
            fallback.to_uppercase()
        };
    }
    if words.len() == 1 {
        let mut chars = words[0].chars();
        let first = chars.next().unwrap_or('M');
        let second = chars.next();
        let mut res = String::new();
        res.extend(first.to_uppercase());
        if let Some(c) = second {
            res.extend(c.to_uppercase());
        }
        res
    } else {
        let c1 = words[0].chars().next().unwrap_or('M');
        let c2 = words[1].chars().next().unwrap_or('P');
        let mut res = String::new();
        res.extend(c1.to_uppercase());
        res.extend(c2.to_uppercase());
        res
    }
}

pub fn song_sort(name: &str, default: TrackSort) -> TrackSort {
    match name {
        "Title" => TrackSort::Title,
        "Artist" => TrackSort::Artist,
        "Album" => TrackSort::Album,
        "Genre" => TrackSort::Genre,
        "Year" => TrackSort::Year,
        "Added" => TrackSort::Added,
        "Duration" => TrackSort::Duration,
        "PlayCount" | "Plays" => TrackSort::PlayCount,
        "LastPlayed" => TrackSort::LastPlayed,
        "TrackNumber" | "Track" => TrackSort::TrackNumber,
        "Type" => TrackSort::Type,
        "Rating" => TrackSort::Rating,
        "Path" => TrackSort::Path,
        _ => default,
    }
}

pub fn default_sort_for_view(view: &str) -> (String, bool) {
    match view {
        "Most Played" => ("PlayCount".into(), true),
        "Recently Played" => ("LastPlayed".into(), true),
        "Recently Added" => ("Added".into(), true),
        _ => ("Title".into(), false),
    }
}

pub fn detail_sort_for_column(column: &str) -> (String, bool) {
    match column {
        "album" => ("TrackNumber".into(), false),
        "artist" => ("PlayCount".into(), true),
        _ => ("Title".into(), false),
    }
}

pub fn group_to_ui(group: &music_player_core::LibraryGroup) -> GroupItem {
    let is_folder = group.name.contains('\\') || group.name.contains('/');
    let display_name = if is_folder {
        let trimmed = group.name.trim_end_matches(['\\', '/']);
        Path::new(trimmed)
            .file_name()
            .and_then(|f| f.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&group.name)
            .to_string()
    } else {
        group.name.clone()
    };
    let monogram_name = if is_folder {
        &display_name
    } else {
        &group.name
    };
    let slot = gradient_slot(monogram_name);
    let subtitle = if let Some(artist) = &group.artist {
        format!(
            "{} · {} song{}",
            artist,
            group.track_count,
            if group.track_count == 1 { "" } else { "s" }
        )
    } else if group.year > 0 {
        format!(
            "{} song{} · {}",
            group.track_count,
            if group.track_count == 1 { "" } else { "s" },
            group.year
        )
    } else {
        format!(
            "{} song{}",
            group.track_count,
            if group.track_count == 1 { "" } else { "s" }
        )
    };
    let (has_cover, cover) = if let Some(art) = &group.artwork_path {
        if let Some(img) = native_io::artwork_preview(Path::new(art)) {
            (true, img)
        } else {
            (false, Image::default())
        }
    } else {
        (false, Image::default())
    };
    GroupItem {
        name: ss(&group.name),
        display_name: ss(&display_name),
        subtitle: ss(subtitle),
        initials: ss(initials(monogram_name)),
        gradient: slot,
        track_count: group.track_count as i32,
        has_cover,
        cover,
    }
}

pub fn chunk_groups(groups: Vec<GroupItem>) -> Vec<GroupRowItem> {
    let mut rows = Vec::new();
    let mut iter = groups.into_iter();
    loop {
        let a = iter.next();
        let b = iter.next();
        let c = iter.next();
        let filled = [&a, &b, &c].iter().filter(|x| x.is_some()).count() as i32;
        if filled == 0 {
            break;
        }
        let empty = GroupItem {
            name: ss(""),
            display_name: ss(""),
            subtitle: ss(""),
            initials: ss(""),
            gradient: 1,
            track_count: 0,
            has_cover: false,
            cover: Image::default(),
        };
        rows.push(GroupRowItem {
            a: a.unwrap_or_else(|| empty.clone()),
            b: b.unwrap_or_else(|| empty.clone()),
            c: c.unwrap_or(empty),
            filled,
        });
    }
    rows
}

pub fn chunk_home_cards(cards: Vec<HomeCardItem>, columns: usize) -> Vec<HomeCardRow> {
    let mut rows = Vec::new();
    let mut iter = cards.into_iter();
    let columns = columns.clamp(1, 5);
    loop {
        let Some(a) = iter.next() else {
            break;
        };
        let b = if columns >= 2 { iter.next() } else { None };
        let c = if columns >= 3 { iter.next() } else { None };
        let d = if columns >= 4 { iter.next() } else { None };
        let e = if columns >= 5 { iter.next() } else { None };
        let filled = 1 + [&b, &c, &d, &e]
            .iter()
            .filter(|item| item.is_some())
            .count() as i32;
        let spare = a.clone();
        rows.push(HomeCardRow {
            a,
            b: b.unwrap_or_else(|| spare.clone()),
            c: c.unwrap_or_else(|| spare.clone()),
            d: d.unwrap_or_else(|| spare.clone()),
            e: e.unwrap_or(spare),
            filled,
        });
    }
    rows
}

pub fn chunk_home_tracks(tracks: Vec<TrackItem>) -> Vec<HomeTrackRow> {
    let mut rows = Vec::new();
    let mut iter = tracks.into_iter();
    loop {
        let Some(a) = iter.next() else {
            break;
        };
        let b = iter.next();
        let filled = if b.is_some() { 2 } else { 1 };
        let spare = a.clone();
        rows.push(HomeTrackRow {
            a,
            b: b.unwrap_or(spare),
            filled,
        });
    }
    rows
}

pub fn chunk_playlists(cards: Vec<PlaylistCardItem>) -> Vec<PlaylistRowItem> {
    let mut rows = Vec::new();
    let mut iter = cards.into_iter();
    loop {
        let a = iter.next();
        let b = iter.next();
        let c = iter.next();
        let filled = [&a, &b, &c].iter().filter(|x| x.is_some()).count() as i32;
        if filled == 0 {
            break;
        }
        let empty = PlaylistCardItem {
            id: ss(""),
            name: ss(""),
            track_count: 0,
            is_smart: false,
            gradient: 1,
            initials: ss(""),
            description: ss(""),
        };
        rows.push(PlaylistRowItem {
            a: a.unwrap_or_else(|| empty.clone()),
            b: b.unwrap_or_else(|| empty.clone()),
            c: c.unwrap_or(empty),
            filled,
        });
    }
    rows
}
