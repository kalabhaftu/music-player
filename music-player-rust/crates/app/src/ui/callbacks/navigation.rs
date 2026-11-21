use crate::*;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(ui: &AppWindow, app: Rc<RefCell<App>>) {
    // Navigation.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_set_view(move |view, search, group, playlist_id| {
                let mut app = a.borrow_mut();
                if view == "Search" && search.is_empty() {
                    app.ui.global::<State>().set_search_filter(ss("all"));
                }
                app.set_view(&view, &search, &group, &playlist_id);
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_go_back(move || a.borrow_mut().go_back());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_go_forward(move || a.borrow_mut().go_forward());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_begin_search(move || a.borrow_mut().begin_search());
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_search(move |query, filter| {
            let mut app = a.borrow_mut();
            let filter = if matches!(filter.as_str(), "song" | "album" | "artist" | "playlist") {
                filter
            } else {
                "all".into()
            };
            app.ui.global::<State>().set_search_filter(ss(&filter));
            app.set_view("Search", &query, "", "");
            app.save_recent_search(&query);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_search_input(move |query| {
            let mut app = a.borrow_mut();
            app.ui.global::<State>().set_view_search(ss(&query));
            let current = app.ui.global::<State>().get_view().to_string();
            if current == "Settings" {
                app.search_debounce = None;
                let words = query.to_lowercase();
                let matches = |terms: &[&str]| terms.iter().any(|term| words.contains(term));
                let section = if matches(&[
                    "update",
                    "release",
                    "version",
                    "license",
                    "privacy",
                    "repository",
                ]) {
                    Some("About")
                } else if matches(&[
                    "sidebar",
                    "navigation",
                    "tray",
                    "logs",
                    "diagnostic",
                    "default app",
                    "export",
                ]) {
                    Some("Advanced")
                } else if matches(&[
                    "folder",
                    "library",
                    "scan",
                    "index",
                    "duplicate",
                    "column",
                    "artwork",
                    "artist",
                    "album size",
                ]) {
                    Some("Library")
                } else if matches(&[
                    "playback",
                    "autoplay",
                    "lyrics",
                    "seek",
                    "queue",
                    "crossfade",
                    "music output",
                    "equalizer",
                    "volume",
                ]) {
                    Some("Playback")
                } else if matches(&[
                    "theme",
                    "appearance",
                    "accent",
                    "color",
                    "colour",
                    "transparen",
                    "motion",
                    "selection",
                ]) {
                    Some("Appearance")
                } else {
                    None
                };
                if let Some(section) = section {
                    app.ui.global::<State>().set_settings_section(ss(section));
                }
                return;
            }
            if current != "Search" {
                let state = app.ui.global::<State>();
                state.set_search_filter(ss("all"));
                state.set_view(ss("Search"));
            }
            app.search_debounce = Some((
                Instant::now() + Duration::from_millis(150),
                query.to_string(),
            ));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_save_recent_search(move |term| {
            a.borrow_mut().save_recent_search(&term);
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_clear_recent_searches(move || a.borrow_mut().clear_recent_searches());
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_remove_recent_search(move |term| a.borrow_mut().remove_recent_search(&term));
    }
}
