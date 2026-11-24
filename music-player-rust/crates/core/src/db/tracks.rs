use crate::models::{LibraryGroup, LibraryStats, Track, TrackSort};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result, Row};
use std::path::Path;
use super::search;

pub fn row_to_track(row: &Row) -> Result<Track> {
    let path: String = row.get("path")?;
    let title: String = row.get("title")?;
    let artist: String = row.get("artist")?;
    let album: String = row.get("album")?;
    let album_artist: String = row.get("album_artist")?;
    let genre: String = row.get("genre")?;
    let year: u32 = row.get::<_, i64>("year")? as u32;
    let track_number: u32 = row.get::<_, i64>("track_number")? as u32;
    let duration_ms: u64 = row.get::<_, i64>("duration_ms")? as u64;
    let file_size: u64 = row.get::<_, i64>("file_size")? as u64;
    let modified_str: String = row.get("modified_utc")?;
    let added_str: String = row.get("added_utc")?;
    let favorite: bool = row.get::<_, i64>("favorite")? != 0;
    let rating: u8 = row.get::<_, i64>("rating")? as u8;
    let play_count: u32 = row.get::<_, i64>("play_count")? as u32;
    let last_played_str: Option<String> = row.get("last_played_utc")?;
    let artwork_path: Option<String> = row.get("artwork_path")?;
    let has_lyrics: bool = row.get::<_, i64>("has_lyrics")? == 1;

    let modified_utc = DateTime::parse_from_rfc3339(&modified_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let added_utc = DateTime::parse_from_rfc3339(&added_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let last_played_utc = last_played_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    let unavailable = row.get::<_, Option<i64>>("unavailable").unwrap_or(Some(0)).unwrap_or(0) != 0;

    Ok(Track {
        path,
        title,
        artist,
        album,
        album_artist,
        genre,
        year,
        track_number,
        duration_ms,
        file_size,
        modified_utc,
        added_utc,
        favorite,
        rating,
        play_count,
        last_played_utc,
        artwork_path,
        has_lyrics,
        unavailable,
    })
}

pub fn upsert_track(conn: &Connection, track: &Track) -> Result<()> {
    let modified_str = track.modified_utc.to_rfc3339();
    let added_str = track.added_utc.to_rfc3339();
    let last_played_str = track.last_played_utc.map(|dt| dt.to_rfc3339());

    conn.execute(
        r#"
        INSERT INTO tracks (
            path, title, artist, album, album_artist, genre, year,
            track_number, duration_ms, file_size, modified_utc, added_utc,
            favorite, rating, play_count, last_played_utc, artwork_path, has_lyrics,
            unavailable
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
        )
        ON CONFLICT(path) DO UPDATE SET
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            album_artist = excluded.album_artist,
            genre = excluded.genre,
            year = excluded.year,
            track_number = excluded.track_number,
            duration_ms = excluded.duration_ms,
            file_size = excluded.file_size,
            modified_utc = excluded.modified_utc,
            artwork_path = excluded.artwork_path,
            has_lyrics = excluded.has_lyrics,
            unavailable = excluded.unavailable
        "#,
        params![
            track.path,
            track.title,
            track.artist,
            track.album,
            track.album_artist,
            track.genre,
            track.year as i64,
            track.track_number as i64,
            track.duration_ms as i64,
            track.file_size as i64,
            modified_str,
            added_str,
            if track.favorite { 1 } else { 0 },
            track.rating as i64,
            track.play_count as i64,
            last_played_str,
            track.artwork_path,
            if track.has_lyrics { 1 } else { 0 },
            if track.unavailable { 1 } else { 0 }
        ],
    )?;

    Ok(())
}

pub fn upsert_tracks(conn: &mut Connection, tracks: &[Track]) -> Result<usize> {
    if tracks.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    for track in tracks {
        upsert_track(&tx, track)?;
    }
    tx.commit()?;
    Ok(tracks.len())
}

pub fn get_track_by_path(conn: &Connection, path: &str) -> Result<Option<Track>> {
    let mut stmt = conn.prepare("SELECT * FROM tracks WHERE path = ?1")?;
    let mut rows = stmt.query(params![path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_track(row)?))
    } else {
        Ok(None)
    }
}

pub fn get_tracks_by_paths(conn: &Connection, paths: &[String]) -> Result<Vec<Track>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    for chunk in paths.chunks(200) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT * FROM tracks WHERE path IN ({})", placeholders);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(chunk.iter()))?;
        while let Some(row) = rows.next()? {
            results.push(row_to_track(row)?);
        }
    }
    Ok(results)
}

