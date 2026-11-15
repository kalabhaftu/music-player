use crate::models::{LibraryGroup, LibraryStats, Track, TrackSort};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result, Row};

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
            favorite, rating, play_count, last_played_utc, artwork_path, has_lyrics
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
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
            has_lyrics = excluded.has_lyrics
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
            if track.has_lyrics { 1 } else { 0 }
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
