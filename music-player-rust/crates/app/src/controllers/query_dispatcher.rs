use crate::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

pub(crate) enum QueryPayload {
    TrackPage {
        query: TrackQuery,
        view: String,
        cache_key: String,
    },
    GroupPage {
        column: String,
        search: Option<String>,
        offset: u32,
        limit: u32,
        hide_dupes: bool,
        view: String,
        cache_key: String,
    },
    SearchCounts {
        search: String,
        album_offset: u32,
        artist_offset: u32,
        hide_dupes: bool,
    },
    Duplicates {
        sort: String,
        descending: bool,
        search: String,
        page: usize,
        page_size: usize,
    },
}

pub(crate) enum QueryResult {
    TrackPage {
        view: String,
        cache_key: String,
        query_offset: u32,
        tracks: Vec<Track>,
        total: i64,
    },
    GroupPage {
        view: String,
        cache_key: String,
        groups: Vec<music_player_core::LibraryGroup>,
        total: i64,
    },
    SearchCounts {
        album_groups: Vec<music_player_core::LibraryGroup>,
        albums: i64,
        artist_groups: Vec<music_player_core::LibraryGroup>,
        artists: i64,
        songs: i64,
    },
    Duplicates {
        groups: Vec<ExactDuplicateGroup>,
        total: usize,
        page: usize,
        pages: usize,
        sort: String,
        descending: bool,
    },
}

pub(crate) struct QueryTask {
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
    pub(crate) payload: QueryPayload,
    pub(crate) store: LibraryStore,
    pub(crate) ui_weak: slint::Weak<AppWindow>,
}

struct DispatcherState {
    pending_task: Option<QueryTask>,
    shutdown: bool,
}

pub(crate) struct QueryDispatcher {
    latest_generation: Arc<AtomicU64>,
    state: Arc<(Mutex<DispatcherState>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl QueryDispatcher {
    pub(crate) fn new() -> Self {
        let latest_generation = Arc::new(AtomicU64::new(0));
        let state = Arc::new((
            Mutex::new(DispatcherState {
                pending_task: None,
                shutdown: false,
            }),
            Condvar::new(),
        ));

        let worker_state = Arc::clone(&state);
        let worker_gen = Arc::clone(&latest_generation);

        let worker = std::thread::Builder::new()
            .name("query-dispatcher-worker".to_string())
            .spawn(move || {
                Self::worker_loop(worker_state, worker_gen);
            })
            .ok();

        Self {
            latest_generation,
            state,
            worker,
        }
    }

    pub(crate) fn dispatch(&self, task: QueryTask) {
        self.latest_generation.store(task.generation, Ordering::Release);
        let (lock, cvar) = &*self.state;
        if let Ok(mut guard) = lock.lock() {
            guard.pending_task = Some(task);
            cvar.notify_one();
        }
    }

    pub(crate) fn cancel_below(&self, generation: u64) {
        self.latest_generation.store(generation, Ordering::Release);
    }

    fn worker_loop(state: Arc<(Mutex<DispatcherState>, Condvar)>, latest_gen: Arc<AtomicU64>) {
        loop {
            let task = {
                let (lock, cvar) = &*state;
                let mut guard = match lock.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                while guard.pending_task.is_none() && !guard.shutdown {
                    match cvar.wait(guard) {
                        Ok(g) => guard = g,
                        Err(_) => return,
                    }
                }
                if guard.shutdown {
                    break;
                }
                match guard.pending_task.take() {
                    Some(t) => t,
                    None => continue,
                }
            };

            // If this task was already superseded before running, skip it immediately.
            if task.generation < latest_gen.load(Ordering::Acquire) {
                continue;
            }

            // Run database query off the UI thread using the LibraryStore reader pool
            let result = match task.payload {
                QueryPayload::TrackPage { query, view, cache_key } => {
                    let (mut tracks, total) = task.store.query_track_page(&query).unwrap_or_default();
                    if view == "Search" {
                        let last_offset = if total > 0 { (((total - 1) / 40) * 40) as u32 } else { 0 };
                        if query.offset > last_offset {
                            let mut corrected_query = query.clone();
                            corrected_query.offset = last_offset;
                            tracks = task.store.query_track_page(&corrected_query).unwrap_or_default().0;
                        }
                    }
                    QueryResult::TrackPage {
                        view,
                        cache_key,
                        query_offset: query.offset,
                        tracks,
                        total,
                    }
                }
                QueryPayload::GroupPage { column, search, offset, limit, hide_dupes, view, cache_key } => {
                    let (groups, total) = task
                        .store
                        .query_group_page(&column, search.as_deref(), offset, limit, hide_dupes)
                        .unwrap_or_default();
                    QueryResult::GroupPage {
                        view,
                        cache_key,
                        groups,
                        total,
                    }
                }
                QueryPayload::SearchCounts { search, album_offset, artist_offset, hide_dupes } => {
                    let (album_groups, albums) = if search.trim().is_empty() {
                        (Vec::new(), 0)
                    } else {
                        task.store
                            .query_group_page("album", Some(&search), album_offset, 24, hide_dupes)
                            .unwrap_or_default()
                    };
                    let (artist_groups, artists) = if search.trim().is_empty() {
                        (Vec::new(), 0)
                    } else {
                        task.store
                            .query_group_page("artist", Some(&search), artist_offset, 24, hide_dupes)
                            .unwrap_or_default()
                    };
                    let (_, songs) = if search.trim().is_empty() {
                        (Vec::new(), 0)
                    } else {
                        task.store
                            .query_track_page(&TrackQuery {
                                search: Some(search.clone()),
                                hide_duplicates: hide_dupes,
                                sort: TrackSort::Title,
                                descending: false,
                                page_size: 1,
                                ..TrackQuery::default()
                            })
                            .unwrap_or_default()
                    };
                    QueryResult::SearchCounts {
                        album_groups,
                        albums,
                        artist_groups,
                        artists,
                        songs,
                    }
                }
                QueryPayload::Duplicates { sort, descending, search, page, page_size } => {
                    let mut groups = task.store.get_duplicate_groups(0, 10000).unwrap_or_default();
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
                    let page_groups: Vec<_> = groups
                        .into_iter()
                        .skip(page * page_size)
                        .take(page_size)
                        .collect();
                    QueryResult::Duplicates {
                        groups: page_groups,
                        total,
                        page,
                        pages,
                        sort,
                        descending,
                    }
                }
            };

            // Check again after query completes if superseded while database read was running
            if task.generation < latest_gen.load(Ordering::Acquire) {
                continue; // Discard superseded result!
            }

            // Deliver results back to the Slint UI thread using slint::invoke_from_event_loop
            let ui_weak = task.ui_weak;
            let generation = task.generation;
            let epoch = task.epoch;
            let _ = slint::invoke_from_event_loop(move || {
                // Ensure callbacks do not access destroyed UI state
                let Some(_ui) = ui_weak.upgrade() else {
                    return;
                };
                crate::with_app(|app| {
                    app.on_query_completed(generation, epoch, result);
                });
            });
        }
    }
}

impl Drop for QueryDispatcher {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.state;
        if let Ok(mut guard) = lock.lock() {
            guard.shutdown = true;
            cvar.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