pub fn update_favorite(conn: &Connection, path: &str, favorite: bool) -> Result<()> {
    conn.execute(
        "UPDATE tracks SET favorite = ?1 WHERE path = ?2",
        params![if favorite { 1 } else { 0 }, path],
    )?;
    Ok(())
}

pub fn update_track_lyrics_status(conn: &Connection, path: &str, has_lyrics: bool) -> Result<()> {
    conn.execute(
        "UPDATE tracks SET has_lyrics = ?1 WHERE path = ?2",
        params![if has_lyrics { 1 } else { 0 }, path],
    )?;
    Ok(())
}

pub fn update_rating(conn: &Connection, path: &str, rating: u8) -> Result<()> {
    conn.execute(
        "UPDATE tracks SET rating = ?1 WHERE path = ?2",
        params![rating.min(5) as i64, path],
    )?;
    Ok(())
}

pub fn record_play(conn: &Connection, path: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE tracks SET play_count = play_count + 1, last_played_utc = ?1 WHERE path = ?2",
        params![now, path],
    )?;
    Ok(())
}

pub fn get_all_tracks(conn: &Connection, sort: TrackSort, descending: bool) -> Result<Vec<Track>> {
    let order_col = match sort {
        TrackSort::Title => "title COLLATE NOCASE",
        TrackSort::Artist => "artist COLLATE NOCASE, album COLLATE NOCASE, track_number",
        TrackSort::Album => "album COLLATE NOCASE, track_number",
        TrackSort::Genre => "genre COLLATE NOCASE, artist COLLATE NOCASE",
        TrackSort::Year => "year, album COLLATE NOCASE, track_number",
        TrackSort::Added => "added_utc",
        TrackSort::Duration => "duration_ms",
        TrackSort::PlayCount => "play_count",
        TrackSort::LastPlayed => "last_played_utc",
        TrackSort::Path => "path COLLATE NOCASE",
        TrackSort::Rating => "rating",
        TrackSort::TrackNumber => "track_number",
        TrackSort::Type => "path",
    };

    let dir = if descending { "DESC" } else { "ASC" };
    let sql = format!("SELECT * FROM tracks ORDER BY {} {}", order_col, dir);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut tracks = Vec::new();
    while let Some(row) = rows.next()? {
        tracks.push(row_to_track(row)?);
    }
    Ok(tracks)
}

pub fn get_favorites(conn: &Connection) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare("SELECT * FROM tracks WHERE favorite = 1 ORDER BY title COLLATE NOCASE")?;
    let mut rows = stmt.query([])?;
    let mut tracks = Vec::new();
    while let Some(row) = rows.next()? {
        tracks.push(row_to_track(row)?);
    }
    Ok(tracks)
}

pub fn get_recently_added(conn: &Connection, limit: u32) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare("SELECT * FROM tracks ORDER BY added_utc DESC LIMIT ?1")?;
    let mut rows = stmt.query(params![limit as i64])?;
    let mut tracks = Vec::new();
    while let Some(row) = rows.next()? {
        tracks.push(row_to_track(row)?);
    }
    Ok(tracks)
}

pub fn get_most_played(conn: &Connection, limit: u32) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare("SELECT * FROM tracks WHERE play_count > 0 ORDER BY play_count DESC, last_played_utc DESC LIMIT ?1")?;
    let mut rows = stmt.query(params![limit as i64])?;
    let mut tracks = Vec::new();
    while let Some(row) = rows.next()? {
        tracks.push(row_to_track(row)?);
    }
    Ok(tracks)
}

pub fn get_library_stats(conn: &Connection) -> Result<LibraryStats> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(file_size), 0) FROM tracks",
        [],
        |row| {
            Ok(LibraryStats {
                total_tracks: row.get::<_, i64>(0)? as u32,
                total_bytes: row.get::<_, i64>(1)? as u64,
            })
        },
    )
}

/// Counts visible tracks directly inside a library root, matching the root
/// summary used by the original folder view (child folders are separate rows).
pub fn count_tracks_in_folder(conn: &Connection, folder: &str) -> Result<i64> {
    let query = if cfg!(windows) {
        "SELECT COUNT(*) FROM tracks WHERE unavailable = 0 AND musicplayer_directory(path) = ? COLLATE NOCASE"
    } else {
        "SELECT COUNT(*) FROM tracks WHERE unavailable = 0 AND musicplayer_directory(path) = ?"
    };
    let folder = folder.trim_end_matches(['/', '\\']);
    conn.query_row(query, [folder], |row| row.get(0))
}

