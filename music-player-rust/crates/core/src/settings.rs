//! Player settings facade — port of `PlayerSettingsService.cs`.
//!
//! All settings persist in the SQLite `settings` table under the exact store
//! keys used by the original app, so a Rust install reads the same library.db.

use crate::db::LibraryStore;
use serde_json::{json, Value};

pub mod keys {
    // Web-editable keys (web name -> store key is 1:1 below).
    pub const THEME: &str = "theme";
    pub const ACCENT_MODE: &str = "accent-mode";
    pub const ACCENT_MANUAL: &str = "accent-manual";
    pub const ACCENT_COLOR: &str = "accent-color";
    pub const SELECTION_COLOR_MODE: &str = "selection-color-mode";
    pub const SELECTION_COLOR: &str = "selection-color";
    pub const TRANSPARENT_WINDOW: &str = "transparent-window";
    pub const WINDOW_TRANSPARENCY: &str = "window-transparency";
    pub const MOTION_STYLE: &str = "motion-style";
    pub const HIDE_DUPLICATES: &str = "hide-exact-duplicates";
    pub const AUTOPLAY_SIMILAR: &str = "autoplay-similar";
    pub const AUTO_OPEN_PANEL: &str = "auto-open-side-panel";
    pub const AUTO_LOAD_LYRICS: &str = "auto-load-lyrics";
    pub const SEEK_FROM_LYRICS: &str = "seek-from-lyrics";
    pub const SHOW_ARTWORK: &str = "show-list-artwork";
    pub const SHOW_ARTIST: &str = "show-list-artist";
    pub const SHOW_ALBUM: &str = "show-list-album";
    pub const SHOW_ADDED: &str = "show-list-added";
    pub const SHOW_YEAR: &str = "show-list-year";
    pub const SHOW_DURATION: &str = "show-list-duration";
    pub const SHOW_FAVORITE: &str = "show-list-favorite";
    pub const CHECK_UPDATES: &str = "check-updates";
    pub const MINIMIZE_TO_TRAY: &str = "minimize-to-tray";
    pub const NAVIGATION_WIDTH: &str = "navigation-pane-width";
    pub const BROWSE_WIDTH: &str = "browse-pane-width";
    pub const NAVIGATION_ORDER: &str = "navigation-order";
    pub const HIDDEN_PANELS: &str = "hidden-panels";
    pub const RIGHT_SIDEBAR_MODE: &str = "right-sidebar-mode";
    pub const RIGHT_PANEL_OPEN: &str = "right-panel-open";
    pub const RIGHT_PANEL_WIDTH: &str = "right-panel-width";
    pub const SIDEBAR_COLLAPSED: &str = "sidebar-collapsed";
    pub const SONG_COLUMN_WIDTHS: &str = "song-column-widths";

    // Internal keys.
    pub const VOLUME: &str = "volume";
    pub const CROSSFADE_SECONDS: &str = "crossfade-seconds";
    pub const AUDIO_OUTPUT_DEVICE: &str = "audio-output-device";
    pub const AUDIO_OUTPUT_NAME: &str = "audio-output-name";
    pub const EQ_CURRENT: &str = "eq-current";
    pub const EQ_BANDS: &str = "eq-bands";
    pub const EQ_PREAMP: &str = "eq-preamp";
    pub const EQ_PRESET_PREFIX: &str = "eq-preset:";
    pub const EQ_PRESET_PREAMP_PREFIX: &str = "eq-preset-preamp:";
    pub const LIBRARY_ROOTS: &str = "library-roots";
    pub const LIBRARY_ROOTS_CONFIGURED: &str = "library-roots-configured";
    pub const LIBRARY_ROOT_SCOPE_VERSION: &str = "library-root-scope-version";
    pub const IGNORED_DIRECTORIES: &str = "ignored-directories";
    pub const FILE_EXTENSION_ENABLED: &str = "file-extension-enabled";
    pub const FILE_EXTENSION_TYPES: &str = "file-extension-types";
    pub const LIBRARY_LAST_SCAN_UTC: &str = "library-last-scan-utc";
    pub const LAST_VIEW_CONTEXT: &str = "last-view-context";
    pub const RECENT_SEARCHES: &str = "recent-searches";
    pub const LAST_UPDATE_CHECK: &str = "last-update-check";
    pub const UPDATE_ETAG: &str = "update-etag";
    pub const UPDATE_RELEASE_TAG: &str = "update-release-tag";
    pub const UPDATE_RELEASE_URL: &str = "update-release-url";
    pub const UPDATE_RELEASE_CHANNEL: &str = "update-release-channel";

