use crate::*;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub fn register(ui: &AppWindow, app: Rc<RefCell<App>>) {
    // Library / scan.
    {
        let a = app.clone();
        ui.global::<Actions>().on_scan_library(move || {
            let mut app = a.borrow_mut();
            let mut roots = app.settings.library_roots();
            let ignored = app.settings.ignored_directories();
            let extra = app.store.discover_unconfigured_music_roots(&roots, &ignored);
            if !extra.is_empty() {
                for r in extra {
                    if !roots.iter().any(|existing| existing.eq_ignore_ascii_case(&r)) {
                        roots.push(r);
                    }
                }
                app.settings.set_library_roots(&roots);
                app.load_roots();
            }
            if roots.is_empty() {
                drop(app);
                if let Some(path) = pick_folder() {
                    a.borrow_mut().add_root(&path);
                }
            } else {
                app.start_scan(&roots, false);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_rebuild_library_index(move || {
            let mut app = a.borrow_mut();
            let mut roots = app.settings.library_roots();
            let ignored = app.settings.ignored_directories();
            let extra = app.store.discover_unconfigured_music_roots(&roots, &ignored);
            if !extra.is_empty() {
                for r in extra {
                    if !roots.iter().any(|existing| existing.eq_ignore_ascii_case(&r)) {
                        roots.push(r);
                    }
                }
                app.settings.set_library_roots(&roots);
                app.load_roots();
            }
            if roots.is_empty() {
                drop(app);
                if let Some(path) = pick_folder() {
                    a.borrow_mut().add_root(&path);
                }
            } else {
                app.start_scan(&roots, true);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_pause_scan(move || {
            a.borrow_mut().scan.toggle_pause();
            let mut app = a.borrow_mut();
            app.publish_scan();
            if app.scan.get_status().paused {
                app.show_notice("Scan paused.", NoticeSeverity::Informational);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_cancel_scan(move || {
            a.borrow_mut().scan.cancel();
            a.borrow_mut()
                .ui
                .global::<State>()
                .set_scan_outcome(ss("Scan cancelled · tracks already indexed were kept"));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_add_root(move |path| a.borrow_mut().add_root(&path));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_remove_root(move |path| {
            let mut app = a.borrow_mut();
            app.confirm_action = Some(ConfirmAction::RemoveRoot(path.to_string()));
            app.ui.global::<State>().set_confirm_message(ss(format!(
                "Stop scanning {} and remove its unshared tracks from the index? Music files and playlist entries remain.",
                path
            )));
            app.ui.global::<State>().set_confirm_danger(true);
            app.open_modal("confirm");
            app.ui
                .global::<State>()
                .set_modal_title(ss("Remove folder?"));
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_rescan_root(move |path| {
            a.borrow_mut().start_scan(&[path.to_string()], true);
        });
    }

    // Duplicates.
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_load_duplicates(move |sort, desc, page| {
                a.borrow_mut().load_duplicates(&sort, desc, page as usize);
            });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more_duplicates(move || {
            let mut app = a.borrow_mut();
            let page = app.dup_page + 1;
            {
                let sort = app.dup_sort.clone();
                let desc = app.dup_desc;
                app.load_duplicates(&sort, desc, page);
            }
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_open_duplicate_files(move |id| a.borrow_mut().open_duplicate_files(&id));
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_close_duplicate_files(move || {
            a.borrow()
                .ui
                .global::<State>()
                .set_duplicate_files(ui_model(Vec::<TrackItem>::new()).into());
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_prepare_duplicates(move || {
            let mut app = a.borrow_mut();
            app.duplicate_fingerprints_ready = false;
            app.start_duplicate_load();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more(move || {
            let mut app = a.borrow_mut();
            app.tracks_offset += TRACK_PAGE_SIZE;
            app.refresh_view();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>().on_load_more_groups(move || {
            let mut app = a.borrow_mut();
            app.groups_offset += 60;
            app.refresh_view();
        });
    }
    {
        let a = app.clone();
        ui.global::<Actions>()
            .on_load_more_artist_albums(move || a.borrow_mut().load_more_artist_albums());
    }
}