pub fn get_album_groups(conn: &Connection) -> Result<Vec<LibraryGroup>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT album, COUNT(*) as cnt, MAX(artwork_path), MAX(artist), MAX(year)
        FROM tracks
        WHERE album != ''
        GROUP BY album COLLATE NOCASE
        ORDER BY album COLLATE NOCASE ASC
        "#,
    )?;
    let mut rows = stmt.query([])?;
    let mut groups = Vec::new();
    while let Some(row) = rows.next()? {
        groups.push(LibraryGroup {
            name: row.get(0)?,
            track_count: row.get::<_, i64>(1)? as u32,
            artwork_path: row.get(2)?,
            artist: row.get(3)?,
            year: row.get::<_, i64>(4).unwrap_or(0) as u32,
        });
    }
    Ok(groups)
}

pub fn get_artist_groups(conn: &Connection) -> Result<Vec<LibraryGroup>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT artist, COUNT(*) as cnt, MAX(artwork_path)
        FROM tracks
        WHERE artist != ''
        GROUP BY artist COLLATE NOCASE
        ORDER BY artist COLLATE NOCASE ASC
        "#,
    )?;
    let mut rows = stmt.query([])?;
    let mut groups = Vec::new();
    while let Some(row) = rows.next()? {
        groups.push(LibraryGroup {
            name: row.get(0)?,
            track_count: row.get::<_, i64>(1)? as u32,
            artwork_path: row.get(2)?,
            artist: None,
            year: 0,
        });
    }
    Ok(groups)
}

pub fn get_genre_groups(conn: &Connection) -> Result<Vec<LibraryGroup>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT genre, COUNT(*) as cnt, MAX(artwork_path)
        FROM tracks
        WHERE genre != ''
        GROUP BY genre COLLATE NOCASE
        ORDER BY genre COLLATE NOCASE ASC
        "#,
    )?;
    let mut rows = stmt.query([])?;
    let mut groups = Vec::new();
    while let Some(row) = rows.next()? {
        groups.push(LibraryGroup {
            name: row.get(0)?,
            track_count: row.get::<_, i64>(1)? as u32,
            artwork_path: row.get(2)?,
            artist: None,
            year: 0,
        });
    }
    Ok(groups)
}

// ---------------------------------------------------------------------------
// View query surface (getTracks / getPlaylistTracks contracts)
// ---------------------------------------------------------------------------

/// Filter + sort + pagination contract used by the app's track views.
#[derive(Debug, Clone, Default)]
pub struct TrackQuery {
    /// `Favorites` view.
    pub favorites: bool,
    /// `Most Played` view (play_count > 0).
    pub most_played: bool,
    /// `Recently Played` view (last_played_utc is set).
    pub recently_played: bool,
    /// `With Lyrics` view.
    pub with_lyrics: bool,
    /// `Videos` view: restrict to the supplied extension set.
    pub extension_filter: Option<Vec<String>>,
    /// Group filter: "album" | "artist" | "genre" | "folder".
    pub group_column: Option<String>,
    pub group_value: Option<String>,
    /// Free-text search narrowed with the FTS index.
    pub search: Option<String>,
    /// Hide copies that share a content fingerprint with another visible track.
    pub hide_duplicates: bool,
    pub sort: TrackSort,
    pub descending: bool,
    pub offset: u32,
    pub page_size: u32,
    /// Folder detail views surface unavailable tracks as placeholders.
    pub include_unavailable: bool,
    /// Browsing-availability overlay: hide paths under these root prefixes
    /// (unavailable roots + scan exclusions; mirrors `musicplayer_visible`).
    pub excluded_prefixes: Vec<String>,
}

