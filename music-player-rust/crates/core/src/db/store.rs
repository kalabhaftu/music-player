use super::{availability, playlists, schema, search, session, tracks};
use crate::models::{
    LibraryGroup, LibraryStats, PlaybackSession, Playlist, PlaylistEntry, PlaylistSummary, Track,
    TrackSort,
};
use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LibraryStore {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl LibraryStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(&db_path)?;
        schema::initialize_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::initialize_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: PathBuf::from(":memory:"),
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
        &self.db_path
    }

    // --- Track Operations ---
    pub fn upsert_track(&self, track: &Track) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        tracks::upsert_track(&conn, track)
    }

    pub fn upsert_tracks(&self, tracks: &[Track]) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        tracks::upsert_tracks(&mut conn, tracks)
    }

    pub fn get_track(&self, path: &str) -> Result<Option<Track>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_track_by_path(&conn, path)
    }

    pub fn get_tracks(&self, paths: &[String]) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_tracks_by_paths(&conn, paths)
    }

    pub fn get_all_tracks(&self, sort: TrackSort, descending: bool) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_all_tracks(&conn, sort, descending)
    }

    pub fn get_favorites(&self) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_favorites(&conn)
    }

    pub fn get_recently_added(&self, limit: u32) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_recently_added(&conn, limit)
    }

    pub fn get_most_played(&self, limit: u32) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_most_played(&conn, limit)
    }

    pub fn get_library_stats(&self) -> Result<LibraryStats> {
        let conn = self.conn.lock().unwrap();
        tracks::get_library_stats(&conn)
    }

    pub fn count_tracks_in_folder(&self, folder: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        tracks::count_tracks_in_folder(&conn, folder)
    }

    pub fn get_album_groups(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_album_groups(&conn)
    }

    pub fn get_artist_groups(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_artist_groups(&conn)
    }

    pub fn get_genre_groups(&self) -> Result<Vec<LibraryGroup>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_genre_groups(&conn)
    }

    pub fn set_favorite(&self, path: &str, favorite: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        tracks::update_favorite(&conn, path, favorite)
    }

    pub fn set_rating(&self, path: &str, rating: u8) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        tracks::update_rating(&conn, path, rating)
    }

    pub fn record_play(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        tracks::record_play(&conn, path)
    }

    // --- Search ---
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        search::search_tracks(&conn, query, limit)
    }

    pub fn search_tracks(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        self.search(query, limit)
    }

    // --- Playlists ---
    pub fn create_playlist(&self, id: &str, name: &str) -> Result<Playlist> {
        let conn = self.conn.lock().unwrap();
        playlists::create_playlist(&conn, id, name)
    }

    pub fn get_playlists(&self) -> Result<Vec<PlaylistSummary>> {
        let conn = self.conn.lock().unwrap();
        playlists::get_playlists(&conn)
    }

    pub fn get_playlist(&self, id: &str) -> Result<Option<Playlist>> {
        let conn = self.conn.lock().unwrap();
        playlists::get_playlist(&conn, id)
    }

    pub fn get_playlist_entries(&self, id: &str) -> Result<Vec<PlaylistEntry>> {
        let conn = self.conn.lock().unwrap();
        playlists::get_playlist_entries(&conn, id)
    }

    pub fn get_playlist_entries_page(
        &self,
        id: &str,
        search: Option<&str>,
        offset: u32,
        page_size: u32,
    ) -> Result<(Vec<PlaylistEntry>, i64)> {
        let conn = self.conn.lock().unwrap();
        playlists::get_playlist_entries_page(&conn, id, search, offset, page_size)
    }

    pub fn add_tracks_to_playlist(&self, id: &str, paths: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        playlists::add_tracks_to_playlist(&conn, id, paths)
    }

    pub fn playlist_exists(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        playlists::playlist_exists(&conn, id)
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        playlists::rename_playlist(&conn, id, name)
    }

    pub fn get_playlist_paths(&self, id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        playlists::get_playlist_paths(&conn, id)
    }

    pub fn remove_from_playlist(&self, id: &str, position: u32) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        playlists::remove_from_playlist(&conn, id, position)
    }

    pub fn move_playlist_track(&self, id: &str, position: u32, to_position: u32) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        playlists::move_playlist_track(&conn, id, position, to_position)
    }

    pub fn set_playlist_tracks(&self, id: &str, paths: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        playlists::set_playlist_tracks(&conn, id, paths)
    }

    pub fn delete_playlist(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        playlists::delete_playlist(&conn, id)
    }

    // --- View & group queries ---
    pub fn query_track_page(&self, query: &tracks::TrackQuery) -> Result<(Vec<Track>, i64)> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        tracks::query_group_page(&conn, column, search, offset, page_size, hide_duplicates)
    }

    // --- Session & Queue ---
    pub fn save_session(&self, session: &PlaybackSession) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        session::save_playback_session(&conn, session)
    }

    pub fn load_session(&self) -> Result<Option<PlaybackSession>> {
        let conn = self.conn.lock().unwrap();
        session::load_playback_session(&conn)
    }

    pub fn save_queue(&self, paths: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        session::save_playback_queue(&conn, paths)
    }

    pub fn load_queue(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        session::load_playback_queue(&conn)
    }

    // --- Settings ---
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        session::get_setting(&conn, key)
    }

    pub fn get_setting_keys_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        session::get_setting_keys_with_prefix(&conn, prefix)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        session::set_setting(&conn, key, value)
    }

    // --- Availability & Cleanup ---
    pub fn purge_deleted_tracks(&self, paths: &[String]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        availability::purge_deleted_tracks(&conn, paths)
    }

    pub fn purge_tracks_under_directory(&self, dir_prefix: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        availability::purge_tracks_under_directory(&conn, dir_prefix)
    }

    pub fn purge_tracks_under_directories(&self, prefixes: &[String]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        availability::purge_tracks_under_directories(&conn, prefixes)
    }

    pub fn purge_tracks_in_default_excluded_directories(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        availability::purge_tracks_in_default_excluded_directories(&conn)
    }

    pub fn get_referenced_paths(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        availability::get_referenced_paths(&conn)
    }

    pub fn clear_unavailable_tracks_that_exist(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        availability::clear_unavailable_tracks_that_exist(&conn)
    }

    pub fn set_track_unavailable(&self, path: &str, unavailable: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        tracks::set_track_unavailable(&conn, path, unavailable)
    }

    /// Restores an available track, reporting whether the flag changed.
    pub fn reinstate_track(&self, path: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        availability::reinstate_track(&conn, path)
    }

    pub fn mark_tracks_under_root_unavailable(&self, root_prefix: &str) -> Result<u32> {
        let conn = self.conn.lock().unwrap();
        tracks::mark_tracks_under_root_unavailable(&conn, root_prefix)
    }

    pub fn get_track_paths_page(
        &self,
        group_column: Option<&str>,
        group_value: Option<&str>,
        offset: u32,
        page_size: u32,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        tracks::get_track_paths_page(&conn, group_column, group_value, offset, page_size)
    }

    /// Bounded page of full tracks in stable path order — used by index
    /// refresh passes (GetTracksPage contract).
    pub fn get_tracks_page(
        &self,
        query: &tracks::TrackQuery,
    ) -> Result<Vec<Track>> {
        let (tracks, _) = self.query_track_page(query)?;
        Ok(tracks)
    }

    pub fn artwork_refs(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        tracks::artwork_refs(&conn)
    }

    pub fn clear_missing_artwork_refs(&self, missing: &[String]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        tracks::clear_missing_artwork_refs(&conn, missing)
    }

    pub fn tracks_with_artwork_refs(&self, refs: &[String]) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        tracks::tracks_with_artwork_refs(&conn, refs)
    }

    pub fn tracks_missing_artwork(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        tracks::tracks_missing_artwork(&conn)
    }

    pub fn set_track_artworks(&self, updates: &[(String, String)]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        tracks::set_track_artworks(&conn, updates)
    }

    pub fn set_track_artwork(&self, path: &str, artwork: Option<&str>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        tracks::set_track_artwork(&conn, path, artwork)
    }

    /// Weighted recommendation query for autoplay (GetSmartAutoplayTracks).
    pub fn smart_autoplay_tracks(
        &self,
        current_path: &str,
        genre: &str,
        artist: &str,
        exclude_paths: &[String],
        limit: usize,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        tracks::smart_autoplay_tracks(&conn, current_path, genre, artist, exclude_paths, limit)
    }

    pub fn checkpoint_wal(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("PRAGMA wal_checkpoint(PASSIVE)", [])?;
        Ok(())
    }

    /// Builds the exact-duplicate fingerprint table. Used by the Duplicates view.
    pub fn prepare_duplicate_fingerprints(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        crate::duplicates::DuplicateDetector::prepare_fingerprints(&conn)
    }

    /// One page of exact-duplicate groups.
    pub fn get_duplicate_groups(
        &self,
        offset: usize,
        page_size: usize,
    ) -> Result<Vec<crate::duplicates::ExactDuplicateGroup>> {
        let conn = self.conn.lock().unwrap();
        crate::duplicates::DuplicateDetector::get_duplicate_groups(&conn, offset, page_size)
    }

    pub fn get_duplicate_tracks(&self, sha256: &str) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        crate::duplicates::DuplicateDetector::get_duplicate_tracks(&conn, sha256)
    }
}
