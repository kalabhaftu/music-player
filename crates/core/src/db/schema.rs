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
    deduplicate_verbatim_paths(conn)?;

    Ok(())
}

/// Repairs legacy Windows verbatim (`\\?\` / `\\?\UNC\`) paths written by
/// `std::fs::canonicalize` across tracks, relations, settings, and session state,
/// merging interaction counts onto the normalized path without data loss.
pub fn deduplicate_verbatim_paths(conn: &Connection) -> Result<usize> {
    const VERBATIM: &str = "\\\\?\\";
    let needs_track_cleanup: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM tracks WHERE substr(path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM playlist_tracks WHERE substr(track_path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM playback_queue WHERE substr(track_path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM metadata_edit_history WHERE substr(track_path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM tag_backups WHERE substr(original_path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM track_handles WHERE substr(path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM track_interaction_state WHERE substr(path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM track_fingerprints WHERE substr(path, 1, 4) = ?1
                UNION ALL
                SELECT 1 FROM playback_session WHERE id = 1 AND instr(payload, ?1) > 0
                UNION ALL
                SELECT 1 FROM settings WHERE key IN ('library-roots', 'ignored-directories') AND instr(value, ?1) > 0
            )",
            [VERBATIM],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !needs_track_cleanup {
        return Ok(0);
    }

    let mut fixed_count = 0usize;
    let tx = conn.unchecked_transaction()?;

    // Collect every verbatim track row first so we can mutate safely.
    let mut verbatim_tracks: Vec<(String, i64, i64, i64, Option<String>, String, Option<String>, i64)> =
        Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT path, favorite, rating, play_count, last_played_utc, added_utc, artwork_path, has_lyrics
             FROM tracks
             WHERE substr(path, 1, 4) = ?1",
        )?;
        let mut rows = stmt.query([VERBATIM])?;
        while let Some(row) = rows.next()? {
            verbatim_tracks.push((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ));
        }
    }

    for (old_path, old_fav, old_rat, old_plays, old_last, old_added, old_art, old_lyrics) in verbatim_tracks {
        let clean = crate::normalize_windows_path_str(&old_path);
        if clean.is_empty() || clean == old_path {
            continue;
        }
        fixed_count += 1;

        let existing: Option<(String, i64, i64, i64, Option<String>, String, Option<String>, i64)> = tx
            .query_row(
                "SELECT path, favorite, rating, play_count, last_played_utc, added_utc, artwork_path, has_lyrics
                 FROM tracks
                 WHERE path = ?1 COLLATE NOCASE AND substr(path, 1, 4) != ?2
                 LIMIT 1",
                rusqlite::params![&clean, VERBATIM],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .ok();

        let target_path = if let Some((
            ex_path,
            ex_fav,
            ex_rat,
            ex_plays,
            ex_last,
            ex_added,
            ex_art,
            ex_lyrics,
        )) = existing
        {
            let fav = ex_fav.max(old_fav);
            let rat = if ex_rat > 0 { ex_rat } else { old_rat };
            let plays = ex_plays.saturating_add(old_plays);
            let last_played = match (&ex_last, &old_last) {
                (Some(a), Some(b)) => Some(if a >= b { a.clone() } else { b.clone() }),
                (Some(a), None) => Some(a.clone()),
                (None, Some(b)) => Some(b.clone()),
                (None, None) => None,
            };
            let added = if ex_added.is_empty() {
                old_added
            } else if old_added.is_empty() {
                ex_added
            } else if ex_added <= old_added {
                ex_added
            } else {
                old_added
            };
            let art = match (
                ex_art.as_deref().filter(|s| !s.is_empty()),
                old_art.as_deref().filter(|s| !s.is_empty()),
            ) {
                (Some(a), _) => Some(a.to_string()),
                (None, Some(b)) => Some(b.to_string()),
                (None, None) => None,
            };
            let lyrics = ex_lyrics.max(old_lyrics);

            tx.execute(
                "UPDATE tracks SET favorite = ?1, rating = ?2, play_count = ?3, last_played_utc = ?4, added_utc = ?5, artwork_path = ?6, has_lyrics = ?7 WHERE path = ?8",
                rusqlite::params![fav, rat, plays, last_played, added, art, lyrics, &ex_path],
            )?;
            tx.execute("DELETE FROM track_handles WHERE path = ?1", [&old_path])?;
            tx.execute("DELETE FROM track_fingerprints WHERE path = ?1", [&old_path])?;
            tx.execute("DELETE FROM tracks WHERE path = ?1", [&old_path])?;
            ex_path
        } else {
            tx.execute(
                "UPDATE tracks SET path = ?1 WHERE path = ?2",
                rusqlite::params![&clean, &old_path],
            )?;
            let _ = tx.execute(
                "UPDATE OR IGNORE track_handles SET path = ?1 WHERE path = ?2",
                rusqlite::params![&clean, &old_path],
            );
            tx.execute("DELETE FROM track_handles WHERE path = ?1", [&old_path])?;
            let _ = tx.execute(
                "UPDATE OR IGNORE track_fingerprints SET path = ?1 WHERE path = ?2",
                rusqlite::params![&clean, &old_path],
            );
            tx.execute("DELETE FROM track_fingerprints WHERE path = ?1", [&old_path])?;
            clean
        };

        tx.execute(
            "UPDATE playlist_tracks SET track_path = ?1 WHERE track_path = ?2",
            rusqlite::params![&target_path, &old_path],
        )?;
        tx.execute(
            "UPDATE playback_queue SET track_path = ?1 WHERE track_path = ?2",
            rusqlite::params![&target_path, &old_path],
        )?;
        tx.execute(
            "UPDATE metadata_edit_history SET track_path = ?1 WHERE track_path = ?2",
            rusqlite::params![&target_path, &old_path],
        )?;
        tx.execute(
            "UPDATE tag_backups SET original_path = ?1 WHERE original_path = ?2",
            rusqlite::params![&target_path, &old_path],
        )?;
        tx.execute(
            "DELETE FROM track_interaction_state WHERE path = ?1",
            [&old_path],
        )?;
    }

    // Clean any lingering orphan verbatim references across secondary tables.
    for (table, col) in [
        ("playlist_tracks", "track_path"),
        ("playback_queue", "track_path"),
        ("metadata_edit_history", "track_path"),
        ("tag_backups", "original_path"),
    ] {
        let mut stale = Vec::new();
        {
            let sql = format!("SELECT DISTINCT {col} FROM {table} WHERE substr({col}, 1, 4) = ?1");
            let mut stmt = tx.prepare(&sql)?;
            let mut rows = stmt.query([VERBATIM])?;
            while let Some(row) = rows.next()? {
                stale.push(row.get::<_, String>(0)?);
            }
        }
        for old in stale {
            let clean = crate::normalize_windows_path_str(&old);
            let upd = format!("UPDATE {table} SET {col} = ?1 WHERE {col} = ?2");
            tx.execute(&upd, rusqlite::params![clean, old])?;
            fixed_count += 1;
        }
    }
    tx.execute(
        "DELETE FROM track_handles WHERE substr(path, 1, 4) = ?1",
        [VERBATIM],
    )?;
    tx.execute(
        "DELETE FROM track_fingerprints WHERE substr(path, 1, 4) = ?1",
        [VERBATIM],
    )?;
    tx.execute(
        "DELETE FROM track_interaction_state WHERE substr(path, 1, 4) = ?1",
        [VERBATIM],
    )?;

    // Clean library-roots & ignored-directories JSON arrays.
    for key in ["library-roots", "ignored-directories"] {
        if let Ok(raw) = tx.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        ) {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) {
                let mut seen = std::collections::HashSet::new();
                let mut cleaned = Vec::with_capacity(list.len());
                for item in list {
                    let norm = crate::normalize_windows_path_str(&item);
                    if !norm.is_empty() && seen.insert(norm.to_lowercase()) {
                        cleaned.push(norm);
                    }
                }
                if let Ok(json_str) = serde_json::to_string(&cleaned) {
                    if json_str != raw {
                        tx.execute(
                            "UPDATE settings SET value = ?1 WHERE key = ?2",
                            rusqlite::params![json_str, key],
                        )?;
                        fixed_count += 1;
                    }
                }
            }
        }
    }

    // Clean saved playback_session JSON if it stored verbatim paths.
    if let Ok(raw) = tx.query_row(
        "SELECT payload FROM playback_session WHERE id = 1",
        [],
        |row| row.get::<_, String>(0),
    ) {
        if raw.contains(VERBATIM) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(obj) = val.as_object_mut() {
                    for k in ["trackPath", "track_path"] {
                        if let Some(s) = obj.get(k).and_then(|v| v.as_str()).map(|s| s.to_string()) {
                            obj.insert(
                                k.to_string(),
                                serde_json::Value::String(crate::normalize_windows_path_str(&s)),
                            );
                        }
                    }
                    if let Some(arr) = obj.get_mut("queue").and_then(|v| v.as_array_mut()) {
                        for elem in arr.iter_mut() {
                            if let Some(s) = elem.as_str() {
                                *elem = serde_json::Value::String(
                                    crate::normalize_windows_path_str(s),
                                );
                            }
                        }
                    }
                }
                if let Ok(updated) = serde_json::to_string(&val) {
                    tx.execute(
                        "UPDATE playback_session SET payload = ?1 WHERE id = 1",
                        [updated],
                    )?;
                    fixed_count += 1;
                }
            }
        }
    }

    if fixed_count > 0 {
        let _ = tx.execute("INSERT INTO tracks_fts(tracks_fts) VALUES('rebuild')", []);
    }
    tx.commit()?;
    Ok(fixed_count)
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