fn order_expression(sort: TrackSort, descending: bool) -> String {
    let dir = if descending { "DESC" } else { "ASC" };
    let col = match sort {
        TrackSort::Title => "t.title COLLATE NOCASE",
        TrackSort::Artist => "t.artist COLLATE NOCASE, t.album COLLATE NOCASE, t.track_number",
        TrackSort::Album => "t.album COLLATE NOCASE, t.track_number",
        TrackSort::Genre => "t.genre COLLATE NOCASE, t.artist COLLATE NOCASE",
        TrackSort::Year => "t.year, t.album COLLATE NOCASE, t.track_number",
        TrackSort::Added => "t.added_utc",
        TrackSort::Duration => "t.duration_ms",
        TrackSort::PlayCount => "t.play_count",
        TrackSort::LastPlayed => "t.last_played_utc",
        TrackSort::Path => "t.path COLLATE NOCASE",
        TrackSort::Rating => "t.rating",
        TrackSort::TrackNumber => "t.track_number",
        TrackSort::Type => "t.path COLLATE NOCASE",
    };
    format!("{} {}", col, dir)
}

/// True when the duplicate-fingerprint cache holds rows for the current library.
fn fingerprints_present(conn: &Connection) -> bool {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM track_fingerprints LIMIT 1)", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|v| v != 0)
    .unwrap_or(false)
}

/// SQL fragment that excludes every copy of a duplicate set except the single
/// deterministic representative (lowest path), mirroring the original
/// `hideExactDuplicates` Songs-page behavior.
fn duplicate_suppression_sql() -> &'static str {
    r#"
    AND t.path NOT IN (
        SELECT t2.path FROM tracks t2
        JOIN track_fingerprints f2 ON f2.path = t2.path
        WHERE EXISTS (
            SELECT 1 FROM track_fingerprints f3
            WHERE f3.audio_hash = f2.audio_hash
              AND f3.path < t2.path
              AND EXISTS (SELECT 1 FROM tracks t3 WHERE t3.path = f3.path AND t3.unavailable = 0)
        )
    )"#
}

fn build_view_where(query: &TrackQuery, use_fts: bool) -> (String, Vec<String>) {
    let mut clauses: Vec<String> = if query.include_unavailable {
        Vec::new()
    } else {
        vec!["t.unavailable = 0".to_string()]
    };
    let mut params: Vec<String> = Vec::new();

    // Browsing-availability overlay (unavailable roots + scan exclusions).
    for prefix in &query.excluded_prefixes {
        let trimmed = prefix.trim_end_matches(['/', '\\']);
        if trimmed.is_empty() {
            continue;
        }
        clauses.push("t.path NOT LIKE ? ESCAPE '\\'".to_string());
        let escaped = trimmed
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        params.push(format!("{}\\%", escaped));
    }

    if query.favorites {
        clauses.push("t.favorite = 1".to_string());
    }
    if query.most_played {
        clauses.push("t.play_count > 0".to_string());
    }
    if query.recently_played {
        clauses.push("t.last_played_utc IS NOT NULL".to_string());
    }
    if query.with_lyrics {
        clauses.push("t.has_lyrics = 1".to_string());
    }
    if let Some(extensions) = &query.extension_filter {
        if !extensions.is_empty() {
            let ext_clauses = extensions
                .iter()
                .map(|_| format!("t.path LIKE ?"))
                .collect::<Vec<_>>()
                .join(" OR ");
            clauses.push(format!("({})", ext_clauses));
            for ext in extensions {
                params.push(format!("%.{}", ext.trim_start_matches('.').to_lowercase()));
            }
        }
    }
    if let (Some(column), Some(value)) = (&query.group_column, &query.group_value) {
        match column.as_str() {
            "album" => {
                clauses.push("t.album = ? COLLATE NOCASE".to_string());
                params.push(value.clone());
            }
            "artist" => {
                // Multi-value artist lists are ';'-separated (1:1 with the
                // original BuildTrackQuery artist predicate).
                clauses.push(
                    "(t.artist = ? COLLATE NOCASE OR t.artist LIKE (? || ';%') OR t.artist LIKE ('%; ' || ? || ';%') OR t.artist LIKE ('%; ' || ?) OR t.artist LIKE (? || ' feat%') OR t.artist LIKE (? || ' ft%'))"
                        .to_string(),
                );
                for _ in 0..6 {
                    params.push(value.clone());
                }
            }
            "genre" => {
                clauses.push("t.genre = ? COLLATE NOCASE".to_string());
                params.push(value.clone());
            }
            "folder" => {
                // Folder detail shows direct children only (1:1 with
                // `musicplayer_directory(tracks.path) = $folderPrefix`).
                clauses.push(super::functions::FOLDER_CHILD_PREDICATE.to_string());
                let normalized = value.trim_end_matches('\\').trim_end_matches('/').to_string();
                params.push(normalized);
            }
            _ => {}
        }
    }
    if let Some(term) = query.search.as_deref().map(str::trim) {
        if !term.is_empty() && use_fts {
            clauses.push(
                "t.path IN (SELECT path FROM tracks_fts WHERE tracks_fts MATCH ?)".to_string(),
            );
            params.push(search::fts_query_string(term));
        }
    }

    (clauses.join(" AND "), params)
}