    pub const ROOT_SCOPE_V2: &str = "selected-music-folders-v2";
    pub const DEFAULT_ACCENT: &str = "#b7ff2d";

    pub const DEFAULT_NAVIGATION_ORDER: [&str; 10] = [
        "Home",
        "Search",
        "Favorites",
        "Songs",
        "Albums",
        "Artists",
        "Genres",
        "Recently Added",
        "Playlists",
        "Folders",
    ];
}

pub const CROSSFADE_OPTIONS: [i64; 5] = [2, 3, 5, 8, 10];

#[derive(Clone)]
pub struct PlayerSettings {
    store: LibraryStore,
}

impl PlayerSettings {
    pub fn new(store: LibraryStore) -> Self {
        PlayerSettings { store }
    }

    pub fn raw(&self, key: &str) -> Option<String> {
        self.store.get_setting(key).ok().flatten()
    }

    pub fn set_raw(&self, key: &str, value: &str) {
        let _ = self.store.set_setting(key, value);
    }

    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        match self.raw(key) {
            Some(v) => v == "true",
            None => default,
        }
    }

    pub fn int_or(&self, key: &str, default: i64) -> i64 {
        self.raw(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// Tri-state bool: `true`, `false`, or None for `auto`.
    pub fn tri_state(&self, key: &str) -> Option<bool> {
        match self.raw(key) {
            Some(v) if v == "true" => Some(true),
            Some(v) if v == "false" => Some(false),
            _ => None,
        }
    }

    pub fn string_list(&self, key: &str) -> Vec<String> {
        self.raw(key)
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default()
    }

    pub fn set_string_list(&self, key: &str, values: &[String]) {
        let value = serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
        self.set_raw(key, &value);
    }

    // --- Library roots / exclusions ---------------------------------------

    pub fn library_roots(&self) -> Vec<String> {
        self.string_list(keys::LIBRARY_ROOTS)
    }

    pub fn set_library_roots(&self, roots: &[String]) {
        self.set_string_list(keys::LIBRARY_ROOTS, roots);
        if !roots.is_empty() {
            self.set_raw(keys::LIBRARY_ROOTS_CONFIGURED, "true");
            self.set_raw(keys::LIBRARY_ROOT_SCOPE_VERSION, keys::ROOT_SCOPE_V2);
        }
    }

    pub fn roots_configured(&self) -> bool {
        self.bool_or(keys::LIBRARY_ROOTS_CONFIGURED, false)
    }

    pub fn ignored_directories(&self) -> Vec<String> {
        self.string_list(keys::IGNORED_DIRECTORIES)
    }

    pub fn set_ignored_directories(&self, dirs: &[String]) {
        self.set_string_list(keys::IGNORED_DIRECTORIES, dirs);
    }

    /// Extension enable map: `.mp3` -> true/false (default true unless listed false).
    pub fn extension_enabled(&self) -> std::collections::HashMap<String, bool> {
        self.raw(keys::FILE_EXTENSION_ENABLED)
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    pub fn set_extension_enabled(&self, map: &std::collections::HashMap<String, bool>) {
        let value = serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string());
        self.set_raw(keys::FILE_EXTENSION_ENABLED, &value);
    }

    /// Extension kind map: `.mp4` -> "Audio" | "Video".
    pub fn extension_types(&self) -> std::collections::HashMap<String, String> {
        self.raw(keys::FILE_EXTENSION_TYPES)
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    pub fn set_extension_types(&self, map: &std::collections::HashMap<String, String>) {
        let value = serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string());
        self.set_raw(keys::FILE_EXTENSION_TYPES, &value);
    }

    pub fn is_extension_enabled(&self, ext: &str) -> bool {
        let map = self.extension_enabled();
        match map.get(ext.to_lowercase().as_str()) {
            Some(enabled) => *enabled,
            None => true,
        }
    }

    // --- Validation & update ----------------------------------------------

    /// Validates and persists a web-named setting; returns the store key that
    /// was written, or an error message (mirrors `updateSettings` semantics).
    pub fn update_web_setting(&self, name: &str, value: &Value) -> Result<String, String> {
        let key = match name {
            "theme" => {
                let v = value.as_str().unwrap_or("System");
                if !matches!(v, "System" | "Light" | "Dark") {
                    return Err(format!("The setting '{}' is invalid.", name));
                }
                self.set_raw(keys::THEME, v);
                keys::THEME
            }
            "accentMode" => {
                let v = value.as_str().unwrap_or("Artwork");
                if !matches!(v, "Native" | "Artwork") {
                    return Err(format!("The setting '{}' is invalid.", name));
                }
                self.set_raw(keys::ACCENT_MODE, v);
                keys::ACCENT_MODE
            }
            "accentManual" | "transparentWindow" | "hideDuplicates" | "autoplaySimilar"
            | "autoOpenPanel" | "autoLoadLyrics" | "seekFromLyrics" | "showArtwork"
            | "showArtist" | "showAlbum" | "showAdded" | "showYear" | "showDuration"
            | "showFavorite" | "checkUpdates" | "minimizeToTray" | "rightPanelOpen" => {
                let v = match value.as_bool() {
                    Some(b) => b.to_string(),
                    None => match value.as_str() {
                        Some("true") | Some("false") => value.as_str().unwrap().to_string(),
                        _ => return Err(format!("The setting '{}' is invalid.", name)),
                    },
                };
                let key = match name {
                    "accentManual" => keys::ACCENT_MANUAL,
                    "transparentWindow" => keys::TRANSPARENT_WINDOW,
                    "hideDuplicates" => keys::HIDE_DUPLICATES,
                    "autoplaySimilar" => keys::AUTOPLAY_SIMILAR,
                    "autoOpenPanel" => keys::AUTO_OPEN_PANEL,
                    "autoLoadLyrics" => keys::AUTO_LOAD_LYRICS,
                    "seekFromLyrics" => keys::SEEK_FROM_LYRICS,
                    "showArtwork" => keys::SHOW_ARTWORK,
                    "showArtist" => keys::SHOW_ARTIST,
                    "showAlbum" => keys::SHOW_ALBUM,
                    "showAdded" => keys::SHOW_ADDED,
                    "showYear" => keys::SHOW_YEAR,
                    "showDuration" => keys::SHOW_DURATION,
                    "showFavorite" => keys::SHOW_FAVORITE,
                    "checkUpdates" => keys::CHECK_UPDATES,
                    "minimizeToTray" => keys::MINIMIZE_TO_TRAY,
                    _ => keys::RIGHT_PANEL_OPEN,
                };
                self.set_raw(key, &v);
                key
            }
            "accentColor" | "selectionColor" => {
                let v = value.as_str().unwrap_or("");
                if !is_hex_color(v) {
                    return Err(format!("The setting '{}' is invalid.", name));
                }
                let key = if name == "accentColor" {
                    keys::ACCENT_COLOR
                } else {
                    keys::SELECTION_COLOR
                };
                self.set_raw(key, v);
                key
            }
            "selectionColorMode" => {
                let v = value.as_str().unwrap_or("Theme");
                if !matches!(v, "Theme" | "Custom") {
                    return Err(format!("The setting '{}' is invalid.", name));
                }
                self.set_raw(keys::SELECTION_COLOR_MODE, v);
                keys::SELECTION_COLOR_MODE
            }
            "windowTransparency" => {
                let n = value.as_i64().unwrap_or(0).clamp(0, 60);
                self.set_raw(keys::WINDOW_TRANSPARENCY, &n.to_string());
                keys::WINDOW_TRANSPARENCY
            }
            "motionStyle" => {
                let v = value.as_str().unwrap_or("Subtle");
                if !matches!(v, "Off" | "Subtle" | "Expressive") {
                    return Err(format!("The setting '{}' is invalid.", name));
                }
                self.set_raw(keys::MOTION_STYLE, v);
                keys::MOTION_STYLE
            }
            "navigationWidth" => {
                let n = value.as_i64().unwrap_or(232).clamp(180, 360);
                self.set_raw(keys::NAVIGATION_WIDTH, &n.to_string());
                keys::NAVIGATION_WIDTH
            }
            "browseWidth" => {
                let n = value.as_i64().unwrap_or(280).clamp(180, 480);
                self.set_raw(keys::BROWSE_WIDTH, &n.to_string());
                keys::BROWSE_WIDTH
            }
            "navigationOrder" | "hiddenPanels" => {
                let list = read_string_array(value);
                let key = if name == "navigationOrder" {
                    keys::NAVIGATION_ORDER
                } else {
                    keys::HIDDEN_PANELS
                };
                self.set_string_list(key, &list);
                key
            }
            "rightSidebarMode" => {
                let v = value.as_str().unwrap_or("Queue");
                if !matches!(v, "Queue" | "Info") {
                    return Err(format!("The setting '{}' is invalid.", name));
                }
                self.set_raw(keys::RIGHT_SIDEBAR_MODE, v);
                keys::RIGHT_SIDEBAR_MODE
            }
            "rightPanelWidth" => {
                let n = value.as_i64().unwrap_or(294).clamp(240, 460);
                self.set_raw(keys::RIGHT_PANEL_WIDTH, &n.to_string());
                keys::RIGHT_PANEL_WIDTH
            }
            "sidebarCollapsed" => {
                let v = match value {
                    Value::Bool(b) => b.to_string(),
                    Value::String(s) if s == "true" || s == "false" || s == "auto" => s.clone(),
                    _ => "auto".to_string(),
                };
                self.set_raw(keys::SIDEBAR_COLLAPSED, &v);
                keys::SIDEBAR_COLLAPSED
            }
            "songColumnWidths" => {
                let obj = value.as_object().cloned().unwrap_or_default();
                let mut widths = serde_json::Map::new();
                let clamp = |v: i64, lo: i64, hi: i64| v.clamp(lo, hi);
                if let Some(v) = obj.get("title").and_then(Value::as_i64) {
                    widths.insert("title".into(), json!(clamp(v, 160, 1200)));
                }
                if let Some(v) = obj.get("album").and_then(Value::as_i64) {
                    widths.insert("album".into(), json!(clamp(v, 90, 800)));
                }
                if let Some(v) = obj.get("added").and_then(Value::as_i64) {
                    widths.insert("added".into(), json!(clamp(v, 76, 300)));
                }
                if let Some(v) = obj.get("year").and_then(Value::as_i64) {
                    widths.insert("year".into(), json!(clamp(v, 50, 200)));
                }
                if let Some(v) = obj.get("plays").and_then(Value::as_i64) {
                    widths.insert("plays".into(), json!(clamp(v, 50, 200)));
                }
                if let Some(v) = obj.get("duration").and_then(Value::as_i64) {
                    widths.insert("duration".into(), json!(clamp(v, 50, 200)));
                }
                self.set_raw(keys::SONG_COLUMN_WIDTHS, &Value::Object(widths).to_string());
                keys::SONG_COLUMN_WIDTHS
            }
            other => {
                return Err(format!(
                    "The setting '{}' is not editable from the player.",
                    other
                ))
            }
        };
        Ok(key.to_string())
    }

    /// Resets setting groups (mirrors `resetUiSettings` groups).
    pub fn reset_groups(&self, groups: &[String]) {
        for group in groups {
            match group.as_str() {
                "appearance" => {
                    self.set_raw(keys::THEME, "System");
                    self.set_raw(keys::ACCENT_MODE, "Artwork");
                    self.set_raw(keys::ACCENT_MANUAL, "false");
                    self.set_raw(keys::ACCENT_COLOR, keys::DEFAULT_ACCENT);
                    self.set_raw(keys::SELECTION_COLOR_MODE, "Theme");
                    self.set_raw(keys::SELECTION_COLOR, keys::DEFAULT_ACCENT);
                    self.set_raw(keys::TRANSPARENT_WINDOW, "false");
                    self.set_raw(keys::WINDOW_TRANSPARENCY, "0");
                    self.set_raw(keys::MOTION_STYLE, "Subtle");
                }
                "layout" => {
                    self.set_raw(keys::NAVIGATION_WIDTH, "232");
                    self.set_raw(keys::BROWSE_WIDTH, "280");
                    self.set_raw(keys::RIGHT_PANEL_WIDTH, "294");
                    self.set_raw(keys::SIDEBAR_COLLAPSED, "auto");
                    self.set_raw(keys::AUTO_OPEN_PANEL, "true");
                    self.set_raw(keys::RIGHT_SIDEBAR_MODE, "Queue");
                    self.set_raw(keys::RIGHT_PANEL_OPEN, "auto");
                    let order: Vec<String> = keys::DEFAULT_NAVIGATION_ORDER
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    self.set_string_list(keys::NAVIGATION_ORDER, &order);
                    self.set_string_list(keys::HIDDEN_PANELS, &[]);
                }
                "libraryDisplay" => {
                    self.set_raw(keys::HIDE_DUPLICATES, "true");
                    self.set_raw(keys::SHOW_ARTWORK, "true");
                    self.set_raw(keys::SHOW_ARTIST, "true");
                    self.set_raw(keys::SHOW_ALBUM, "true");
                    self.set_raw(keys::SHOW_ADDED, "true");
                    self.set_raw(keys::SHOW_YEAR, "false");
                    self.set_raw(keys::SHOW_DURATION, "true");
                    self.set_raw(keys::SHOW_FAVORITE, "true");
                    self.set_raw(keys::SONG_COLUMN_WIDTHS, "{}");
                }
                _ => {}
            }
        }
    }

    // --- Recent searches ---------------------------------------------------

    pub fn save_recent_search(&self, term: &str) {
        let trimmed = term.trim();
        if trimmed.chars().count() < 2 || trimmed.chars().all(char::is_whitespace) {
            return;
        }
        let mut list = self.recent_searches();
        let lowered = trimmed.to_lowercase();
        list.retain(|t| t.to_lowercase() != lowered);
        list.insert(0, trimmed.to_string());
        list.truncate(10);
        self.set_string_list(keys::RECENT_SEARCHES, &list);
    }

    pub fn recent_searches(&self) -> Vec<String> {
        self.string_list(keys::RECENT_SEARCHES)
    }

    // --- Equalizer ----------------------------------------------------------

    pub fn eq_current(&self) -> String {
        self.raw(keys::EQ_CURRENT).unwrap_or_else(|| "Off".into())
    }

    pub fn eq_bands(&self) -> Option<[f32; 10]> {
        self.raw(keys::EQ_BANDS).and_then(|v| {
            serde_json::from_str::<Vec<f32>>(&v).ok().and_then(|v| {
                if v.len() == 10 {
                    let mut out = [0f32; 10];
                    out.copy_from_slice(&v);
                    Some(out)
                } else {
                    None
                }
            })
        })
    }

    pub fn eq_preamp(&self) -> Option<f32> {
        self.raw(keys::EQ_PREAMP).and_then(|v| v.parse().ok())
    }

    pub fn set_eq_state(&self, name: &str, bands: [f32; 10], preamp: f32) {
        self.set_raw(
            keys::EQ_BANDS,
            &serde_json::to_string(&bands).unwrap_or_else(|_| "[]".into()),
        );
        self.set_raw(keys::EQ_PREAMP, &format!("{}", preamp));
        self.set_raw(keys::EQ_CURRENT, name);
    }

    pub fn saved_eq_preset(&self, name: &str) -> Option<([f32; 10], f32)> {
        let bands = self
            .raw(&format!("{}{}", keys::EQ_PRESET_PREFIX, name))
            .and_then(|v| {
                serde_json::from_str::<Vec<f32>>(&v).ok().and_then(|v| {
                    if v.len() == 10 {
                        let mut out = [0f32; 10];
                        out.copy_from_slice(&v);
                        Some(out)
                    } else {
                        None
                    }
                })
            })?;
        let preamp = self
            .raw(&format!("{}{}", keys::EQ_PRESET_PREAMP_PREFIX, name))
            .and_then(|v| v.parse().ok())
            .unwrap_or(12.0412);
        Some((bands, preamp))
    }

    pub fn save_eq_preset(&self, name: &str, bands: [f32; 10], preamp: f32) {
        self.set_raw(
            &format!("{}{}", keys::EQ_PRESET_PREFIX, name),
            &serde_json::to_string(&bands).unwrap_or_else(|_| "[]".into()),
        );
        self.set_raw(
            &format!("{}{}", keys::EQ_PRESET_PREAMP_PREFIX, name),
            &format!("{}", preamp),
        );
    }

    pub fn saved_eq_preset_names(&self) -> Vec<String> {
        // Settings scan for the preset prefix.
        self.raw(keys::EQ_CURRENT); // ensure connection alive
        self.store
            .get_setting_keys_with_prefix(keys::EQ_PRESET_PREFIX)
            .map(|keys| {
                keys.into_iter()
                    .map(|k| k.trim_start_matches(keys::EQ_PRESET_PREFIX).to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    // --- Crossfade ----------------------------------------------------------

    /// Crossfade whitelist {2,3,5,8,10}; anything else disables crossfade.
    pub fn crossfade_seconds(&self) -> i64 {
        let parsed = self.int_or(keys::CROSSFADE_SECONDS, 0);
        if CROSSFADE_OPTIONS.contains(&parsed) {
            parsed
        } else {
            0
        }
    }
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

fn read_string_array(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .collect(),
        Value::String(text) => serde_json::from_str::<Vec<String>>(text).unwrap_or_default(),
        _ => Vec::new(),
    }
}
