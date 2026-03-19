use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::models::{Activity, ActivityKind, Source};

use super::Database;

/// Inserts an activity or replaces it if a row with the same (source, source_id) already exists.
pub fn upsert_activity(db: &Database, activity: &Activity) -> rusqlite::Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO activities
            (id, source, source_id, kind, title, description, url, project, occurred_at, metadata, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            activity.id,
            activity.source.to_string(),
            activity.source_id,
            activity.kind.to_string(),
            activity.title,
            activity.description,
            activity.url,
            activity.project,
            activity.occurred_at.to_rfc3339(),
            activity.metadata.to_string(),
            activity.synced_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Returns all activities whose `occurred_at` falls within [start, end), ordered by occurred_at DESC.
pub fn get_activities_for_range(
    db: &Database,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> rusqlite::Result<Vec<Activity>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, source, source_id, kind, title, description, url, project, occurred_at, metadata, synced_at
         FROM activities
         WHERE occurred_at >= ?1 AND occurred_at < ?2
         ORDER BY occurred_at DESC",
    )?;

    let rows = stmt.query_map(params![start.to_rfc3339(), end.to_rfc3339()], row_to_activity)?;

    rows.collect()
}

/// Returns activities for a specific source within a time range, ordered by occurred_at DESC.
pub fn get_activities_by_source(
    db: &Database,
    source: &Source,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> rusqlite::Result<Vec<Activity>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, source, source_id, kind, title, description, url, project, occurred_at, metadata, synced_at
         FROM activities
         WHERE source = ?1 AND occurred_at >= ?2 AND occurred_at < ?3
         ORDER BY occurred_at DESC",
    )?;

    let rows = stmt.query_map(
        params![source.to_string(), start.to_rfc3339(), end.to_rfc3339()],
        row_to_activity,
    )?;

    rows.collect()
}

/// Upserts the sync cursor for a given source, recording the current time as last_sync.
pub fn update_sync_cursor(db: &Database, source: &Source, cursor: &str) {
    let conn = db.conn.lock().unwrap();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT OR REPLACE INTO sync_cursors (source, cursor, last_sync) VALUES (?1, ?2, ?3)",
        params![source.to_string(), cursor, now],
    )
    .expect("failed to update sync cursor");
}

/// Returns the stored sync cursor for a source, if one exists.
pub fn get_sync_cursor(db: &Database, source: &Source) -> Option<String> {
    let conn = db.conn.lock().unwrap();
    conn.query_row(
        "SELECT cursor FROM sync_cursors WHERE source = ?1",
        params![source.to_string()],
        |row| row.get(0),
    )
    .ok()
}

/// Maps a rusqlite row to an Activity struct.
fn row_to_activity(row: &rusqlite::Row) -> rusqlite::Result<Activity> {
    let source_str: String = row.get(1)?;
    let kind_str: String = row.get(3)?;
    let occurred_at_str: String = row.get(8)?;
    let metadata_str: String = row.get(9)?;
    let synced_at_str: String = row.get(10)?;

    Ok(Activity {
        id: row.get(0)?,
        source: source_str
            .parse::<Source>()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))))?,
        source_id: row.get(2)?,
        kind: kind_str
            .parse::<ActivityKind>()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))))?,
        title: row.get(4)?,
        description: row.get(5)?,
        url: row.get(6)?,
        project: row.get(7)?,
        occurred_at: DateTime::parse_from_rfc3339(&occurred_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e)))?,
        metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::Value::Null),
        synced_at: DateTime::parse_from_rfc3339(&synced_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e)))?,
    })
}