/// Paginated track page for a view query: `(tracks, total_count)`.
/// `hide_duplicates` only takes effect when a fingerprint cache exists.
pub fn query_track_page(
    conn: &Connection,
    query: &TrackQuery,
) -> Result<(Vec<Track>, i64)> {
    let (where_sql, params) = build_view_where(query, true);
    let suppression = if query.hide_duplicates && fingerprints_present(conn) {
        duplicate_suppression_sql().to_string()
    } else {
        String::new()
    };

    // FTS MATCH can be an empty/invalid expression for punctuation-only terms;
    // fall back to a plain (non-FTS) where clause on error.
    let attempt = || -> Result<(Vec<Track>, i64)> {
        let order = order_expression(query.sort, query.descending);
        let sql = format!(
            "SELECT t.* FROM tracks t WHERE {}{} ORDER BY {} LIMIT ? OFFSET ?",
            where_sql,
            suppression,
            order
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = params
            .iter()
            .map(|p| Box::new(p.clone()) as Box<dyn rusqlite::ToSql>)
            .collect();
        bound.push(Box::new(query.page_size.max(1) as i64));
        bound.push(Box::new(query.offset as i64));
        let mut rows = stmt.query(rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())))?;
        let mut tracks = Vec::new();
        while let Some(row) = rows.next()? {
            tracks.push(row_to_track(row)?);
        }

        let count_sql = format!(
            "SELECT COUNT(*) FROM tracks t WHERE {}{}",
            where_sql,
            suppression
        );
        let total: i64 = conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(params.iter()),
            |r| r.get(0),
        )?;
        Ok((tracks, total))
    };

    match attempt() {
        Ok(page) => Ok(page),
        Err(_) => {
            // Retire the FTS clause and retry, mirroring the original
            // duplicate-suppression fallback logging/retry behavior.
            let mut relaxed = query.clone();
            relaxed.search = None;
            let (where_sql, params) = build_view_where(&relaxed, false);
            let order = order_expression(query.sort, query.descending);
            let sql = format!(
                "SELECT t.* FROM tracks t WHERE {} ORDER BY {} LIMIT ? OFFSET ?",
                where_sql, order
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut bound: Vec<Box<dyn rusqlite::ToSql>> = params
                .iter()
                .map(|p| Box::new(p.clone()) as Box<dyn rusqlite::ToSql>)
                .collect();
            bound.push(Box::new(query.page_size.max(1) as i64));
            bound.push(Box::new(query.offset as i64));
            let mut rows = stmt.query(rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())))?;
            let mut tracks = Vec::new();
            while let Some(row) = rows.next()? {
                tracks.push(row_to_track(row)?);
            }
            let count_sql = format!("SELECT COUNT(*) FROM tracks t WHERE {}", where_sql);
            let total: i64 =
                conn.query_row(&count_sql, rusqlite::params_from_iter(params.iter()), |r| {
                    r.get(0)
                })?;
            Ok((tracks, total))
        }
    }
}

