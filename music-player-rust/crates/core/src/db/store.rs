use super::{
    availability, playlists,
    reader::{PooledReader, ReaderPool},
    schema, search, session, tracks,
    writer::LibraryWriter,
};
use crate::models::{
    LibraryGroup, LibraryStats, PlaybackSession, Playlist, PlaylistEntry, PlaylistSummary, Track,
    TrackSort,
};
use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub enum ReaderGuard<'a> {
    Pooled(PooledReader),
    Locked(std::sync::MutexGuard<'a, Connection>),
}

impl<'a> std::ops::Deref for ReaderGuard<'a> {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        match self {
            ReaderGuard::Pooled(r) => r.deref(),
            ReaderGuard::Locked(g) => g.deref(),
        }
    }
}

enum StoreInner {
    Disk {
        writer: LibraryWriter,
        readers: ReaderPool,
        db_path: PathBuf,
    },
    Memory {
        conn: Mutex<Connection>,
        db_path: PathBuf,
    },
}

#[derive(Clone)]
pub struct LibraryStore {
    inner: Arc<StoreInner>,
}

impl LibraryStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let writer = LibraryWriter::start(&db_path)?;
        let readers = ReaderPool::new(&db_path, 8)?;

        Ok(Self {
            inner: Arc::new(StoreInner::Disk {
                writer,
                readers,
                db_path,
            }),
        })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::initialize_schema(&conn)?;
        super::functions::register_query_functions(&conn);

        Ok(Self {
            inner: Arc::new(StoreInner::Memory {
                conn: Mutex::new(conn),
                db_path: PathBuf::from(":memory:"),
            }),
        })
    }

    pub fn in_app_data() -> Result<Self> {
        let app_dir = directories::BaseDirs::new()
            .map(|dirs| dirs.data_local_dir().join("MusicPlayer"))
            .unwrap_or_else(|| PathBuf::from("."));
        let db_path = app_dir.join("library.db");
        Self::open(db_path)
    }

    pub fn db_path(&self) -> &Path {
        match &*self.inner {
            StoreInner::Disk { db_path, .. } => db_path,
            StoreInner::Memory { db_path, .. } => db_path,
        }
    }

    pub fn read_conn(&self) -> Result<ReaderGuard<'_>> {
        match &*self.inner {
            StoreInner::Disk { readers, .. } => readers.acquire().map(ReaderGuard::Pooled),
            StoreInner::Memory { conn, .. } => Ok(ReaderGuard::Locked(conn.lock().unwrap())),
        }
    }

    pub fn write<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        match &*self.inner {
            StoreInner::Disk { writer, .. } => writer.execute(f),
            StoreInner::Memory { conn, .. } => {
                let mut guard = conn.lock().unwrap();
                f(&mut guard)
            }
        }
    }

    pub fn write_async<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Connection) -> Result<()> + Send + 'static,
    {
        match &*self.inner {
            StoreInner::Disk { writer, .. } => writer.execute_async(f),
            StoreInner::Memory { conn, .. } => {
                let mut guard = conn.lock().unwrap();
                f(&mut guard)
            }
        }
    }

    // --- Track Operations ---
    pub fn upsert_track(&self, track: &Track) -> Result<()> {
        let track = track.clone();
        self.write(move |conn| tracks::upsert_track(conn, &track))
    }

    pub fn upsert_tracks(&self, tracks: &[Track]) -> Result<usize> {
        let tracks = tracks.to_vec();
        self.write(move |conn| tracks::upsert_tracks(conn, &tracks))
    }

    pub fn get_track(&self, path: &str) -> Result<Option<Track>> {
        let conn = self.read_conn()?;
        tracks::get_track_by_path(&conn, path)
    }

    pub fn get_tracks(&self, paths: &[String]) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        tracks::get_tracks_by_paths(&conn, paths)
    }

    pub fn get_all_tracks(&self, sort: TrackSort, descending: bool) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        tracks::get_all_tracks(&conn, sort, descending)
    }

    pub fn get_favorites(&self) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        tracks::get_favorites(&conn)
    }

    pub fn get_recently_added(&self, limit: u32) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        tracks::get_recently_added(&conn, limit)
    }

    pub fn get_most_played(&self, limit: u32) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        tracks::get_most_played(&conn, limit)
    }

    pub fn get_library_stats(&self) -> Result<LibraryStats> {
        let conn = self.read_conn()?;
        tracks::get_library_stats(&conn)
    }

    pub fn count_tracks_in_folder(&self, folder: &str) -> Result<i64> {
        let conn = self.read_conn()?;
        tracks::count_tracks_in_folder(&conn, folder)
    }

    pub fn get_album_groups(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.read_conn()?;
        tracks::get_album_groups(&conn)
    }

    pub fn get_artist_groups(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.read_conn()?;
        tracks::get_artist_groups(&conn)
    }

    pub fn get_genre_groups(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.read_conn()?;
        tracks::get_genre_groups(&conn)
    }

    pub fn set_favorite(&self, path: &str, favorite: bool) -> Result<()> {
        let path = path.to_string();
        self.write(move |conn| tracks::update_favorite(conn, &path, favorite))
    }

    pub fn update_track_lyrics_status(&self, path: &str, has_lyrics: bool) -> Result<()> {
        let path = path.to_string();
        self.write(move |conn| tracks::update_track_lyrics_status(conn, &path, has_lyrics))
    }

    pub fn set_rating(&self, path: &str, rating: u8) -> Result<()> {
        let path = path.to_string();
        self.write(move |conn| tracks::update_rating(conn, &path, rating))
    }

    pub fn record_play(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        self.write(move |conn| tracks::record_play(conn, &path))
    }

    // --- Search ---
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        search::search_tracks(&conn, query, limit)
    }

    pub fn search_tracks(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        self.search(query, limit)
    }

    // --- Playlists ---
    pub fn create_playlist(&self, id: &str, name: &str) -> Result<Playlist> {
        let id = id.to_string();
        let name = name.to_string();
        self.write(move |conn| playlists::create_playlist(conn, &id, &name))
    }

    pub fn get_playlists(&self) -> Result<Vec<PlaylistSummary>> {
        let conn = self.read_conn()?;
        playlists::get_playlists(&conn)
    }

    pub fn get_playlist(&self, id: &str) -> Result<Option<Playlist>> {
        let conn = self.read_conn()?;
        playlists::get_playlist(&conn, id)
    }

    pub fn get_playlist_entries(&self, id: &str) -> Result<Vec<PlaylistEntry>> {
        let conn = self.read_conn()?;
        playlists::get_playlist_entries(&conn, id)
    }

    pub fn get_playlist_entries_page(
        &self,
        id: &str,
        search: Option<&str>,
        offset: u32,
        page_size: u32,
    ) -> Result<(Vec<PlaylistEntry>, i64)> {
        let conn = self.read_conn()?;
        playlists::get_playlist_entries_page(&conn, id, search, offset, page_size)
    }

    pub fn add_tracks_to_playlist(&self, id: &str, paths: &[String]) -> Result<()> {
        let id = id.to_string();
        let paths = paths.to_vec();
        self.write(move |conn| playlists::add_tracks_to_playlist(conn, &id, &paths))
    }

    pub fn playlist_exists(&self, id: &str) -> Result<bool> {
        let conn = self.read_conn()?;
        playlists::playlist_exists(&conn, id)
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> Result<()> {
        let id = id.to_string();
        let name = name.to_string();
        self.write(move |conn| playlists::rename_playlist(conn, &id, &name))
    }

    pub fn get_playlist_paths(&self, id: &str) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        playlists::get_playlist_paths(&conn, id)
    }

    pub fn remove_from_playlist(&self, id: &str, position: u32) -> Result<bool> {
        let id = id.to_string();
        self.write(move |conn| playlists::remove_from_playlist(conn, &id, position))
    }

    pub fn move_playlist_track(&self, id: &str, position: u32, to_position: u32) -> Result<bool> {
        let id = id.to_string();
        self.write(move |conn| playlists::move_playlist_track(conn, &id, position, to_position))
    }

    pub fn set_playlist_tracks(&self, id: &str, paths: &[String]) -> Result<()> {
        let id = id.to_string();
        let paths = paths.to_vec();
        self.write(move |conn| playlists::set_playlist_tracks(conn, &id, &paths))
    }

    pub fn delete_playlist(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.write(move |conn| playlists::delete_playlist(conn, &id))
    }

    // --- View & group queries ---
    pub fn query_track_page(&self, query: &tracks::TrackQuery) -> Result<(Vec<Track>, i64)> {
        let conn = self.read_conn()?;
        tracks::query_track_page(&conn, query)
    }

    pub fn query_group_page(
        &self,
        column: &str,
        search: Option<&str>,
        offset: u32,
        page_size: u32,
        hide_duplicates: bool,
    ) -> Result<(Vec<LibraryGroup>, i64)> {
        let conn = self.read_conn()?;
        tracks::query_group_page(&conn, column, search, offset, page_size, hide_duplicates)
    }

    // --- Session & Queue ---
    pub fn save_session(&self, session: &PlaybackSession) -> Result<()> {
        let session = session.clone();
        self.write(move |conn| session::save_playback_session(conn, &session))
    }

    pub fn load_session(&self) -> Result<Option<PlaybackSession>> {
        let conn = self.read_conn()?;
        session::load_playback_session(&conn)
    }

    pub fn save_queue(&self, paths: &[String]) -> Result<()> {
        let paths = paths.to_vec();
        self.write(move |conn| session::save_playback_queue(conn, &paths))
    }

    pub fn load_queue(&self) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        session::load_playback_queue(&conn)
    }

    // --- Settings ---
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.read_conn()?;
        session::get_setting(&conn, key)
    }

    pub fn get_setting_keys_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        session::get_setting_keys_with_prefix(&conn, prefix)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let key = key.to_string();
        let value = value.to_string();
        self.write(move |conn| session::set_setting(conn, &key, &value))
    }

    // --- Availability & Cleanup ---
    pub fn purge_deleted_tracks(&self, paths: &[String]) -> Result<usize> {
        let paths = paths.to_vec();
        self.write(move |conn| availability::purge_deleted_tracks(conn, &paths))
    }

    pub fn purge_tracks_under_directory(&self, dir_prefix: &str) -> Result<usize> {
        let dir_prefix = dir_prefix.to_string();
        self.write(move |conn| availability::purge_tracks_under_directory(conn, &dir_prefix))
    }

    pub fn purge_tracks_under_directories(&self, prefixes: &[String]) -> Result<usize> {
        let prefixes = prefixes.to_vec();
        self.write(move |conn| availability::purge_tracks_under_directories(conn, &prefixes))
    }

    pub fn purge_tracks_in_default_excluded_directories(&self) -> Result<usize> {
        self.write(move |conn| availability::purge_tracks_in_default_excluded_directories(conn))
    }

    pub fn get_referenced_paths(&self) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        availability::get_referenced_paths(&conn)
    }

    pub fn clear_unavailable_tracks_that_exist(&self) -> Result<Vec<String>> {
        self.write(move |conn| availability::clear_unavailable_tracks_that_exist(conn))
    }

    pub fn set_track_unavailable(&self, path: &str, unavailable: bool) -> Result<()> {
        let path = path.to_string();
        self.write(move |conn| tracks::set_track_unavailable(conn, &path, unavailable))
    }

    pub fn reinstate_track(&self, path: &str) -> Result<bool> {
        let path = path.to_string();
        self.write(move |conn| availability::reinstate_track(conn, &path))
    }

    pub fn mark_tracks_under_root_unavailable(&self, root_prefix: &str) -> Result<u32> {
        let root_prefix = root_prefix.to_string();
        self.write(move |conn| tracks::mark_tracks_under_root_unavailable(conn, &root_prefix))
    }

    pub fn get_track_paths_page(
        &self,
        group_column: Option<&str>,
        group_value: Option<&str>,
        offset: u32,
        page_size: u32,
    ) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        tracks::get_track_paths_page(&conn, group_column, group_value, offset, page_size)
    }

    pub fn get_tracks_page(&self, query: &tracks::TrackQuery) -> Result<Vec<Track>> {
        let (tracks, _) = self.query_track_page(query)?;
        Ok(tracks)
    }

    pub fn artwork_refs(&self) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        tracks::artwork_refs(&conn)
    }

    pub fn clear_missing_artwork_refs(&self, missing: &[String]) -> Result<usize> {
        let missing = missing.to_vec();
        self.write(move |conn| tracks::clear_missing_artwork_refs(conn, &missing))
    }

    pub fn tracks_with_artwork_refs(&self, refs: &[String]) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        tracks::tracks_with_artwork_refs(&conn, refs)
    }

    pub fn tracks_missing_artwork(&self) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        tracks::tracks_missing_artwork(&conn)
    }

    pub fn set_track_artworks(&self, updates: &[(String, String)]) -> Result<usize> {
        let updates = updates.to_vec();
        self.write(move |conn| tracks::set_track_artworks(conn, &updates))
    }

    pub fn set_track_artwork(&self, path: &str, artwork: Option<&str>) -> Result<usize> {
        let path = path.to_string();
        let artwork = artwork.map(|s| s.to_string());
        self.write(move |conn| tracks::set_track_artwork(conn, &path, artwork.as_deref()))
    }

    pub fn find_album_artwork(&self, album: &str) -> Result<Option<String>> {
        let conn = self.read_conn()?;
        tracks::find_album_artwork(&conn, album)
    }

    pub fn find_folder_artwork(&self, track_path: &str) -> Result<Option<String>> {
        let conn = self.read_conn()?;
        tracks::find_folder_artwork(&conn, track_path)
    }

    pub fn smart_autoplay_tracks(
        &self,
        current_path: &str,
        genre: &str,
        artist: &str,
        exclude_paths: &[String],
        limit: usize,
    ) -> Result<Vec<String>> {
        let conn = self.read_conn()?;
        tracks::smart_autoplay_tracks(&conn, current_path, genre, artist, exclude_paths, limit)
    }

    pub fn checkpoint_wal(&self) -> Result<()> {
        self.write(|conn| {
            conn.execute("PRAGMA wal_checkpoint(PASSIVE)", [])?;
            Ok(())
        })
    }

    pub fn discover_unconfigured_music_roots(
        &self,
        configured_roots: &[String],
        ignored_directories: &[String],
    ) -> Vec<String> {
        let Ok(conn) = self.read_conn() else {
            return Vec::new();
        };
        availability::discover_unconfigured_music_roots(
            &conn,
            configured_roots,
            ignored_directories,
        )
    }

    pub fn prepare_duplicate_fingerprints(&self) -> Result<usize> {
        self.write(|conn| crate::duplicates::DuplicateDetector::prepare_fingerprints(conn))
    }

    pub fn get_duplicate_groups(
        &self,
        offset: usize,
        page_size: usize,
    ) -> Result<Vec<crate::duplicates::ExactDuplicateGroup>> {
        let conn = self.read_conn()?;
        crate::duplicates::DuplicateDetector::get_duplicate_groups(&conn, offset, page_size)
    }

    pub fn get_duplicate_tracks(&self, sha256: &str) -> Result<Vec<Track>> {
        let conn = self.read_conn()?;
        crate::duplicates::DuplicateDetector::get_duplicate_tracks(&conn, sha256)
    }

    pub fn record_metadata_edit(
        &self,
        track_path: &str,
        field_name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        snapshot_json: &str,
        backup_file_path: Option<&str>,
    ) -> Result<i64> {
        let track_path = track_path.to_string();
        let field_name = field_name.to_string();
        let old_value = old_value.map(|s| s.to_string());
        let new_value = new_value.map(|s| s.to_string());
        let snapshot_json = snapshot_json.to_string();
        let backup_file_path = backup_file_path.map(|s| s.to_string());
        self.write(move |conn| {
            session::record_metadata_edit(
                conn,
                &track_path,
                &field_name,
                old_value.as_deref(),
                new_value.as_deref(),
                &snapshot_json,
                backup_file_path.as_deref(),
            )
        })
    }

    pub fn get_metadata_edit_history(
        &self,
        track_path: &str,
    ) -> Result<Vec<crate::models::MetadataEditEntry>> {
        let conn = self.read_conn()?;
        session::get_metadata_edit_history(&conn, track_path)
    }
}
