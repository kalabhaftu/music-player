use rusqlite::{Connection, Result};

pub const SCHEMA_VERSION: i32 = 11;

pub fn initialize_schema(conn: &Connection) -> Result<()> {
    // Migration gate (mirrors LibraryStore.cs): stamp PRAGMA user_version with the
    // schema version; existing pre-versioned databases are upgraded in place by the
    // idempotent CREATE IF NOT EXISTS batch below.
    let user_version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    let _ = user_version;

    // CREATE TABLE IF NOT EXISTS never adds columns to a database written by an
    // older build. Add the newer columns *before* their indexes are created, or
    // `CREATE INDEX ... ON tracks(unavailable)` fails on an existing library.db.
    if table_has_column(conn, "tracks", "path")? {
        for (column, declaration) in [
            ("artwork_path", "artwork_path TEXT NULL"),
            ("has_lyrics", "has_lyrics INTEGER NOT NULL DEFAULT -1"),
            ("unavailable", "unavailable INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !table_has_column(conn, "tracks", column)? {
                conn.execute(
                    &format!("ALTER TABLE tracks ADD COLUMN {}", declaration),
                    [],
                )?;
            }
        }
    }

    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS tracks (
            path TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            artist TEXT NOT NULL DEFAULT '',
            album TEXT NOT NULL DEFAULT '',
            album_artist TEXT NOT NULL DEFAULT '',
            genre TEXT NOT NULL DEFAULT '',
            year INTEGER NOT NULL DEFAULT 0,
            track_number INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            file_size INTEGER NOT NULL DEFAULT 0,
            modified_utc TEXT NOT NULL,
            added_utc TEXT NOT NULL,
            favorite INTEGER NOT NULL DEFAULT 0,
            rating INTEGER NOT NULL DEFAULT 0 CHECK(rating BETWEEN 0 AND 5),
            play_count INTEGER NOT NULL DEFAULT 0,
            last_played_utc TEXT NULL,
            artwork_path TEXT NULL,
            has_lyrics INTEGER NOT NULL DEFAULT -1 CHECK(has_lyrics IN (-1,0,1)),
            unavailable INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS ix_tracks_unavailable ON tracks(unavailable);
        CREATE INDEX IF NOT EXISTS ix_tracks_title ON tracks(title COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS ix_tracks_artist ON tracks(artist COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS ix_tracks_album ON tracks(album COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS ix_tracks_artwork_path ON tracks(artwork_path COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS ix_tracks_path_nocase ON tracks(path COLLATE NOCASE);

        CREATE TABLE IF NOT EXISTS playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_utc TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            track_path TEXT NOT NULL,
            PRIMARY KEY(playlist_id, position),
            FOREIGN KEY(playlist_id) REFERENCES playlists(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS ix_playlist_tracks_path ON playlist_tracks(playlist_id, track_path, position);

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS playback_session (
            id INTEGER PRIMARY KEY CHECK(id=1),
            payload TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS playback_queue (
            position INTEGER PRIMARY KEY,
            track_path TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tag_backups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            original_path TEXT NOT NULL,
            backup_path TEXT NOT NULL,
            created_utc TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS metadata_edit_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            track_path TEXT NOT NULL,
            field_name TEXT NOT NULL,
            old_value TEXT,
            new_value TEXT,
            snapshot_json TEXT,
            backup_file_path TEXT,
            created_utc TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS ix_metadata_edit_history_path ON metadata_edit_history(track_path COLLATE NOCASE);

        CREATE TABLE IF NOT EXISTS track_handles (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS track_interaction_state (
            path TEXT PRIMARY KEY,
            added_utc TEXT NOT NULL,
            favorite INTEGER NOT NULL,
            rating INTEGER NOT NULL,
            play_count INTEGER NOT NULL,
            last_played_utc TEXT NULL
        );

        CREATE TABLE IF NOT EXISTS track_fingerprints (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            audio_hash TEXT NOT NULL,
            created_utc TEXT NOT NULL
        );

        -- Scan session state (LibraryScanSession.cs): unseen tracks under a
        -- completed root are removed when the run completes.
        CREATE TABLE IF NOT EXISTS scan_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_utc TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS scan_roots (
            run_id INTEGER NOT NULL,
            root_path TEXT NOT NULL,
            root_prefix TEXT NOT NULL,
            completed INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(run_id, root_path)
        );
        CREATE TABLE IF NOT EXISTS scan_seen (
            run_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY(run_id, path)
        );
        CREATE TABLE IF NOT EXISTS scan_excluded (
            run_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            prefix TEXT NOT NULL,
            PRIMARY KEY(run_id, path)
        );
        CREATE TABLE IF NOT EXISTS scan_user_ignored (
            run_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            prefix TEXT NOT NULL,
            PRIMARY KEY(run_id, path)
        );
        CREATE TABLE IF NOT EXISTS scan_default_excluded (
            run_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            prefix TEXT NOT NULL,
            PRIMARY KEY(run_id, path)
        );
        CREATE TABLE IF NOT EXISTS scan_incomplete_paths (
            run_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            prefix TEXT NOT NULL,
            PRIMARY KEY(run_id, path)
        );

        -- Preserve interactions when tracks are deleted and re-scanned
        CREATE TRIGGER IF NOT EXISTS tracks_preserve_interactions BEFORE DELETE ON tracks BEGIN
            INSERT INTO track_interaction_state(path, added_utc, favorite, rating, play_count, last_played_utc)
            VALUES(old.path, old.added_utc, old.favorite, old.rating, old.play_count, old.last_played_utc)
            ON CONFLICT(path) DO UPDATE SET
                added_utc = excluded.added_utc,
                favorite = excluded.favorite,
                rating = excluded.rating,
                play_count = excluded.play_count,
                last_played_utc = excluded.last_played_utc;
        END;

        CREATE TRIGGER IF NOT EXISTS tracks_restore_interactions AFTER INSERT ON tracks
        WHEN EXISTS(SELECT 1 FROM track_interaction_state WHERE path = new.path) BEGIN
            UPDATE tracks SET (added_utc, favorite, rating, play_count, last_played_utc) =
                (SELECT added_utc, favorite, rating, play_count, last_played_utc FROM track_interaction_state WHERE path = new.path)
            WHERE path = new.path;
        END;
        "#,
    )?;

    // FTS5 Virtual Table for Instant Search
    let fts_check: Result<i64, _> = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='tracks_fts'",
        [],
        |row| row.get(0),
    );

    if fts_check.unwrap_or(0) == 0 {
        conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
                title, artist, album, album_artist, genre, path,
                content='tracks',
                content_rowid='rowid',
                tokenize='unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS tracks_fts_insert AFTER INSERT ON tracks BEGIN
                INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre, path)
                VALUES(new.rowid, new.title, new.artist, new.album, new.album_artist, new.genre, new.path);
            END;

            CREATE TRIGGER IF NOT EXISTS tracks_fts_delete AFTER DELETE ON tracks BEGIN
                INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre, path)
                VALUES('delete', old.rowid, old.title, old.artist, old.album, old.album_artist, old.genre, old.path);
            END;

            CREATE TRIGGER IF NOT EXISTS tracks_fts_update AFTER UPDATE OF title, artist, album, album_artist, genre, path ON tracks BEGIN
                INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre, path)
                VALUES('delete', old.rowid, old.title, old.artist, old.album, old.album_artist, old.genre, old.path);
                INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre, path)
                VALUES(new.rowid, new.title, new.artist, new.album, new.album_artist, new.genre, new.path);
            END;
            "#,
        )?;
    }

    // Pre-v11 databases carry no `unavailable` column; add it so the soft
    // availability model (retain row / exclude from visible queries) applies.
    let has_unavailable: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('tracks') WHERE name = 'unavailable'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if has_unavailable == 0 {
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN unavailable INTEGER NOT NULL DEFAULT 0;
             CREATE INDEX IF NOT EXISTS ix_tracks_unavailable ON tracks(unavailable);",
        )?;
    }

    conn.execute(&format!("PRAGMA user_version = {}", SCHEMA_VERSION), [])?;

    super::functions::register_query_functions(conn);

    Ok(())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