/// Distinct group page (`getGroups` contract): album | artist | genre, with
/// optional search narrowing, ordered alphabetically.
pub fn query_group_page(
    conn: &Connection,
    column: &str,
    search: Option<&str>,
    offset: u32,
    page_size: u32,
    hide_duplicates: bool,
) -> Result<(Vec<LibraryGroup>, i64)> {
    let (value_expr, artwork_expr, artist_expr, year_expr, where_name) = match column {
        "album" => (
            "t.album",
            "MAX(NULLIF(t.artwork_path, ''))",
            "MIN(t.artist)",
            "MIN(t.year)",
            "t.album",
        ),
        "artist" => ("t.artist", "MAX(NULLIF(t.artwork_path, ''))", "NULL", "0", "t.artist"),
        "genre" => ("t.genre", "MAX(NULLIF(t.artwork_path, ''))", "NULL", "0", "t.genre"),
        "folder" => (
            "musicplayer_directory(t.path)",
            "MAX(NULLIF(t.artwork_path, ''))",
            "NULL",
            "0",
            "musicplayer_directory(t.path)",
        ),
        _ => return Ok((Vec::new(), 0)),
    };

    let search_terms = search
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>();
    let mut predicates = vec![
        "t.unavailable = 0".to_string(),
        format!("{} != ''", where_name),
        format!("trim({}) != ''", where_name),
        format!("lower(trim({})) NOT IN ('undefined', 'null')", where_name),
    ];
    let mut search_values = Vec::new();
    for (index, term) in search_terms.iter().enumerate() {
        let placeholder = format!("?{}", index + 1);
        if column == "album" {
            predicates.push(format!(
                "({value_expr} LIKE {placeholder} ESCAPE '\\' OR t.artist LIKE {placeholder} ESCAPE '\\' OR t.album_artist LIKE {placeholder} ESCAPE '\\')"
            ));
        } else {
            predicates.push(format!("{value_expr} LIKE {placeholder} ESCAPE '\\'"));
        }
        let escaped = term
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        search_values.push(format!("%{escaped}%"));
    }
    let where_clause = predicates.join(" AND ");
    let count_expr = if hide_duplicates {
        let has_sha256: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('track_fingerprints') WHERE name = 'sha256')",
            [],
            |row| row.get(0),
        )?;
        if has_sha256 {
            "COUNT(DISTINCT COALESCE((SELECT 'sha256:' || f.file_size || ':' || f.sha256 FROM track_fingerprints f WHERE f.path = t.path AND f.file_size = t.file_size AND f.modified_utc = t.modified_utc), 'path:' || t.path))"
        } else {
            "COUNT(DISTINCT COALESCE((SELECT 'sha256:' || f.file_size || ':' || f.audio_hash FROM track_fingerprints f WHERE f.path = t.path AND f.file_size = t.file_size), 'path:' || t.path))"
        }
    } else {
        "COUNT(*)"
    };

    let sql = format!(
        r#"
        WITH groups AS (
            SELECT {} AS value, {} AS cnt, {} AS artwork, {} AS artist, {} AS year
            FROM tracks t
            WHERE {}
            GROUP BY value COLLATE NOCASE
        )
        SELECT value, cnt, artwork, artist, year FROM groups
        ORDER BY value COLLATE NOCASE ASC
        LIMIT ?{} OFFSET ?{}
        "#,
        value_expr,
        count_expr,
        artwork_expr,
        artist_expr,
        year_expr,
        where_clause,
        search_terms.len() + 1,
        search_terms.len() + 2
    );

    let page_limit = page_size.max(1) as i64;
    let page_offset = offset as i64;
    let mut page_bound: Vec<&dyn rusqlite::ToSql> = search_values
        .iter()
        .map(|value| value as &dyn rusqlite::ToSql)
        .collect();
    page_bound.push(&page_limit);
    page_bound.push(&page_offset);

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(page_bound))?;
    let mut groups = Vec::new();
    while let Some(row) = rows.next()? {
        groups.push(LibraryGroup {
            name: row.get(0)?,
            track_count: row.get::<_, i64>(1)? as u32,
            artwork_path: row.get(2)?,
            artist: row.get(3)?,
            year: row.get::<_, Option<i64>>(4).unwrap_or(None).unwrap_or(0) as u32,
        });
    }

    let count_sql = format!(
        "SELECT COUNT(*) FROM (SELECT {} AS value FROM tracks t WHERE {} GROUP BY value COLLATE NOCASE)",
        value_expr, where_clause
    );
    let total: i64 = conn.query_row(
        &count_sql,
        rusqlite::params_from_iter(search_values.iter()),
        |r| r.get(0),
    )?;
    Ok((groups, total))
}

/// Case-insensitive path key used for the autoplay exclusion set (matches
/// the OrdinalIgnoreCase HashSet in `GetSmartAutoplayTracks`).
fn path_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

