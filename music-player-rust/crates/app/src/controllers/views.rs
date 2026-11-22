use crate::*;

impl App {
    pub(crate) fn publish_appearance(&self) {
        use music_player_core::settings::keys as k;
        let theme = self
            .settings
            .raw(k::THEME)
            .unwrap_or_else(|| "System".into());
        let light = match theme.as_str() {
            "Light" => true,
            "Dark" => false,
            _ => native_io::prefers_light_theme(),
        };
        let accent_mode = self
            .settings
            .raw(k::ACCENT_MODE)
            .unwrap_or_else(|| "Artwork".into());
        let manual = self.settings.bool_or(k::ACCENT_MANUAL, false);
        let manual_hex = self
            .settings
            .raw(k::ACCENT_COLOR)
            .unwrap_or_else(|| "#b7ff2d".into());
        let artwork_palette = if accent_mode == "Artwork" {
            self.current
                .as_ref()
                .and_then(|track| track.artwork_path.as_deref())
                .and_then(|path| crate::native_io::extract_artwork_palette(Path::new(path)))
        } else {
            None
        };
        let native_accent = if light { "#548c00" } else { "#b7ff2d" };
        let accent_hex = if manual {
            manual_hex
        } else {
            artwork_palette
                .as_ref()
                .map(|palette| palette.accent.hex())
                .unwrap_or_else(|| native_accent.to_string())
        };
        let state = self.ui.global::<State>();
        state.set_theme(ss(theme));
        state.set_light_theme(light);
        state.set_accent_mode(ss(&accent_mode));
        state.set_accent_manual_enabled(manual);
        state.set_accent_manual(parse_hex_color(
            &self
                .settings
                .raw(k::ACCENT_COLOR)
                .unwrap_or_else(|| "#b7ff2d".into()),
        ));
        state.set_artwork_palette(artwork_palette.is_some());
        if let Some(palette) = &artwork_palette {
            state.set_art_background(parse_hex_color(&palette.background.hex()));
            state.set_art_sidebar(parse_hex_color(&palette.sidebar.hex()));
            state.set_art_main(parse_hex_color(&palette.main.hex()));
            state.set_art_panel(parse_hex_color(&palette.panel.hex()));
            state.set_art_raised(parse_hex_color(&palette.raised.hex()));
            state.set_art_hover(parse_hex_color(&palette.hover.hex()));
            state.set_art_control(parse_hex_color(&palette.control.hex()));
        }
        state.set_accent(parse_hex_color(&accent_hex));
        state.set_accent_ink(contrasting_ink(&accent_hex));
        let accent_preset = match accent_hex.to_ascii_lowercase().as_str() {
            "#59d7ff" => "Sky",
            "#a878ff" => "Violet",
            "#ff6e91" => "Rose",
            "#ffb04a" => "Amber",
            _ => "Lime",
        };
        state.set_accent_preset_name(ss(accent_preset));

        let custom_caption = if accent_mode == "Artwork" {
            artwork_palette
                .as_ref()
                .map(|p| (p.sidebar.r, p.sidebar.g, p.sidebar.b))
        } else {
            None
        };
        let hwnd = self
            .ui
            .window()
            .window_handle()
            .window_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                _ => None,
            });
        if let Some(hwnd) = hwnd {
            self.sync_titlebar_theme(hwnd, light, custom_caption);
        }
    }

    // ---- State publication --------------------------------------------------

    pub(crate) fn publish_playback(&mut self) {
        let position = self.engine.position_ms();
        let duration = self.engine.duration_ms();
        let playing = self.engine.is_playing();
        if let Some(os) = &self.os {
            os.set_status(os::TrayStatus {
                title: self
                    .current
                    .as_ref()
                    .map(|t| t.title.clone())
                    .unwrap_or_default(),
                artist: self
                    .current
                    .as_ref()
                    .map(|t| t.artist.clone())
                    .unwrap_or_default(),
                playing,
            });
        }
        os_shell::update_playback(
            self.current
                .as_ref()
                .map(|t| t.title.as_str())
                .unwrap_or(""),
            self.current
                .as_ref()
                .map(|t| t.artist.as_str())
                .unwrap_or(""),
            self.current
                .as_ref()
                .map(|t| t.album.as_str())
                .unwrap_or(""),
            self.current
                .as_ref()
                .and_then(|t| t.artwork_path.as_deref()),
            self.current.is_some() || !self.coordinator.queue().entries().is_empty(),
            playing,
            position,
            duration,
        );
        let display_position = self.seek_preview_ms.unwrap_or(position);
        let progress = if duration > 0 {
            (display_position as f32 / duration as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let state = self.ui.global::<State>();
        state.set_is_playing(playing);
        state.set_progress(progress);
        state.set_current_time(ss(fmt_time(display_position)));
        state.set_total_time(ss(fmt_time(duration)));
        state.set_has_track(self.current.is_some());
        state.set_crossfading(self.crossfade_in_progress);
        state.set_shuffle(self.coordinator.queue().shuffle);
        state.set_repeat_mode(ss(self.coordinator.queue().repeat_mode.as_str()));
        state.set_volume(self.volume as i32);
        if let Some(track) = &self.current {
            state.set_repeat_a(ss(self
                .coordinator
                .queue()
                .repeat_a_ms()
                .map(fmt_time)
                .unwrap_or_default()));
            state.set_repeat_b(ss(self
                .coordinator
                .queue()
                .repeat_b_ms()
                .map(fmt_time)
                .unwrap_or_default()));
            let _ = track;
        } else {
            state.set_repeat_a(ss(""));
            state.set_repeat_b(ss(""));
        }
    }

    pub(crate) fn update_current_track(&mut self, track: &Track) {
        self.seek_wheel_timer.stop();
        self.seek_preview_ms = None;
        self.current = Some(track.clone());
        self.id_to_path.insert(track.id(), track.path.clone());
        let state = self.ui.global::<State>();
        state.set_current_id(ss(track.id()));
        state.set_current_title(ss(&track.title));
        state.set_current_artist(ss(&track.artist));
        state.set_current_album(ss(&track.album));
        state.set_current_unavailable(track.unavailable);
        state.set_current_favorite(track.favorite);
        let (has_cover, cover) = track
            .artwork_path
            .as_deref()
            .and_then(|p| native_io::load_artwork_image(Path::new(p)))
            .map(|image| (true, image))
            .unwrap_or((false, Image::default()));
        state.set_current_cover(cover);
        let slot = gradient_slot(if track.album.is_empty() {
            &track.title
        } else {
            &track.album
        });
        state.set_current_has_cover(has_cover);
        state.set_current_initials(ss(initials(&track.title)));
        state.set_current_badge_bg(badge_color(slot));
        drop(state);
        self.publish_appearance();
        self.publish_info(track);
        self.load_lyrics(track);
    }

    pub(crate) fn publish_info(&mut self, track: &Track) {
        let details = inspect_file(Path::new(&track.path));
        let state = self.ui.global::<State>();
        state.set_info_album(ss(&track.album));
        state.set_info_album_artist(ss(&track.album_artist));
        state.set_info_year_genre(ss(format!(
            "{} · {}",
            if track.year > 0 {
                track.year.to_string()
            } else {
                "—".into()
            },
            if track.genre.is_empty() {
                "—".into()
            } else {
                track.genre.clone()
            }
        )));
        state.set_info_format(ss(details
            .as_ref()
            .map(|d| d.container.clone())
            .unwrap_or_else(|| {
                file_name(&track.path)
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_uppercase()
            })));
        state.set_info_status(ss(if self.engine.is_playing() {
            "Playing"
        } else {
            "Paused"
        }));
        state.set_info_quality(ss(details
            .as_ref()
            .map(|d| {
                format!(
                    "{} kbps · {} Hz · {} bit",
                    d.bitrate_kbps, d.sample_rate_hz, d.bits_per_sample
                )
            })
            .unwrap_or_default()));
        state.set_info_location(ss(&track.path));
    }

    pub(crate) fn track_to_ui(
        &mut self,
        track: &Track,
        index: i32,
        playing_path: &str,
    ) -> TrackItem {
        self.id_to_path.insert(track.id(), track.path.clone());
        let seed = if !track.album.is_empty() {
            format!(
                "{}:{}",
                if track.album_artist.is_empty() {
                    &track.artist
                } else {
                    &track.album_artist
                },
                &track.album
            )
        } else if !track.artist.is_empty() {
            track.artist.clone()
        } else {
            track.title.clone()
        };
        let slot = gradient_slot(&seed);
        let mark_src = if !track.album.is_empty() {
            &track.album
        } else if !track.artist.is_empty() {
            &track.artist
        } else {
            &track.title
        };
        let added = chrono::DateTime::from_timestamp(track.added_utc.timestamp(), 0)
            .map(|dt| dt.timestamp() as u64)
            .unwrap_or(0);
        let (has_cover, cover) = track
            .artwork_path
            .as_deref()
            .and_then(|path| native_io::artwork_preview(Path::new(path)))
            .map(|image| (true, image))
            .unwrap_or((false, Image::default()));
        TrackItem {
            id: ss(track.id()),
            position: (index - 1).max(0),
            title: ss(&track.title),
            artist: ss(&track.artist),
            album: ss(&track.album),
            album_artist: ss(&track.album_artist),
            genre: ss(&track.genre),
            year: ss(if track.year > 0 {
                track.year.to_string()
            } else {
                String::new()
            }),
            duration: ss(fmt_time(track.duration_ms)),
            added: ss(fmt_added(added)),
            initials: ss(initials(mark_src)),
            badge_bg: badge_color(slot),
            has_cover,
            cover,
            is_favorite: track.favorite,
            rating: track.rating as i32,
            play_count: track.play_count as i32,
            is_playing: track.path.eq_ignore_ascii_case(playing_path),
            unavailable: track.unavailable,
        }
    }

    pub(crate) fn queue_item(
        &mut self,
        track: &Track,
        queue_index: usize,
        is_current: bool,
        playing_path: &str,
    ) -> QueueItem {
        self.id_to_path.insert(track.id(), track.path.clone());
        let seed = if !track.album.is_empty() {
            format!(
                "{}:{}",
                if track.album_artist.is_empty() {
                    &track.artist
                } else {
                    &track.album_artist
                },
                &track.album
            )
        } else if !track.artist.is_empty() {
            track.artist.clone()
        } else {
            track.title.clone()
        };
        let slot = gradient_slot(&seed);
        let mark_src = if !track.album.is_empty() {
            &track.album
        } else if !track.artist.is_empty() {
            &track.artist
        } else {
            &track.title
        };
        let (has_cover, cover) = track
            .artwork_path
            .as_deref()
            .and_then(|path| native_io::artwork_preview(Path::new(path)))
            .map(|image| (true, image))
            .unwrap_or((false, Image::default()));
        QueueItem {
            id: ss(track.id()),
            queue_index: queue_index as i32,
            title: ss(&track.title),
            artist: ss(&track.artist),
            duration: ss(fmt_time(track.duration_ms)),
            initials: ss(initials(mark_src)),
            badge_bg: badge_color(slot),
            has_cover,
            cover,
            is_current,
            unavailable: track.unavailable,
            is_playing: track.path.eq_ignore_ascii_case(playing_path),
        }
    }

    /// Placeholder DTO from the WebUI contract for paths whose library row is gone.
    pub(crate) fn placeholder_item(&mut self, path: &str, playing_path: &str) -> TrackItem {
        let id = music_player_core::track_id_for_path(path);
        self.id_to_path.insert(id.clone(), path.to_string());
        TrackItem {
            id: ss(&id),
            position: 0,
            title: ss(file_stem(path)),
            artist: ss("Unavailable"),
            album: ss(""),
            album_artist: ss(""),
            genre: ss(""),
            year: ss(""),
            duration: ss("0:00"),
            added: ss("Unavailable"),
            initials: ss(initials(&file_stem(path))),
            badge_bg: badge_color(3),
            has_cover: false,
            cover: Image::default(),
            is_favorite: false,
            rating: 0,
            play_count: 0,
            is_playing: path.eq_ignore_ascii_case(playing_path),
            unavailable: true,
        }
    }

    pub(crate) fn publish_queue(&mut self, force: bool) {
        let entries = self.coordinator.queue().entries().to_vec();
        let current_index = self.coordinator.queue().current_index();
        let start = current_index.unwrap_or(0);
        let full_queue_view = {
            let state = self.ui.global::<State>();
            state.get_view() == "Queue"
                || state.get_immersive_open() && state.get_immersive_kind() == "Queue"
        };
        let display_limit = if full_queue_view {
            self.queue_display_limit
        } else {
            30
        };
        let playing_path = self
            .current
            .as_ref()
            .map(|t| t.path.clone())
            .unwrap_or_default();
        let mut items = Vec::new();
        for (offset, path) in entries.iter().skip(start).take(display_limit).enumerate() {
            let is_current = current_index == Some(start + offset);
            let item = match self.resolve_track(path) {
                Some(track) => self.queue_item(&track, start + offset, is_current, &playing_path),
                None => {
                    let id = music_player_core::track_id_for_path(path);
                    self.id_to_path.insert(id.clone(), path.clone());
                    QueueItem {
                        id: ss(&id),
                        queue_index: (start + offset) as i32,
                        title: ss(file_stem(path)),
                        artist: ss("Unavailable"),
                        duration: ss("0:00"),
                        initials: ss(initials(&file_stem(path))),
                        badge_bg: badge_color(3),
                        has_cover: false,
                        cover: Image::default(),
                        is_current,
                        unavailable: true,
                        is_playing: false,
                    }
                }
            };
            items.push(item);
        }
        let upcoming_start = current_index.map(|i| i + 1).unwrap_or(0);
        let mut panel_items = Vec::new();
        for (offset, path) in entries.iter().skip(upcoming_start).take(30).enumerate() {
            let item = match self.resolve_track(path) {
                Some(track) => {
                    self.queue_item(&track, upcoming_start + offset, false, &playing_path)
                }
                None => {
                    let id = music_player_core::track_id_for_path(path);
                    self.id_to_path.insert(id.clone(), path.clone());
                    QueueItem {
                        id: ss(&id),
                        queue_index: (upcoming_start + offset) as i32,
                        title: ss(file_stem(path)),
                        artist: ss("Unavailable"),
                        duration: ss("0:00"),
                        initials: ss(initials(&file_stem(path))),
                        badge_bg: badge_color(3),
                        has_cover: false,
                        cover: Image::default(),
                        is_current: false,
                        unavailable: true,
                        is_playing: false,
                    }
                }
            };
            panel_items.push(item);
        }
        let panel_more = entries.len().saturating_sub(upcoming_start + 30);
        let view_more = if full_queue_view {
            entries.len().saturating_sub(start + display_limit)
        } else {
            0
        };
        let state = self.ui.global::<State>();
        state.set_queue_items(ui_model(items).into());
        state.set_panel_queue_items(ui_model(panel_items).into());
        let upcoming = entries.len().saturating_sub(upcoming_start);
        state.set_queue_count(upcoming as i32);
        state.set_queue_current_index(current_index.map(|index| index as i32).unwrap_or(-1));
        state.set_queue_more_count(view_more as i32);
        state.set_queue_panel_more_count(panel_more as i32);
        let _ = force;
    }

    pub(crate) fn publish_scan(&mut self) {
        let status = self.scan.get_status();
        let state = self.ui.global::<State>();
        state.set_scan_active(status.active);
        state.set_scan_paused(status.paused);
        state.set_scan_cancelling(status.cancelling);
        if !status.active {
            let stats = self.store.get_library_stats().unwrap_or(LibraryStats {
                total_tracks: 0,
                total_bytes: 0,
            });
            state.set_library_tracks(stats.total_tracks as i32);
            state.set_indexed_library_size(ss(fmt_bytes(stats.total_bytes)));
        }
    }

    pub(crate) fn publish_folder_roots(&mut self, roots: &[String]) {
        let items: Vec<FolderRootItem> = roots
            .iter()
            .map(|r| {
                let trimmed = r.trim_end_matches(['\\', '/']);
                let name = Path::new(trimmed)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(r);
                let lower_name = name.to_ascii_lowercase();
                let lower_path = r.to_ascii_lowercase();
                let icon = if lower_name.contains("download") || lower_path.contains("download") {
                    "download"
                } else if lower_name == "music" || lower_path.contains("music") {
                    "music"
                } else if Path::new(trimmed).parent().is_none() || trimmed.ends_with(':') {
                    "disc"
                } else {
                    "folder"
                };
                FolderRootItem {
                    name: ss(name),
                    path: ss(r),
                    icon: ss(icon),
                    track_count: self.store.count_tracks_in_folder(r).unwrap_or(0) as i32,
                    is_available: Path::new(r).exists(),
                }
            })
            .collect();
        self.ui
            .global::<State>()
            .set_folder_roots(ui_model(items).into());
    }

    pub(crate) fn refresh_playlists(&mut self) {
        self.playlists = self.store.get_playlists().unwrap_or_default();
        let video_extensions = self.video_extensions();
        let smart_specs = [
            ("Most Played", "Songs played most often"),
            ("With Lyrics", "Songs with available lyrics"),
            ("Recently Played", "Your recent listening history"),
        ];
        let mut smart_cards: Vec<PlaylistCardItem> = smart_specs
            .iter()
            .map(|(view, description)| {
                let query = match *view {
                    "Most Played" => TrackQuery {
                        most_played: true,
                        sort: TrackSort::PlayCount,
                        descending: true,
                        page_size: 1,
                        ..TrackQuery::default()
                    },
                    "With Lyrics" => TrackQuery {
                        with_lyrics: true,
                        page_size: 1,
                        ..TrackQuery::default()
                    },
                    _ => TrackQuery {
                        recently_played: true,
                        sort: TrackSort::LastPlayed,
                        descending: true,
                        page_size: 1,
                        ..TrackQuery::default()
                    },
                };
                let count = self
                    .store
                    .query_track_page(&query)
                    .map(|(_, count)| count)
                    .unwrap_or(0);
                PlaylistCardItem {
                    id: ss(*view),
                    name: ss(*view),
                    track_count: count as i32,
                    is_smart: true,
                    gradient: 1,
                    initials: ss(""),
                    description: ss(*description),
                }
            })
            .collect();
        if !video_extensions.is_empty() {
            let count = self
                .store
                .query_track_page(&TrackQuery {
                    extension_filter: Some(video_extensions),
                    page_size: 1,
                    ..TrackQuery::default()
                })
                .map(|(_, count)| count)
                .unwrap_or(0);
            smart_cards.push(PlaylistCardItem {
                id: ss("Videos"),
                name: ss("Videos"),
                track_count: count as i32,
                is_smart: true,
                gradient: 1,
                initials: ss(""),
                description: ss("Video files only"),
            });
        }
        let mut cards = Vec::new();
        for p in &self.playlists {
            let slot = gradient_slot(&p.name);
            cards.push(PlaylistCardItem {
                id: ss(&p.id),
                name: ss(&p.name),
                track_count: p.track_count as i32,
                is_smart: false,
                gradient: slot,
                initials: initials(&p.name).into(),
                description: ss(""),
            });
        }
        let names: Vec<SharedString> = self.playlists.iter().map(|p| ss(&p.name)).collect();
        let state = self.ui.global::<State>();
        state.set_playlists(ui_model(cards.clone()).into());
        state.set_playlist_rows(ui_model(chunk_playlists(cards)).into());
        state.set_smart_playlists(ui_model(smart_cards).into());
        state.set_playlist_names(ui_model(names).into());
    }

    pub(crate) fn publish_settings(&mut self) {
        use music_player_core::settings::keys as k;
        self.publish_appearance();
        let s = &self.settings;
        let state = self.ui.global::<State>();
        state.set_transparent_window(s.bool_or(k::TRANSPARENT_WINDOW, false));
        state.set_window_transparency(s.int_or(k::WINDOW_TRANSPARENCY, 0) as i32);
        state.set_accent_color(ss(s
            .raw(k::ACCENT_COLOR)
            .unwrap_or_else(|| "#b7ff2d".into())));
        let selection_color = s
            .raw(k::SELECTION_COLOR)
            .unwrap_or_else(|| "#b7ff2d".into());
        state.set_selection_color(ss(selection_color.clone()));
        state.set_selection_color_value(parse_hex_color(&selection_color));
        state.set_selection_color_ink(contrasting_ink(&selection_color));
        state.set_selection_color_mode(ss(s
            .raw(k::SELECTION_COLOR_MODE)
            .unwrap_or_else(|| "Theme".into())));
        let library_stats = self.store.get_library_stats().unwrap_or(LibraryStats {
            total_tracks: 0,
            total_bytes: 0,
        });
        state.set_indexed_library_size(ss(fmt_bytes(library_stats.total_bytes)));
        state.set_motion_style(ss(s
            .raw(k::MOTION_STYLE)
            .unwrap_or_else(|| "Subtle".into())));
        state.set_hide_exact_duplicates(s.bool_or(k::HIDE_DUPLICATES, true));
        state.set_autoplay_similar(s.bool_or(k::AUTOPLAY_SIMILAR, true));
        state.set_auto_open_side_panel(s.bool_or(k::AUTO_OPEN_PANEL, true));
        state.set_auto_load_lyrics(s.bool_or(k::AUTO_LOAD_LYRICS, true));
        state.set_seek_from_lyrics(s.bool_or(k::SEEK_FROM_LYRICS, true));
        state.set_show_list_artwork(s.bool_or(k::SHOW_ARTWORK, true));
        state.set_show_list_artist(s.bool_or(k::SHOW_ARTIST, true));
        state.set_show_list_album(s.bool_or(k::SHOW_ALBUM, true));
        state.set_show_list_added(s.bool_or(k::SHOW_ADDED, true));
        state.set_show_list_year(s.bool_or(k::SHOW_YEAR, false));
        state.set_show_list_duration(s.bool_or(k::SHOW_DURATION, true));
        state.set_show_list_favorite(s.bool_or(k::SHOW_FAVORITE, true));
        state.set_check_updates(s.bool_or(k::CHECK_UPDATES, true));
        state.set_minimize_to_tray(s.bool_or(k::MINIMIZE_TO_TRAY, true));
        state.set_navigation_width(s.int_or(k::NAVIGATION_WIDTH, 232) as i32);
        state.set_navigation_collapsed(s.bool_or(k::SIDEBAR_COLLAPSED, false));
        state.set_browse_width(s.int_or(k::BROWSE_WIDTH, 280) as i32);
        state.set_right_panel_width(s.int_or(k::RIGHT_PANEL_WIDTH, 294) as i32);
        state.set_right_panel_open(
            s.raw(k::RIGHT_PANEL_OPEN)
                .map(|v| v != "false")
                .unwrap_or(true),
        );
        state.set_crossfade_label(ss(crossfade_label(s.crossfade_seconds())));
        state.set_eq_preset(ss(s.eq_current()));
        state.set_recent_searches(
            ui_model(s.recent_searches().into_iter().map(ss).collect::<Vec<_>>()).into(),
        );
        self.publish_eq();
        self.publish_exclusions();
        self.publish_navigation();
    }

    pub(crate) fn navigation_order(&self) -> Vec<String> {
        use music_player_core::settings::keys as k;
        let available = [
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
        let mut seen = HashSet::new();
        let mut order = vec!["Home".to_string()];
        for view in self.settings.string_list(k::NAVIGATION_ORDER) {
            if view != "Home" && available.contains(&view.as_str()) && seen.insert(view.clone()) {
                order.push(view);
            }
        }
        for view in available.iter().skip(1) {
            if seen.insert((*view).to_string()) {
                order.push((*view).to_string());
            }
        }
        order
    }

    pub(crate) fn publish_navigation(&self) {
        use music_player_core::settings::keys as k;
        let order = self.navigation_order();
        let hidden: HashSet<String> = self
            .settings
            .string_list(k::HIDDEN_PANELS)
            .into_iter()
            .collect();
        let visible: Vec<&str> = order
            .iter()
            .map(String::as_str)
            .filter(|view| !hidden.contains(*view))
            .collect();
        let group = |view: &str| match view {
            "Home" | "Search" | "Favorites" => (0, "DISCOVER"),
            "Songs" | "Albums" | "Artists" | "Genres" | "Recently Added" => (1, "YOUR LIBRARY"),
            _ => (2, "MANAGE"),
        };
        let mut nav_items = Vec::new();
        let mut seen_groups = HashSet::new();
        for view in visible.iter() {
            let (group_index, header) = group(view);
            let section_title = if seen_groups.insert(group_index) {
                header
            } else {
                ""
            };
            nav_items.push(NavEntry {
                view: ss(*view),
                label: ss(*view),
                section_title: ss(section_title),
            });
        }
        let nav_settings: Vec<NavSettingItem> = order
            .iter()
            .enumerate()
            .map(|(index, view)| NavSettingItem {
                view: ss(view),
                hidden: hidden.contains(view),
                can_move_up: index > 1,
                can_move_down: index > 0 && index + 1 < order.len(),
                always_visible: index == 0,
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_navigation_items(ui_model(nav_items).into());
        state.set_navigation_settings(ui_model(nav_settings).into());
    }

    pub(crate) fn coordinator_eq_name(&self) -> String {
        self.ui.global::<State>().get_eq_preset().to_string()
    }

    pub(crate) fn publish_eq(&mut self) {
        let labels: Vec<String> = EQUALIZER_BANDS
            .iter()
            .map(|f| {
                if *f >= 1000 {
                    format!("{}k", f / 1000)
                } else {
                    f.to_string()
                }
            })
            .collect();
        let bands: Vec<EqBandItem> = labels
            .iter()
            .zip(self.eq_bands.iter())
            .map(|(label, value)| EqBandItem {
                label: ss(label),
                value: *value,
            })
            .collect();
        let mut presets: Vec<String> = vec!["Off".into()];
        presets.extend(self.engine.built_in_preset_names());
        presets.extend(self.settings.saved_eq_preset_names());
        let mut devices: Vec<String> = vec!["System default".into()];
        self.audio_devices = os_shell::audio_output_devices();
        for device in &self.audio_devices {
            devices.push(device.name.clone());
        }
        let stored_id = self
            .settings
            .raw(music_player_core::settings::keys::AUDIO_OUTPUT_DEVICE)
            .unwrap_or_default();
        let mut selected_name = self
            .settings
            .raw(music_player_core::settings::keys::AUDIO_OUTPUT_NAME)
            .unwrap_or_else(|| "System default".into());
        if !stored_id.trim().is_empty()
            && !self
                .audio_devices
                .iter()
                .any(|device| device.id.eq_ignore_ascii_case(&stored_id))
        {
            // The saved endpoint disappeared; fall back exactly like the
            // original AudioSettingsAsync path.
            self.settings
                .set_raw(music_player_core::settings::keys::AUDIO_OUTPUT_DEVICE, "");
            self.settings.set_raw(
                music_player_core::settings::keys::AUDIO_OUTPUT_NAME,
                "System default",
            );
            let _ = self.engine.set_audio_output_device("");
            selected_name = "System default".into();
        }
        let state = self.ui.global::<State>();
        state.set_eq_bands(ui_model(bands).into());
        state.set_eq_preamp(self.eq_preamp);
        state.set_eq_presets(ui_model(presets.into_iter().map(ss).collect::<Vec<_>>()).into());
        state.set_output_devices(ui_model(devices.into_iter().map(ss).collect::<Vec<_>>()).into());
        state.set_output_device(ss(selected_name));
    }

    pub(crate) fn publish_exclusions(&mut self) {
        let enabled = self.settings.extension_enabled();
        let kinds = self.settings.extension_types();
        let mut exts: Vec<String> = music_player_core::AUDIO_EXTENSIONS
            .iter()
            .map(|e| e.to_string())
            .collect();
        for ext in [".mp4", ".m4v", ".mkv", ".avi", ".mov", ".wmv", ".webm"] {
            if !exts.contains(&ext.to_string()) {
                exts.push(ext.to_string());
            }
        }
        for ext in enabled.keys() {
            if !exts.contains(ext) {
                exts.push(ext.clone());
            }
        }
        exts.sort();
        let items: Vec<ExtensionItem> = exts
            .into_iter()
            .map(|e| ExtensionItem {
                extension: ss(&e),
                kind: ss(kinds.get(&e).cloned().unwrap_or_else(|| {
                    if matches!(
                        e.as_str(),
                        ".mp4" | ".m4v" | ".mkv" | ".avi" | ".mov" | ".wmv" | ".webm"
                    ) {
                        "Video".into()
                    } else {
                        "Audio".into()
                    }
                })),
                enabled: *enabled.get(&e).unwrap_or(&true),
            })
            .collect();
        let ignored: Vec<SharedString> = self
            .settings
            .ignored_directories()
            .into_iter()
            .map(ss)
            .collect();
        let state = self.ui.global::<State>();
        state.set_exclusion_extensions(ui_model(items).into());
        state.set_ignored_directories(ui_model(ignored).into());
    }

    pub(crate) fn publish_about(&mut self) {
        let stats = self.store.get_library_stats().unwrap_or(LibraryStats {
            total_tracks: 0,
            total_bytes: 0,
        });
        let rows = vec![
            DetailRowItem {
                label: ss("Version"),
                value: ss(env!("CARGO_PKG_VERSION")),
            },
            DetailRowItem {
                label: "Repository".into(),
                value: ss(updates::REPOSITORY_URL),
            },
            DetailRowItem {
                label: ss("Library"),
                value: ss(self.store.db_path().to_string_lossy()),
            },
            DetailRowItem {
                label: ss("Log folder"),
                value: ss(self.log_folder.to_string_lossy()),
            },
            DetailRowItem {
                label: ss("Indexed tracks"),
                value: ss(stats.total_tracks.to_string()),
            },
        ];
        let components = vec![
            AboutComponentItem {
                name: ss("Microsoft Edge WebView2 SDK"),
                description: ss("Embedded WebView interface"),
                version: ss("1.0.3179.45"),
                license: ss("[BSD-3-Clause](third-party-licenses/WebView2.txt)"),
                source: ss("WebView2"),
            },
            AboutComponentItem {
                name: ss("LibVLC"),
                description: ss("Local media playback engine and codecs included by the Windows package"),
                version: ss("3.0.24"),
                license: ss("LGPL-2.1-or-later; bundled modules and plugins may carry additional notices"),
                source: ss("VideoLAN VLC"),
            },
            AboutComponentItem {
                name: ss("LibVLCSharp"),
                description: ss(".NET binding for LibVLC"),
                version: ss("3.10.1"),
                license: ss("LGPL-2.1-or-later"),
                source: ss("LibVLCSharp"),
            },
            AboutComponentItem {
                name: ss("TagLib#"),
                description: ss("Audio metadata reading and writing"),
                version: ss("2.3.0"),
                license: ss("LGPL-2.1-or-later"),
                source: ss("TagLib#"),
            },
            AboutComponentItem {
                name: ss("Microsoft.Data.Sqlite"),
                description: ss("Local SQLite access"),
                version: ss("10.0.12"),
                license: ss("MIT"),
                source: ss("Microsoft.Data.Sqlite"),
            },
            AboutComponentItem {
                name: ss("SQLitePCLRaw"),
                description: ss("Native SQLite provider and platform binding"),
                version: ss("2.1.12 (transitive)"),
                license: ss("Apache-2.0"),
                source: ss("SQLitePCLRaw"),
            },
            AboutComponentItem {
                name: ss("SQLite"),
                description: ss("Database engine used by SQLitePCLRaw"),
                version: ss("Bundled with SQLitePCLRaw 2.1.12"),
                license: ss("Public domain"),
                source: ss("SQLite"),
            },
            AboutComponentItem {
                name: ss("Windows App SDK WinUI and dependencies"),
                description: ss("Native WinUI 3 application framework"),
                version: ss("1.8.260803003 (WinUI)"),
                license: ss("Microsoft Software License Terms"),
                source: ss("Windows App SDK"),
            },
        ];
        let releases = vec![
            ReleaseItem {
                tag: ss("v1.0.4"),
                prerelease: false,
                url: ss("https://github.com/kalabhaftu/music-player/releases/tag/v1.0.4"),
            },
            ReleaseItem {
                tag: ss("v1.0.3"),
                prerelease: false,
                url: ss("https://github.com/kalabhaftu/music-player/releases/tag/v1.0.3"),
            },
            ReleaseItem {
                tag: ss("v1.0.2"),
                prerelease: false,
                url: ss("https://github.com/kalabhaftu/music-player/releases/tag/v1.0.2"),
            },
            ReleaseItem {
                tag: ss("v1.0.1"),
                prerelease: false,
                url: ss("https://github.com/kalabhaftu/music-player/releases/tag/v1.0.1"),
            },
            ReleaseItem {
                tag: ss("v1.0.0"),
                prerelease: false,
                url: ss("https://github.com/kalabhaftu/music-player/releases/tag/v1.0.0"),
            },
        ];
        let state = self.ui.global::<State>();
        state.set_about_rows(ui_model(rows).into());
        state.set_about_components(ui_model(components).into());
        state.set_release_history(ui_model(releases).into());
    }

    // ---- View data (port of MainWindow.WebUi.Data.cs query layer) -----------

    pub(crate) fn refresh_view(&mut self) {
        let state = self.ui.global::<State>();
        let view = state.get_view().to_string();
        let search = state.get_view_search().to_string();
        let group_column = state.get_view_group_column().to_string();
        let group_value = state.get_view_group_value().to_string();
        let playlist_id = state.get_view_playlist_id().to_string();
        let sort_name = state.get_view_sort().to_string();
        let descending = state.get_view_descending();
        if view == "Search" && search != state.get_search_results_query().to_string() {
            self.tracks_offset = 0;
            self.groups_offset = 0;
            state.set_search_results_query(ss(&search));
            state.set_search_song_offset(0);
            state.set_search_album_offset(0);
            state.set_search_artist_offset(0);
            state.set_search_playlist_offset(0);
        }
        let search_song_offset = state.get_search_song_offset().max(0) as u32;
        drop(state);

        let playing = self
            .current
            .as_ref()
            .map(|t| t.path.clone())
            .unwrap_or_default();
        if view == "Playlist" {
            self.publish_playlist(&playlist_id, &playing);
            self.ui
                .global::<State>()
                .set_has_roots(!self.settings.library_roots().is_empty());
            return;
        }
        // Only list views own `State.tracks`. Grid, chrome and immersive views
        // used to pay for a 200-row query (and its artwork decodes) on every
        // tab switch even though they never render the result.
        let track_view = matches!(
            view.as_str(),
            "Songs"
                | "Favorites"
                | "Most Played"
                | "Recently Played"
                | "Recently Added"
                | "With Lyrics"
                | "Videos"
                | "Search"
                | "Album"
                | "Artist"
                | "Genre"
                | "Folder"
        );
        if !track_view {
            let total_tracks = if view == "Home" {
                self.store
                    .get_library_stats()
                    .map(|stats| stats.total_tracks)
                    .unwrap_or(0)
            } else {
                0
            };
            if view == "Home" {
                self.publish_home(&playing);
            }
            if matches!(view.as_str(), "Albums" | "Artists" | "Genres" | "Folders") {
                self.publish_groups(&view, &search);
            }
            if view == "Duplicates" && !self.duplicate_fingerprints_ready {
                self.start_duplicate_load();
            }
            let state = self.ui.global::<State>();
            state.set_has_roots(!self.settings.library_roots().is_empty());
            state.set_library_empty(view == "Home" && total_tracks == 0);
            state.set_page_loading(false);
            return;
        }
        let hide_dupes = self
            .settings
            .bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        let video_extensions = self.video_extensions();
        let mut query = TrackQuery {
            favorites: view == "Favorites",
            most_played: view == "Most Played",
            recently_played: view == "Recently Played",
            with_lyrics: view == "With Lyrics",
            search: if view == "Search" && !search.trim().is_empty() {
                Some(search.clone())
            } else {
                None
            },
            group_column: if matches!(view.as_str(), "Album" | "Artist" | "Genre" | "Folder") {
                Some(group_column.clone())
            } else {
                None
            },
            group_value: if matches!(view.as_str(), "Album" | "Artist" | "Genre" | "Folder") {
                Some(group_value.clone())
            } else {
                None
            },
            extension_filter: if view == "Videos" {
                Some(if video_extensions.is_empty() {
                    vec![".__no_video_files__".into()]
                } else {
                    video_extensions
                })
            } else {
                None
            },
            hide_duplicates: hide_dupes && matches!(view.as_str(), "Songs" | "Search"),
            sort: song_sort(
                if view == "Search" {
                    "Title"
                } else {
                    &sort_name
                },
                if view == "Search" {
                    TrackSort::Title
                } else {
                    song_sort(&default_sort_for_view(&view).0, TrackSort::Title)
                },
            ),
            descending: view != "Search" && descending,
            offset: if view == "Search" {
                search_song_offset
            } else {
                0
            },
            page_size: if view == "Search" {
                40
            } else {
                (self.tracks_offset + TRACK_PAGE_SIZE).min(u32::MAX as usize) as u32
            },
            ..TrackQuery::default()
        };
        let (mut tracks, total) = self.store.query_track_page(&query).unwrap_or_default();
        if view == "Search" {
            let last_offset = if total > 0 {
                (((total - 1) / 40) * 40) as u32
            } else {
                0
            };
            if search_song_offset > last_offset {
                query.offset = last_offset;
                self.ui
                    .global::<State>()
                    .set_search_song_offset(last_offset as i32);
                tracks = self.store.query_track_page(&query).unwrap_or_default().0;
            }
        }
        let items: Vec<TrackItem> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| self.track_to_ui(t, (i + 1 + query.offset as usize) as i32, &playing))
            .collect();
        self.view_paths = tracks.iter().map(|t| t.path.clone()).collect();
        self.id_to_path.clear();
        for t in &tracks {
            self.id_to_path.insert(t.id(), t.path.clone());
            self.id_to_path.insert(t.path.clone(), t.path.clone());
            self.track_cache.insert(t.path.clone(), t.clone());
        }

        let state = self.ui.global::<State>();
        state.set_tracks(ui_model(items).into());
        state.set_tracks_total(total as i32);
        state.set_can_load_more(
            view != "Search" && self.tracks_offset + TRACK_PAGE_SIZE < total as usize,
        );
        state.set_view_title(ss(&view));
        state.set_view_subtitle(ss(format!(
            "{} {}",
            fmt_count(total as u32),
            if view == "Videos" { "videos" } else { "tracks" }
        )));
        state.set_has_roots(!self.settings.library_roots().is_empty());
        state.set_library_empty(view == "Home" && total == 0);
        state.set_page_loading(false);

        // Home sections.
        if view == "Home" {
            self.publish_home(&playing);
        }
        if matches!(view.as_str(), "Albums" | "Artists" | "Genres" | "Folders") {
            self.publish_groups(&view, &search);
        }
        if view == "Playlist" {
            self.publish_playlist(&playlist_id, &playing);
        }
        if matches!(view.as_str(), "Album" | "Artist" | "Genre" | "Folder") {
            self.publish_detail_hero(&view, &group_value);
        }
        if view == "Search" {
            self.publish_search_counts(&search);
        }
        if view == "Duplicates" && !self.duplicate_fingerprints_ready {
            self.start_duplicate_load();
        }
    }

    pub(crate) fn publish_home(&mut self, playing: &str) {
        let greeting = match chrono::Local::now().hour() {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        };
        let hide_dupes = self
            .settings
            .bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        let recent = self
            .store
            .query_track_page(&TrackQuery {
                recently_played: true,
                hide_duplicates: hide_dupes,
                sort: TrackSort::LastPlayed,
                descending: true,
                page_size: 8,
                ..TrackQuery::default()
            })
            .map(|(tracks, _)| tracks)
            .unwrap_or_default();
        let mut quick = self
            .store
            .query_track_page(&TrackQuery {
                favorites: true,
                hide_duplicates: hide_dupes,
                sort: TrackSort::Added,
                descending: true,
                page_size: 8,
                ..TrackQuery::default()
            })
            .map(|(tracks, _)| tracks)
            .unwrap_or_default();
        if quick.is_empty() {
            quick = recent.clone();
        }
        if quick.is_empty() {
            quick = self
                .store
                .query_track_page(&TrackQuery {
                    hide_duplicates: hide_dupes,
                    sort: TrackSort::Added,
                    descending: true,
                    page_size: 8,
                    ..TrackQuery::default()
                })
                .map(|(tracks, _)| tracks)
                .unwrap_or_default();
        }

        let total_tracks = self
            .store
            .get_library_stats()
            .map(|stats| stats.total_tracks)
            .unwrap_or(0);
        let subtitle = if !recent.is_empty() {
            "Pick up where you left off.".to_string()
        } else if total_tracks > 0 {
            format!("{} tracks in your local library", fmt_count(total_tracks))
        } else {
            "Your music library".to_string()
        };
        let quick_items: Vec<TrackItem> = quick
            .iter()
            .take(6)
            .enumerate()
            .map(|(i, track)| self.track_to_ui(track, i as i32 + 1, playing))
            .collect();
        let mut album_seen = HashSet::new();
        let recent_album_items: Vec<HomeCardItem> = recent
            .iter()
            .filter(|track| !track.album.trim().is_empty())
            .filter(|track| {
                album_seen.insert((
                    track.album.clone(),
                    if track.album_artist.trim().is_empty() {
                        track.artist.clone()
                    } else {
                        track.album_artist.clone()
                    },
                ))
            })
            .take(5)
            .map(|track| {
                let title = track.album.trim();
                let artist = if track.album_artist.trim().is_empty() {
                    &track.artist
                } else {
                    &track.album_artist
                };
                let subtitle = if track.year > 0 {
                    format!("{artist} • {}", track.year)
                } else {
                    artist.to_string()
                };
                self.home_track_card(track, title, &subtitle, "album", playing)
            })
            .collect();

        let playlists = self.store.get_playlists().unwrap_or_default();
        let mut library_cards: Vec<HomeCardItem> = Vec::new();
        let (library_title, library_view) = if !playlists.is_empty() {
            library_cards.extend(playlists.iter().take(5).map(|playlist| HomeCardItem {
                id: ss(&playlist.id),
                title: ss(&playlist.name),
                subtitle: ss(format!(
                    "{} song{}",
                    playlist.track_count,
                    if playlist.track_count == 1 { "" } else { "s" }
                )),
                kind: ss("playlist"),
                gradient: gradient_slot(&playlist.name),
                initials: ss(initials(&playlist.name)),
                has_cover: false,
                cover: Image::default(),
            }));
            ("Made from your library", "Playlists")
        } else {
            let (album_groups, _) = self
                .store
                .query_group_page("album", None, 0, 5, hide_dupes)
                .unwrap_or_default();
            if !album_groups.is_empty() {
                library_cards.extend(album_groups.iter().map(|group| self.home_group_card(group)));
                ("Made from your library", "Albums")
            } else if !recent.is_empty() {
                library_cards.extend(recent.iter().take(5).map(|track| {
                    self.home_track_card(track, &track.title, &track.artist, "track", playing)
                }));
                ("Recently played", "Recently Played")
            } else {
                ("Made from your library", "Playlists")
            }
        };

        let state = self.ui.global::<State>();
        state.set_home_greeting(ss(greeting));
        state.set_home_subtitle(ss(subtitle));
        state.set_home_quick_tracks(ui_model(quick_items.clone()).into());
        state.set_home_recent_albums(ui_model(recent_album_items.clone()).into());
        state.set_home_library_cards(ui_model(library_cards.clone()).into());
        state.set_home_quick_rows(ui_model(chunk_home_tracks(quick_items)).into());
        state.set_home_recent_album_rows(ui_model(chunk_home_cards(recent_album_items, 5)).into());
        state.set_home_library_card_rows(ui_model(chunk_home_cards(library_cards, 5)).into());
        state.set_home_library_title(ss(library_title));
        state.set_home_library_view(ss(library_view));
    }

    pub(crate) fn home_track_card(
        &mut self,
        track: &Track,
        title: &str,
        subtitle: &str,
        kind: &str,
        playing: &str,
    ) -> HomeCardItem {
        let item = self.track_to_ui(track, 0, playing);
        HomeCardItem {
            id: item.id,
            title: ss(title),
            subtitle: ss(subtitle),
            kind: ss(kind),
            gradient: gradient_slot(if track.album.is_empty() {
                title
            } else {
                &track.album
            }),
            initials: ss(initials(title)),
            has_cover: item.has_cover,
            cover: item.cover,
        }
    }

    pub(crate) fn home_group_card(&self, group: &music_player_core::LibraryGroup) -> HomeCardItem {
        let item = group_to_ui(group);
        HomeCardItem {
            id: ss(&group.name),
            title: item.name,
            subtitle: item.subtitle,
            kind: ss("album"),
            gradient: item.gradient,
            initials: item.initials,
            has_cover: item.has_cover,
            cover: item.cover,
        }
    }

    pub(crate) fn publish_groups(&mut self, view: &str, search: &str) {
        let hide_dupes = self
            .settings
            .bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        let column = match view {
            "Albums" => "album",
            "Artists" => "artist",
            "Genres" => "genre",
            "Folders" => "folder",
            "Search" => "album",
            _ => "album",
        };
        let (groups, total) = self
            .store
            .query_group_page(
                column,
                if view == "Search" { Some(search) } else { None },
                self.groups_offset as u32,
                60,
                hide_dupes,
            )
            .unwrap_or_default();
        self.group_total = total;
        let items: Vec<GroupItem> = groups.iter().map(group_to_ui).collect();
        if self.groups_offset == 0 {
            self.group_items.clear();
        }
        self.group_items.extend(items);
        let rows = chunk_groups(self.group_items.clone());
        let state = self.ui.global::<State>();
        state.set_groups(ui_model(self.group_items.clone()).into());
        state.set_group_rows(ui_model(rows).into());
        state.set_groups_total(total as i32);
    }

    pub(crate) fn publish_search_counts(&mut self, search: &str) {
        let hide_dupes = self
            .settings
            .bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        let state = self.ui.global::<State>();
        let mut album_offset = state.get_search_album_offset().max(0) as u32;
        let mut artist_offset = state.get_search_artist_offset().max(0) as u32;
        let mut playlist_offset = state.get_search_playlist_offset().max(0) as usize;
        drop(state);
        let (mut album_groups, albums) = if search.trim().is_empty() {
            (Vec::new(), 0)
        } else {
            self.store
                .query_group_page("album", Some(search), album_offset, 24, hide_dupes)
                .unwrap_or_default()
        };
        let album_last_offset = if albums > 0 {
            (((albums - 1) / 24) * 24) as u32
        } else {
            0
        };
        if album_offset > album_last_offset {
            album_offset = album_last_offset;
            if !search.trim().is_empty() {
                if let Ok((groups, _)) =
                    self.store
                        .query_group_page("album", Some(search), album_offset, 24, hide_dupes)
                {
                    album_groups = groups;
                }
            }
            self.ui
                .global::<State>()
                .set_search_album_offset(album_offset as i32);
        }
        let (mut artist_groups, artists) = if search.trim().is_empty() {
            (Vec::new(), 0)
        } else {
            self.store
                .query_group_page("artist", Some(search), artist_offset, 24, hide_dupes)
                .unwrap_or_default()
        };
        let artist_last_offset = if artists > 0 {
            (((artists - 1) / 24) * 24) as u32
        } else {
            0
        };
        if artist_offset > artist_last_offset {
            artist_offset = artist_last_offset;
            if !search.trim().is_empty() {
                if let Ok((groups, _)) = self.store.query_group_page(
                    "artist",
                    Some(search),
                    artist_offset,
                    24,
                    hide_dupes,
                ) {
                    artist_groups = groups;
                }
            }
            self.ui
                .global::<State>()
                .set_search_artist_offset(artist_offset as i32);
        }
        let (_, songs) = if search.trim().is_empty() {
            (Vec::new(), 0)
        } else {
            self.store
                .query_track_page(&TrackQuery {
                    search: Some(search.into()),
                    hide_duplicates: hide_dupes,
                    sort: TrackSort::Title,
                    descending: false,
                    page_size: 1,
                    ..TrackQuery::default()
                })
                .unwrap_or_default()
        };
        let matching_playlists: Vec<PlaylistCardItem> = if search.trim().is_empty() {
            Vec::new()
        } else {
            self.playlists
                .iter()
                .filter(|playlist| {
                    search
                        .split_whitespace()
                        .all(|term| playlist.name.to_lowercase().contains(&term.to_lowercase()))
                })
                .map(|playlist| PlaylistCardItem {
                    id: ss(&playlist.id),
                    name: ss(&playlist.name),
                    track_count: playlist.track_count as i32,
                    is_smart: false,
                    gradient: gradient_slot(&playlist.name),
                    initials: initials(&playlist.name).into(),
                    description: ss(""),
                })
                .collect()
        };
        let playlist_count = matching_playlists.len();
        let playlist_last_offset = if playlist_count > 0 {
            ((playlist_count - 1) / 20) * 20
        } else {
            0
        };
        if playlist_offset > playlist_last_offset {
            playlist_offset = playlist_last_offset;
            self.ui
                .global::<State>()
                .set_search_playlist_offset(playlist_offset as i32);
        }
        let search_playlists = matching_playlists
            .into_iter()
            .skip(playlist_offset)
            .take(20)
            .collect::<Vec<_>>();
        let state = self.ui.global::<State>();
        state.set_search_albums(
            ui_model(album_groups.iter().map(group_to_ui).collect::<Vec<_>>()).into(),
        );
        state.set_search_artists(
            ui_model(artist_groups.iter().map(group_to_ui).collect::<Vec<_>>()).into(),
        );
        state.set_search_playlists(ui_model(search_playlists).into());
        state.set_search_song_count(songs as i32);
        state.set_search_album_count(albums as i32);
        state.set_search_artist_count(artists as i32);
        state.set_search_playlist_count(playlist_count as i32);
    }

    pub(crate) fn publish_playlist(&mut self, playlist_id: &str, playing: &str) {
        self.id_to_path.clear();
        let Some(playlist) = self.store.get_playlist(playlist_id).ok().flatten() else {
            let state = self.ui.global::<State>();
            state.set_playlist_name(ss("Playlist not found."));
            state.set_playlist_track_total(0);
            state.set_playlist_empty_message(ss("This playlist is empty."));
            state.set_tracks(ui_model(Vec::<TrackItem>::new()).into());
            state.set_tracks_total(0);
            state.set_can_load_more(false);
            state.set_page_loading(false);
            state.set_view_title(ss("Playlist not found."));
            state.set_view_subtitle(ss(""));
            return;
        };
        let search = self
            .ui
            .global::<State>()
            .get_view_search()
            .to_string();
        let offset = self.tracks_offset.min(u32::MAX as usize) as u32;
        let page_size = TRACK_PAGE_SIZE.min(u32::MAX as usize) as u32;
        let (entries, total) = self
            .store
            .get_playlist_entries_page(playlist_id, Some(&search), offset, page_size)
            .unwrap_or_default();
        let items: Vec<TrackItem> = entries
            .iter()
            .map(|entry| {
                let mut item = match &entry.track {
                    Some(track) => self.track_to_ui(track, entry.position as i32 + 1, playing),
                    None => self.placeholder_item(&entry.path, playing),
                };
                item.position = entry.position as i32;
                if let Some(track) = &entry.track {
                    self.track_cache.insert(entry.path.clone(), track.clone());
                }
                item
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_playlist_tracks(ui_model(items.clone()).into());
        state.set_playlist_name(ss(&playlist.name));
        state.set_playlist_track_total(total as i32);
        state.set_playlist_empty_message(ss(if total == 0 && !search.trim().is_empty() {
            "No tracks match this search. Try another title, artist, or album."
        } else if total == 0 {
            "This playlist is empty. Add tracks from your library to get started."
        } else {
            ""
        }));
        state.set_tracks(ui_model(items).into());
        state.set_tracks_total(total as i32);
        state.set_can_load_more(offset as usize + (page_size as usize) < total as usize);
        state.set_view_title(ss(&playlist.name));
        state.set_view_subtitle(ss(format!(
            "{} {}tracks",
            total,
            if search.trim().is_empty() { "" } else { "matching " }
        )));
        state.set_page_loading(false);
        self.view_paths = self
            .store
            .get_playlist_paths(playlist_id)
            .unwrap_or_default();
    }

    pub(crate) fn publish_detail_hero(&mut self, view: &str, value: &str) {
        let mut subtitle = String::new();
        let mut cover = Image::default();
        let mut has_cover = false;
        let column = match view {
            "Album" => "album",
            "Artist" => "artist",
            "Genre" => "genre",
            _ => "folder",
        };
        let group_title = if column == "folder" {
            Path::new(value)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(value)
                .to_string()
        } else {
            value.to_string()
        };
        let hide_dupes = self
            .settings
            .bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        if let Ok((groups, _)) = self
            .store
            .query_group_page(column, Some(value), 0, 10, hide_dupes)
        {
            if let Some(g) = groups.iter().find(|g| g.name.eq_ignore_ascii_case(value)) {
                subtitle = if column == "folder" {
                    format!(
                        "{} · {} track{}",
                        value,
                        g.track_count,
                        if g.track_count == 1 { "" } else { "s" }
                    )
                } else {
                    format!(
                        "{} song{}",
                        g.track_count,
                        if g.track_count == 1 { "" } else { "s" }
                    )
                };
                if let Some(art) = &g.artwork_path {
                    if let Some(image) = native_io::load_artwork_image(Path::new(art)) {
                        cover = image;
                        has_cover = true;
                    }
                }
            } else if column == "folder" {
                subtitle = value.to_string();
            }
        }
        let mut albums = Vec::new();
        let mut albums_have_more = false;
        if column == "artist" {
            if let Ok((groups, total)) =
                self.store
                    .query_group_page("album", Some(value), 0, 30, hide_dupes)
            {
                albums = groups.iter().map(group_to_ui).collect();
                albums_have_more = total > albums.len() as i64;
            }
        }
        self.artist_album_items = albums.clone();
        let state = self.ui.global::<State>();
        state.set_view_group_title(ss(&group_title));
        state.set_hero_subtitle(ss(subtitle));
        state.set_hero_cover(cover);
        state.set_hero_has_cover(has_cover);
        state.set_artist_albums(ui_model(albums).into());
        state.set_artist_albums_have_more(albums_have_more);
    }

    pub(crate) fn load_more_artist_albums(&mut self) {
        let state = self.ui.global::<State>();
        let artist = state.get_view_group_value().to_string();
        if artist.is_empty() || !state.get_artist_albums_have_more() {
            return;
        }
        let offset = self.artist_album_items.len() as u32;
        let hide_dupes = self
            .settings
            .bool_or(music_player_core::settings::keys::HIDE_DUPLICATES, true);
        let Ok((groups, total)) = self
            .store
            .query_group_page("album", Some(&artist), offset, 30, hide_dupes)
        else {
            return;
        };
        self.artist_album_items
            .extend(groups.iter().map(group_to_ui));
        state.set_artist_albums(ui_model(self.artist_album_items.clone()).into());
        state.set_artist_albums_have_more(total > self.artist_album_items.len() as i64);
    }

    // ---- Lyrics -------------------------------------------------------------

    pub(crate) fn load_lyrics(&mut self, track: &Track) {
        self.lyrics_target = Some(track.path.clone());
        self.lyrics_doc = None;
        self.lyrics_timed = false;
        self.lyrics_active = -1;
        self.auto_lyrics_rx = None;

        let (text, source) = if let Some(sidecar) = read_sidecar(&track.path) {
            (Some(sidecar), "sidecar")
        } else if let Some(embedded) = read_tags(&track.path).lyrics {
            (Some(embedded), "embedded")
        } else {
            (None, "none")
        };
        let mut lines = Vec::new();
        let has_text = text.is_some();
        if let Some(ref text_val) = text {
            let doc = parse_lrc(text_val);
            self.lyrics_timed = !doc.lines.is_empty();
            for line in &doc.lines {
                lines.push(LyricLineItem {
                    text: ss(&line.text),
                    is_active: false,
                    is_past: false,
                    is_upcoming: true,
                });
            }
            if !self.lyrics_timed {
                for plain in text_val.lines().filter(|l| !l.trim().is_empty()) {
                    lines.push(LyricLineItem {
                        text: ss(plain),
                        is_active: false,
                        is_past: false,
                        is_upcoming: true,
                    });
                }
            }
            self.lyrics_doc = Some(doc);
        }
        let state = self.ui.global::<State>();
        state.set_lyrics_lines(ui_model(lines).into());
        let has_local_lyrics = !self
            .lyrics_doc
            .as_ref()
            .map(|d| d.lines.is_empty())
            .unwrap_or(true)
            || has_text;
        state.set_has_lyrics(has_local_lyrics);
        state.set_lyrics_source(ss(source));
        state.set_lyrics_timed(self.lyrics_timed);
        state.set_lyrics_active_line(-1);

        // If local lyrics are already timed, no need to query online.
        if self.lyrics_timed {
            state.set_lyrics_searching(false);
            return;
        }

        // Check if auto-load is enabled
        use music_player_core::settings::keys as k;
        if !self.settings.bool_or(k::AUTO_LOAD_LYRICS, true) {
            state.set_lyrics_searching(false);
            return;
        }

        // Check in-memory cache
        let now = Instant::now();
        if let Some((cached_at, cached_match)) = self.lyrics_cache.get(&track.path) {
            let duration = if cached_match.as_ref().map(|m| m.timed).unwrap_or(false) {
                std::time::Duration::from_secs(30 * 60)
            } else {
                std::time::Duration::from_secs(5 * 60)
            };
            if now.duration_since(*cached_at) < duration {
                if let Some(matched) = cached_match {
                    if !has_local_lyrics || matched.timed {
                        let doc = parse_lrc(&matched.raw);
                        self.lyrics_timed = matched.timed;
                        let mut l_lines = Vec::new();
                        for line in &doc.lines {
                            l_lines.push(LyricLineItem {
                                text: ss(&line.text),
                                is_active: false,
                                is_past: false,
                                is_upcoming: true,
                            });
                        }
                        if !self.lyrics_timed {
                            for plain in matched.raw.lines().filter(|l| !l.trim().is_empty()) {
                                l_lines.push(LyricLineItem {
                                    text: ss(plain),
                                    is_active: false,
                                    is_past: false,
                                    is_upcoming: true,
                                });
                            }
                        }
                        self.lyrics_doc = Some(doc);
                        state.set_lyrics_lines(ui_model(l_lines).into());
                        state.set_has_lyrics(true);
                        state.set_lyrics_source(ss("lrclib"));
                        state.set_lyrics_timed(matched.timed);
                        state.set_lyrics_searching(false);
                        return;
                    }
                } else if !has_local_lyrics {
                    state.set_lyrics_searching(false);
                    return;
                }
            }
        }

        // Spawning background fetch for LRCLIB lyrics
        state.set_lyrics_searching(true);
        let track_path = track.path.clone();
        let track_title = track.title.clone();
        let track_artist = track.artist.clone();
        let track_duration = (track.duration_ms as f64) / 1000.0;
        let (tx, rx) = std::sync::mpsc::channel();
        self.auto_lyrics_rx = Some(rx);

        std::thread::spawn(move || {
            let mut client = music_player_core::lyrics::LrclibSearchClient::new();
            let result = client.search(&track_title, &track_artist).map(|results| {
                music_player_core::lyrics::select_lyrics_match(
                    &track_title,
                    &track_artist,
                    track_duration,
                    &results,
                )
            });
            let _ = tx.send((track_path, result));
        });
    }

    pub(crate) fn update_lyrics_position(&mut self, position_ms: u64) {
        if !self.lyrics_timed {
            return;
        }
        let Some(doc) = &self.lyrics_doc else { return };
        let mut active = -1i32;
        for (i, line) in doc.lines.iter().enumerate() {
            if line.time_ms <= position_ms {
                active = i as i32;
            } else {
                break;
            }
        }
        if active == self.lyrics_active {
            return;
        }
        self.lyrics_active = active;
        let timed = self.lyrics_timed;
        let lines: Vec<LyricLineItem> = doc
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| LyricLineItem {
                text: ss(&line.text),
                is_active: i as i32 == active,
                is_past: (i as i32) < active,
                is_upcoming: (i as i32) > active,
            })
            .collect();
        let state = self.ui.global::<State>();
        state.set_lyrics_lines(ui_model(lines).into());
        state.set_lyrics_active_line(active);
        let _ = timed;
    }

    pub(crate) fn poll_lyrics_search(&mut self) {
        if let Some(rx) = &self.lyrics_rx {
            if let Ok(result) = rx.try_recv() {
                self.lyrics_rx = None;
                let state = self.ui.global::<State>();
                state.set_lyrics_searching(false);
                match result {
                    Ok(results) => {
                        state.set_lyrics_search_status(ss(if results.is_empty() {
                            "No matching lyrics were found."
                        } else {
                            ""
                        }));
                        let items: Vec<LyricResultItem> = results
                            .iter()
                            .map(|r| LyricResultItem {
                                title: ss(&r.track_name),
                                artist: ss(&r.artist_name),
                                badge: ss(if r.synced_lyrics.is_some() {
                                    "Timed"
                                } else {
                                    "Plain"
                                }),
                                timed: r.synced_lyrics.is_some(),
                                synced_id: ss(r.id.to_string()),
                                plain_id: ss(r.id.to_string()),
                            })
                            .collect();
                        state.set_lyrics_results(ui_model(items).into());
                        self.lyrics_results = results;
                    }
                    Err(message) => state.set_lyrics_search_status(ss(message)),
                }
            }
        }

        if let Some(rx) = &self.auto_lyrics_rx {
            if let Ok((path, result)) = rx.try_recv() {
                self.auto_lyrics_rx = None;
                let matched_opt = result.as_ref().ok().cloned().flatten();
                self.lyrics_cache
                    .insert(path.clone(), (Instant::now(), matched_opt.clone()));

                // Only apply if the track being viewed / played is still this track
                if self.lyrics_target.as_deref() == Some(&path) {
                    let state = self.ui.global::<State>();
                    state.set_lyrics_searching(false);

                    if let Some(matched) = matched_opt {
                        // If we already have timed local lyrics, do not overwrite
                        if !self.lyrics_timed {
                            let doc = parse_lrc(&matched.raw);
                            self.lyrics_timed = matched.timed;
                            let mut lines = Vec::new();
                            for line in &doc.lines {
                                lines.push(LyricLineItem {
                                    text: ss(&line.text),
                                    is_active: false,
                                    is_past: false,
                                    is_upcoming: true,
                                });
                            }
                            if !self.lyrics_timed {
                                for plain in matched.raw.lines().filter(|l| !l.trim().is_empty()) {
                                    lines.push(LyricLineItem {
                                        text: ss(plain),
                                        is_active: false,
                                        is_past: false,
                                        is_upcoming: true,
                                    });
                                }
                            }
                            self.lyrics_doc = Some(doc);
                            state.set_lyrics_lines(ui_model(lines).into());
                            state.set_has_lyrics(true);
                            state.set_lyrics_source(ss("lrclib"));
                            state.set_lyrics_timed(matched.timed);
                            state.set_lyrics_active_line(-1);

                            // Sync position if audio is currently playing this track
                            let position_ms = self.engine.position_ms();
                            if position_ms > 0 {
                                self.update_lyrics_position(position_ms);
                            }
                        }
                    } else if !state.get_has_lyrics() {
                        state.set_has_lyrics(false);
                        state.set_lyrics_source(ss("none"));
                    }
                }
            }
        }
    }

    // ---- Duplicates ---------------------------------------------------------

    pub(crate) fn start_duplicate_load(&mut self) {
        if self.duplicate_prepare_rx.is_some() {
            return;
        }
        if self.duplicate_fingerprints_ready {
            self.load_duplicates(&self.dup_sort.clone(), self.dup_desc, 0);
            return;
        }
        self.ui.global::<State>().set_duplicate_loading(true);
        let store = self.store.clone();
        let (tx, rx) = mpsc::channel();
        self.duplicate_prepare_rx = Some(rx);
        std::thread::spawn(move || {
            let result = store
                .prepare_duplicate_fingerprints()
                .map(|_| ())
                .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
    }

    pub(crate) fn poll_duplicate_load(&mut self) {
        let Some(rx) = &self.duplicate_prepare_rx else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("duplicate scan stopped unexpectedly".into()),
        };
        self.duplicate_prepare_rx = None;
        self.ui.global::<State>().set_duplicate_loading(false);
        match result {
            Ok(()) => {
                self.duplicate_fingerprints_ready = true;
                let sort = self.dup_sort.clone();
                self.load_duplicates(&sort, self.dup_desc, 0);
            }
            Err(error) => {
                LocalAppLog::shared().error("duplicates", "fingerprint failed", Some(&error));
                self.show_notice("Duplicate groups could not be loaded.", NoticeSeverity::Error);
            }
        }
    }

    pub(crate) fn load_duplicates(&mut self, sort: &str, descending: bool, page: usize) {
        if !self.duplicate_fingerprints_ready && self.duplicate_prepare_rx.is_some() {
            return;
        }
        self.dup_sort = sort.to_string();
        self.dup_desc = descending;
        let page_size = 100usize;
        let search = self.ui.global::<State>().get_view_search().to_string();
        let mut groups = self
            .store
            .get_duplicate_groups(0, 10000)
            .unwrap_or_default();
        if !search.trim().is_empty() {
            let needle = search.trim().to_lowercase();
            groups.retain(|group| {
                let track = &group.representative;
                [
                    track.title.as_str(),
                    track.artist.as_str(),
                    track.album.as_str(),
                    track.genre.as_str(),
                    track.path.as_str(),
                ]
                .iter()
                .any(|field| field.to_lowercase().contains(&needle))
            });
        }
        groups.sort_by(|left, right| {
            let left_track = &left.representative;
            let right_track = &right.representative;
            let compare_text = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
            match sort.to_ascii_lowercase().as_str() {
                "artist" => compare_text(&left_track.artist, &right_track.artist),
                "album" => compare_text(&left_track.album, &right_track.album),
                "year" => left_track.year.cmp(&right_track.year),
                "added" => left_track.added_utc.cmp(&right_track.added_utc),
                "duration" => left_track.duration_ms.cmp(&right_track.duration_ms),
                "playcount" => left_track.play_count.cmp(&right_track.play_count),
                "lastplayed" => left_track.last_played_utc.cmp(&right_track.last_played_utc),
                "path" => compare_text(&left_track.path, &right_track.path),
                "rating" => left_track.rating.cmp(&right_track.rating),
                _ => compare_text(&left_track.title, &right_track.title),
            }
            .then_with(|| compare_text(&left_track.title, &right_track.title))
            .then_with(|| left_track.path.cmp(&right_track.path))
        });
        if descending {
            groups.reverse();
        }
        let total = groups.len();
        let pages = ((total + page_size - 1) / page_size).max(1);
        let page = page.min(pages - 1);
        let playing = self
            .current
            .as_ref()
            .map(|t| t.path.clone())
            .unwrap_or_default();
        let items: Vec<DuplicateItem> = groups
            .iter()
            .skip(page * page_size)
            .take(page_size)
            .map(|g| self.duplicate_to_ui(g, &playing))
            .collect();
        let state = self.ui.global::<State>();
        state.set_duplicates(ui_model(items).into());
        state.set_duplicates_total(total as i32);
        state.set_duplicates_page(page as i32);
        state.set_duplicates_pages(pages as i32);
        state.set_duplicates_sort(ss(sort));
        state.set_duplicates_descending(descending);
        state.set_duplicate_loading(false);
    }

    pub(crate) fn duplicate_to_ui(
        &mut self,
        group: &ExactDuplicateGroup,
        _playing: &str,
    ) -> DuplicateItem {
        let id = group.representative.id();
        self.id_to_path
            .insert(id.clone(), group.representative.path.clone());
        DuplicateItem {
            title: ss(&group.representative.title),
            artist: ss(&group.representative.artist),
            copies: group.copy_count as i32,
            locations: ss(format!("{} copies", group.copy_count)),
            first_id: ss(id),
        }
    }

    pub(crate) fn open_duplicate_files(&mut self, id: &str) {
        let Some(path) = self.id_to_path.get(id).cloned() else {
            return;
        };
        let Some(group) = self
            .store
            .get_duplicate_groups(0, 10000)
            .ok()
            .and_then(|groups| groups.into_iter().find(|g| g.representative.path == path))
        else {
            return;
        };
        let Ok(files) = self.store.get_duplicate_tracks(&group.sha256) else {
            return;
        };
        let state = self.ui.global::<State>();
        let paths: Vec<SharedString> = files.iter().map(|file| ss(&file.path)).collect();
        state.set_duplicate_locations(ui_model(paths).into());
        state.set_duplicate_locations_total(files.len() as i32);
        self.open_modal("duplicate-locations");
    }

    // ---- Toasts (C# 4-second InfoBar notice) -------------------------------

    pub(crate) fn show_notice(&mut self, message: impl Into<String>, severity: NoticeSeverity) {
        let severity_text = match severity {
            NoticeSeverity::Informational => "Informational",
            NoticeSeverity::Warning => "Warning",
            NoticeSeverity::Error => "Error",
            NoticeSeverity::Success => "Success",
        };
        let state = self.ui.global::<State>();
        state.set_notice_message(ss(message));
        state.set_notice_severity(ss(severity_text));
        state.set_notice_visible(true);
        let weak = self.ui.as_weak();
        self.notice_timer
            .start(TimerMode::SingleShot, Duration::from_secs(4), move || {
                if let Some(ui) = weak.upgrade() {
                    ui.global::<State>().set_notice_visible(false);
                }
            });
    }

    // ---- Navigation ---------------------------------------------------------

    /// Restores the JSON `last-view-context` payload written by navigation
    /// (1:1 with `LibraryQueryService.RestoreContext`).
    pub(crate) fn restore_view_context(&mut self) {
        use music_player_core::settings::keys::LAST_VIEW_CONTEXT;

        const VIEWS: &[&str] = &[
            "Home",
            "Search",
            "Songs",
            "Albums",
            "Artists",
            "Genres",
            "Folders",
            "Favorites",
            "Most Played",
            "Recently Played",
            "Recently Added",
            "Playlists",
            "Playlist",
            "Album",
            "Artist",
            "Genre",
            "Folder",
            "With Lyrics",
            "Videos",
            "Queue",
            "Audio",
            "Settings",
            "Duplicates",
            "Lyrics",
            "Now Playing",
        ];

        let Some(serialized) = self.settings.raw(LAST_VIEW_CONTEXT) else {
            return;
        };
        if serialized.trim().is_empty() || serialized.len() > 8192 {
            return;
        }
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(&serialized) else {
            LocalAppLog::shared().warning(
                "navigation",
                "The saved player view could not be restored; opening Home instead.",
            );
            return;
        };
        let Some(object) = payload.as_object() else {
            return;
        };
        let text = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let mut view = text("view");
        if view.is_empty() {
            return;
        }
        if !VIEWS.contains(&view.as_str()) {
            LocalAppLog::shared().warning(
                "navigation",
                "The saved player view is unknown; opening Home instead.",
            );
            view = "Home".into();
        }
        let mut search = text("search");
        let group_value = text("groupValue");
        let mut playlist_id = text("playlistId");

        if view == "Playlist"
            && (playlist_id.trim().is_empty()
                || !self
                    .playlists
                    .iter()
                    .any(|playlist| playlist.id == playlist_id))
        {
            view = "Playlists".into();
            playlist_id.clear();
        }
        if view == "Videos" && self.video_extensions().is_empty() {
            view = "Playlists".into();
        }
        if view != "Search" {
            search.clear();
        }

        let group_column = match view.as_str() {
            "Album" => "album",
            "Artist" => "artist",
            "Genre" => "genre",
            "Folder" => "folder",
            _ => "",
        };
        let state = self.ui.global::<State>();
        state.set_view(ss(&view));
        state.set_view_search(ss(&search));
        state.set_view_group_column(ss(group_column));
        state.set_view_group_value(ss(if group_column.is_empty() {
            ""
        } else {
            &group_value
        }));
        state.set_view_playlist_id(ss(&playlist_id));
        let (sort, descending) = if group_column.is_empty() {
            default_sort_for_view(&view)
        } else {
            detail_sort_for_column(group_column)
        };
        state.set_view_sort(ss(sort));
        state.set_view_descending(descending);
        drop(state);

        if view != text("view") {
            self.persist_view_context();
        }
    }

    /// Persists the full navigation context as JSON, matching the C# payload.
    pub(crate) fn persist_view_context(&self) {
        use music_player_core::settings::keys::LAST_VIEW_CONTEXT;

        let state = self.ui.global::<State>();
        let context = NavContext::snapshot(&state);
        drop(state);
        let nullable = |value: &str| {
            if value.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(value.to_string())
            }
        };
        let payload = json!({
            "view": context.view,
            "search": context.search,
            "groupColumn": nullable(&context.group_column),
            "groupValue": nullable(&context.group_value),
            "playlistId": nullable(&context.playlist_id),
        });
        self.settings
            .set_raw(LAST_VIEW_CONTEXT, &payload.to_string());
    }
}
