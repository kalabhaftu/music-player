use crate::*;

impl App {
    pub(crate) fn set_view(&mut self, view: &str, search: &str, group: &str, playlist_id: &str) {
        let (resolved_search, resolved_group) =
            if matches!(view, "Album" | "Artist" | "Genre" | "Folder") {
                if group.is_empty() && !search.is_empty() {
                    ("", search)
                } else if matches!(group, "album" | "artist" | "genre" | "folder")
                    && !search.is_empty()
                    && search != group
                {
                    ("", search)
                } else {
                    (search, group)
                }
            } else {
                (search, group)
            };
        let state = self.ui.global::<State>();
        let current = NavContext::snapshot(&state);
        if current.view != view
            || current.search != resolved_search
            || current.group_value != resolved_group
            || current.playlist_id != playlist_id
        {
            self.history.truncate(self.history_pos + 1);
            self.history.push(current);
            if self.history.len() > 50 {
                self.history.remove(0);
            }
            self.history_pos = self.history.len();
        }
        self.tracks_offset = 0;
        self.groups_offset = 0;
        self.query_generation = self.query_generation.wrapping_add(1);
        if view != "Search" {
            self.search_debounce = None;
        }
        state.set_view(ss(view));
        state.set_view_search(ss(resolved_search));
        state.set_view_group_column(ss(
            if matches!(view, "Album" | "Artist" | "Genre" | "Folder") {
                match view {
                    "Album" => "album",
                    "Artist" => "artist",
                    "Genre" => "genre",
                    _ => "folder",
                }
            } else {
                ""
            },
        ));
        state.set_view_group_value(ss(resolved_group));
        let group_title = if view == "Folder" {
            let trimmed = resolved_group.trim_end_matches(['\\', '/']);
            std::path::Path::new(trimmed)
                .file_name()
                .and_then(|f| f.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(resolved_group)
        } else {
            resolved_group
        };
        state.set_view_group_title(ss(group_title));
        state.set_view_playlist_id(ss(playlist_id));
        let (sort, desc) = if matches!(view, "Album" | "Artist" | "Genre" | "Folder") {
            detail_sort_for_column(match view {
                "Album" => "album",
                "Artist" => "artist",
                "Genre" => "genre",
                _ => "folder",
            })
        } else {
            default_sort_for_view(view)
        };
        state.set_view_sort(ss(sort));
        state.set_view_descending(desc);
        state.set_can_go_back(self.history_pos > 0);
        state.set_can_go_forward(false);
        self.persist_view_context();
        self.refresh_view();
        if view == "Duplicates" && self.duplicate_fingerprints_ready {
            let sort = self.dup_sort.clone();
            self.load_duplicates(&sort, self.dup_desc, 0);
        }
        if view == "Queue" {
            self.publish_queue(true);
        }
    }

    pub(crate) fn go_back(&mut self) {
        if self.history_pos == 0 {
            return;
        }
        self.history_pos -= 1;
        let ctx = self.history[self.history_pos].clone();
        {
            let state = self.ui.global::<State>();
            ctx.restore(&state);
            state.set_can_go_back(self.history_pos > 0);
            state.set_can_go_forward(true);
        }
        self.persist_view_context();
        self.tracks_offset = 0;
        self.refresh_view();
        if self.ui.global::<State>().get_view() == "Queue" {
            self.publish_queue(true);
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if self.history_pos + 1 >= self.history.len() {
            return;
        }
        self.history_pos += 1;
        let ctx = self.history[self.history_pos].clone();
        {
            let state = self.ui.global::<State>();
            ctx.restore(&state);
            state.set_can_go_back(true);
            state.set_can_go_forward(self.history_pos + 1 < self.history.len());
        }
        self.persist_view_context();
        self.tracks_offset = 0;
        self.refresh_view();
        if self.ui.global::<State>().get_view() == "Queue" {
            self.publish_queue(true);
        }
    }

    pub(crate) fn move_navigation(&mut self, view: &str, delta: i32) {
        use music_player_core::settings::keys as k;
        let mut order = self.navigation_order();
        let Some(index) = order.iter().position(|entry| entry == view) else {
            return;
        };
        let target = index as i32 + delta;
        if index == 0 || target <= 0 || target >= order.len() as i32 {
            return;
        }
        order.swap(index, target as usize);
        self.settings
            .set_string_list(k::NAVIGATION_ORDER, &order[1..]);
        self.publish_navigation();
    }

    pub(crate) fn toggle_navigation(&mut self, view: &str) {
        use music_player_core::settings::keys as k;
        if view == "Home" {
            return;
        }
        let mut hidden = self.settings.string_list(k::HIDDEN_PANELS);
        if hidden.iter().any(|entry| entry == view) {
            hidden.retain(|entry| entry != view);
        } else {
            hidden.push(view.to_string());
        }
        self.settings.set_string_list(k::HIDDEN_PANELS, &hidden);
        self.publish_navigation();
    }

    pub(crate) fn begin_search(&mut self) {
        self.ui.global::<State>().set_page_loading(true);
    }

    pub(crate) fn clear_recent_searches(&mut self) {
        self.settings
            .set_string_list(music_player_core::settings::keys::RECENT_SEARCHES, &[]);
        self.ui
            .global::<State>()
            .set_recent_searches(ui_model(Vec::<SharedString>::new()).into());
    }

    pub(crate) fn remove_recent_search(&mut self, term: &str) {
        let mut searches = self.settings.recent_searches();
        searches.retain(|item| !item.eq_ignore_ascii_case(term));
        self.settings.set_string_list(
            music_player_core::settings::keys::RECENT_SEARCHES,
            &searches,
        );
        self.ui
            .global::<State>()
            .set_recent_searches(ui_model(searches.into_iter().map(ss).collect::<Vec<_>>()).into());
    }

    pub(crate) fn save_recent_search(&mut self, term: &str) {
        if term.trim().is_empty() {
            return;
        }
        self.settings.save_recent_search(term);
        let searches = self
            .settings
            .recent_searches()
            .into_iter()
            .map(ss)
            .collect::<Vec<_>>();
        self.ui
            .global::<State>()
            .set_recent_searches(ui_model(searches).into());
    }
}