/// Smart autoplay recommendation query (1:1 port of
/// `LibraryStore.GetSmartAutoplayTracks`): weighted genre/artist/favorite/
/// play-count scoring plus random jitter, bounded to 150 rows, then filtered
/// for queue exclusions and on-disk availability. Falls back to 50 random
/// visible rows when nothing survives.
pub fn smart_autoplay_tracks(
    conn: &Connection,
    current_path: &str,
    genre: &str,
    artist: &str,
    exclude_paths: &[String],
    limit: usize,
) -> Result<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut excludes = std::collections::HashSet::new();
    if !current_path.trim().is_empty() {
        excludes.insert(path_key(current_path));
    }
    for path in exclude_paths {
        if !path.trim().is_empty() {
            excludes.insert(path_key(path));
        }
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT path
        FROM tracks
        WHERE duration_ms >= 15000
        ORDER BY (
            (CASE WHEN ?1 != '' AND genre = ?1 COLLATE NOCASE THEN 4.5 ELSE 0.0 END) +
            (CASE WHEN ?2 != '' AND artist = ?2 COLLATE NOCASE THEN 3.0 ELSE 0.0 END) +
            (CASE WHEN favorite = 1 THEN 2.0 ELSE 0.0 END) +
            MIN(play_count * 0.4, 3.0) +
            (abs(random()) % 300) / 100.0
        ) DESC
        LIMIT 150
        "#,
    )?;
    let mut rows = stmt.query(params![genre, artist])?;
    let mut result: Vec<String> = Vec::with_capacity(limit);
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if excludes.contains(&path_key(&path)) {
            continue;
        }
        if !Path::new(&path).exists() {
            continue;
        }
        result.push(path);
        if result.len() >= limit {
            break;
        }
    }

    if result.is_empty() {
        let mut fallback = conn.prepare(
            "SELECT path FROM tracks WHERE unavailable = 0 AND path != '' ORDER BY random() LIMIT 50",
        )?;
        let mut rows = fallback.query([])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            if path.eq_ignore_ascii_case(current_path) {
                continue;
            }
            if !Path::new(&path).exists() {
                continue;
            }
            result.push(path);
            if result.len() >= limit {
                break;
            }
        }
    }

    Ok(result)
}

/// Availability helpers ------------------------------------------------------

pub fn set_track_unavailable(conn: &Connection, path: &str, unavailable: bool) -> Result<()> {
    conn.execute(
        "UPDATE tracks SET unavailable = ?1 WHERE path = ?2",
        rusqlite::params![if unavailable { 1 } else { 0 }, path],
    )?;
    Ok(())
}

/// Marks every track under an unavailable root (prefix match) as unavailable.
pub fn mark_tracks_under_root_unavailable(conn: &Connection, root_prefix: &str) -> Result<u32> {
    let pattern = format!("{}%", root_prefix.trim_end_matches(['/', '\\']));
    let count = conn.execute(
        "UPDATE tracks SET unavailable = 1 WHERE path LIKE ?1",
        rusqlite::params![pattern],
    )?;
    Ok(count as u32)
}

/// Returns a page of indexed paths (any availability), optionally narrowed to
/// a group prefix — the paging contract used by availability reconciliation.
pub fn get_track_paths_page(
    conn: &Connection,
    group_column: Option<&str>,
    group_value: Option<&str>,
    offset: u32,
    page_size: u32,
) -> Result<Vec<String>> {
    let (sql, params): (String, Vec<String>) = match (group_column, group_value) {
        (Some("folder"), Some(value)) => (
            // Parent-directory equality (1:1 with musicplayer_directory(path) = $folderPrefix).
            "SELECT path FROM tracks WHERE musicplayer_directory(path) = ? COLLATE NOCASE ORDER BY path LIMIT ? OFFSET ?"
                .to_string(),
            vec![value.trim_end_matches(['/', '\\']).to_string()],
        ),
        (Some("album"), Some(value)) => (
            "SELECT path FROM tracks WHERE album = ? COLLATE NOCASE ORDER BY path LIMIT ? OFFSET ?"
                .to_string(),
            vec![value.to_string()],
        ),
        (Some("artist"), Some(value)) => (
            "SELECT path FROM tracks WHERE artist = ? COLLATE NOCASE ORDER BY path LIMIT ? OFFSET ?"
                .to_string(),
            vec![value.to_string()],
        ),
        (Some("genre"), Some(value)) => (
            "SELECT path FROM tracks WHERE genre = ? COLLATE NOCASE ORDER BY path LIMIT ? OFFSET ?"
                .to_string(),
            vec![value.to_string()],
        ),
        _ => (
            "SELECT path FROM tracks ORDER BY path LIMIT ? OFFSET ?".to_string(),
            Vec::new(),
        ),
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut bound: Vec<Box<dyn rusqlite::ToSql>> = params
        .iter()
        .map(|p| Box::new(p.clone()) as Box<dyn rusqlite::ToSql>)
        .collect();
    bound.push(Box::new(page_size.max(1) as i64));
    bound.push(Box::new(offset as i64));
    let mut rows = stmt.query(rusqlite::params_from_iter(bound.iter().map(|b| b.as_ref())))?;
    let mut paths = Vec::new();
    while let Some(row) = rows.next()? {
        paths.push(row.get(0)?);
    }
    Ok(paths)
}

/// Rows that reference artwork files no longer referenced by any track.
pub fn artwork_refs(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT artwork_path FROM tracks WHERE artwork_path IS NOT NULL AND artwork_path != ''",
    )?;
    let mut rows = stmt.query([])?;
    let mut refs = Vec::new();
    while let Some(row) = rows.next()? {
        refs.push(row.get(0)?);
    }
    Ok(refs)
}

