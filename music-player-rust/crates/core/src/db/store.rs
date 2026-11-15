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

    pub fn add_tracks_to_playlist(&self, id: &str, paths: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        playlists::add_tracks_to_playlist(&conn, id, paths)
    }

    pub fn delete_playlist(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        playlists::delete_playlist(&conn, id)
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
}
