use crate::models::PlaybackSession;
use rusqlite::{params, Connection, Result};

pub fn save_playback_session(conn: &Connection, session: &PlaybackSession) -> Result<()> {
    let payload = serde_json::to_string(session).unwrap_or_default();
    conn.execute(
        "INSERT INTO playback_session (id, payload) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
        params![payload],
    )?;
    Ok(())
}

pub fn load_playback_session(conn: &Connection) -> Result<Option<PlaybackSession>> {
    let mut stmt = conn.prepare("SELECT payload FROM playback_session WHERE id = 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let payload: String = row.get(0)?;
        let session = serde_json::from_str::<PlaybackSession>(&payload).ok();
        Ok(session)
    } else {
        Ok(None)
    }
}

pub fn save_playback_queue(conn: &Connection, paths: &[String]) -> Result<()> {
    conn.execute("DELETE FROM playback_queue", [])?;
    let mut stmt = conn.prepare("INSERT INTO playback_queue (position, track_path) VALUES (?1, ?2)")?;
    for (pos, path) in paths.iter().enumerate() {
        stmt.execute(params![pos as i64, path])?;
    }
    Ok(())
}

pub fn load_playback_queue(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT track_path FROM playback_queue ORDER BY position ASC")?;
    let mut rows = stmt.query([])?;
    let mut paths = Vec::new();
    while let Some(row) = rows.next()? {
        paths.push(row.get(0)?);
    }
    Ok(paths)
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

/// All setting keys starting with `prefix` (equalizer preset discovery).
pub fn get_setting_keys_with_prefix(conn: &Connection, prefix: &str) -> Result<Vec<String>> {
    let pattern = format!("{}%", prefix);
    let mut stmt = conn.prepare("SELECT key FROM settings WHERE key LIKE ?1")?;
    let mut rows = stmt.query(params![pattern])?;
    let mut keys = Vec::new();
    while let Some(row) = rows.next()? {
        keys.push(row.get(0)?);
    }
    Ok(keys)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn record_metadata_edit(
    conn: &Connection,
    track_path: &str,
    field_name: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
    snapshot_json: &str,
    backup_file_path: Option<&str>,
) -> Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        r#"INSERT INTO metadata_edit_history
           (track_path, field_name, old_value, new_value, snapshot_json, backup_file_path, created_utc)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        params![
            track_path,
            field_name,
            old_value,
            new_value,
            snapshot_json,
            backup_file_path,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_metadata_edit_history(
    conn: &Connection,
    track_path: &str,
) -> Result<Vec<crate::models::MetadataEditEntry>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, track_path, field_name, old_value, new_value, snapshot_json, backup_file_path, created_utc
           FROM metadata_edit_history
           WHERE track_path = ?1 COLLATE NOCASE
           ORDER BY id DESC"#,
    )?;
    let mut rows = stmt.query(params![track_path])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        let created_raw: String = row.get(7)?;
        let created_utc = chrono::DateTime::parse_from_rfc3339(&created_raw)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        entries.push(crate::models::MetadataEditEntry {
            id: row.get(0)?,
            track_path: row.get(1)?,
            field_name: row.get(2)?,
            old_value: row.get(3)?,
            new_value: row.get(4)?,
            snapshot_json: row.get(5)?,
            backup_file_path: row.get(6)?,
            created_utc,
        });
    }
    Ok(entries)
}