/// Deletes artwork rows whose cache file no longer exists (the caller removes
/// the file itself after checking references).
pub fn clear_missing_artwork_refs(conn: &Connection, missing: &[String]) -> Result<usize> {
    let mut count = 0;
    for path in missing {
        count += conn.execute(
            "UPDATE tracks SET artwork_path = NULL WHERE artwork_path = ?1",
            rusqlite::params![path],
        )?;
    }
    Ok(count)
}

/// Indexed track paths that currently point at one of the supplied artwork
/// cache files. Used to re-extract embedded artwork after stale references
/// have been cleared.
pub fn tracks_with_artwork_refs(conn: &Connection, refs: &[String]) -> Result<Vec<String>> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(refs.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT path FROM tracks WHERE artwork_path IN ({}) ORDER BY path COLLATE NOCASE, path",
        placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(refs.iter()))?;
    let mut paths = Vec::new();
    while let Some(row) = rows.next()? {
        paths.push(row.get(0)?);
    }
    Ok(paths)
}

/// Updates only the cached artwork reference for one track.
pub fn set_track_artwork(conn: &Connection, path: &str, artwork: Option<&str>) -> Result<usize> {
    conn.execute(
        "UPDATE tracks SET artwork_path = ?1 WHERE path = ?2",
        rusqlite::params![artwork, path],
    )
}

/// Finds an existing valid cached artwork path for any track in the given album.
pub fn find_album_artwork(conn: &Connection, album: &str) -> Result<Option<String>> {
    let trimmed = album.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT artwork_path FROM tracks WHERE album = ?1 COLLATE NOCASE AND artwork_path IS NOT NULL AND artwork_path != '' LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![trimmed])?;
    if let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if Path::new(&path).exists() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Finds an artwork path for a track by checking other tracks in the same parent folder,
/// or looking for common cover image files on disk.
pub fn find_folder_artwork(conn: &Connection, track_path: &str) -> Result<Option<String>> {
    let p = Path::new(track_path);
    let Some(dir) = p.parent() else {
        return Ok(None);
    };
    let dir_str = dir.to_string_lossy();
    let prefix = format!("{}\\", dir_str.trim_end_matches('\\'));
    let mut stmt = conn.prepare(
        "SELECT artwork_path FROM tracks WHERE path LIKE (?1 || '%') AND artwork_path IS NOT NULL AND artwork_path != '' LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![prefix])?;
    if let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if Path::new(&path).exists() {
            return Ok(Some(path));
        }
    }
    for name in &["cover.jpg", "folder.jpg", "cover.png", "folder.png", "front.jpg", "albumart.jpg"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(Some(candidate.to_string_lossy().to_string()));
        }
    }
    Ok(None)
}

/// Indexed track paths whose artwork reference is missing. Older builds could
/// persist rows without artwork (or lose the cache file); the startup repair
/// re-reads these files and restores the cached artwork.
pub fn tracks_missing_artwork(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM tracks \
         WHERE artwork_path IS NULL OR artwork_path = '' \
         ORDER BY path COLLATE NOCASE, path",
    )?;
    let mut rows = stmt.query([])?;
    let mut paths = Vec::new();
    while let Some(row) = rows.next()? {
        paths.push(row.get(0)?);
    }
    Ok(paths)
}

/// Updates many artwork references in one transaction. The startup repair
/// writes batches so the UI's reader connection is never starved.
pub fn set_track_artworks(conn: &Connection, updates: &[(String, String)]) -> Result<usize> {
    if updates.is_empty() {
        return Ok(0);
    }
    let mut count = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE tracks SET artwork_path = ?1 WHERE path = ?2")?;
        for (path, artwork) in updates {
            count += stmt.execute(rusqlite::params![artwork, path])?;
        }
    }
    tx.commit()?;
    Ok(count)
}
