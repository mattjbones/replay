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

/// Maximum number of activities to load in a single query.
const ACTIVITY_LIMIT: u32 = 500;

/// Returns activities whose `occurred_at` falls within [start, end), ordered by occurred_at DESC.
/// Limited to ACTIVITY_LIMIT rows to cap memory usage.
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
         ORDER BY occurred_at DESC
         LIMIT ?3",
    )?;

    let rows = stmt.query_map(params![start.to_rfc3339(), end.to_rfc3339(), ACTIVITY_LIMIT], row_to_activity)?;

    rows.collect()
}

/// Returns all activities whose `occurred_at` falls within [start, end), ordered by occurred_at DESC.
/// Used by rollups that need complete aggregates (for example burnout over multiple weeks).
pub fn get_activities_for_range_unlimited(
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
/// Limited to ACTIVITY_LIMIT rows to cap memory usage.
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
         ORDER BY occurred_at DESC
         LIMIT ?4",
    )?;

    let rows = stmt.query_map(
        params![source.to_string(), start.to_rfc3339(), end.to_rfc3339(), ACTIVITY_LIMIT],
        row_to_activity,
    )?;

    rows.collect()
}

/// Batch-insert activities in a single transaction, much faster than individual upserts.
pub fn batch_upsert_activities(db: &Database, activities: &[Activity]) -> rusqlite::Result<()> {
    let conn = db.conn.lock().unwrap();
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO activities
                (id, source, source_id, kind, title, description, url, project, occurred_at, metadata, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for activity in activities {
            stmt.execute(params![
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
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
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

/// Returns all sync cursors with their last sync times: (source, cursor, last_sync).
pub fn get_all_sync_cursors(db: &Database) -> rusqlite::Result<Vec<(String, String, String)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT source, cursor, last_sync FROM sync_cursors ORDER BY source",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

// ---------------------------------------------------------------------------
// Trends queries
// ---------------------------------------------------------------------------

/// Weekly activity counts by kind for trailing N weeks.
pub fn query_weekly_velocity(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(String, String, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-W%W', occurred_at) AS week, kind, COUNT(*) AS cnt
         FROM activities WHERE occurred_at >= ?1
         GROUP BY week, kind ORDER BY week",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Activities for a specific day-of-week + hour (used by heatmap drill-down).
/// `dow` is 0=Sun..6=Sat, `hour` is 0..23. Looks back from `since`.
pub fn get_activities_for_dow_hour(
    db: &Database,
    since: DateTime<Utc>,
    dow: i32,
    hour: i32,
) -> rusqlite::Result<Vec<Activity>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, source, source_id, kind, title, description, url, project, occurred_at, metadata, synced_at
         FROM activities
         WHERE occurred_at >= ?1
           AND CAST(strftime('%w', occurred_at) AS INTEGER) = ?2
           AND CAST(strftime('%H', occurred_at) AS INTEGER) = ?3
         ORDER BY occurred_at DESC
         LIMIT 100",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339(), dow, hour], row_to_activity)?;
    rows.collect()
}

/// Activity heatmap: day-of-week (0=Sun) x hour-of-day.
pub fn query_activity_heatmap(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(i32, i32, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%w', occurred_at) AS INTEGER) AS dow,
                CAST(strftime('%H', occurred_at) AS INTEGER) AS hour,
                COUNT(*) AS cnt
         FROM activities WHERE occurred_at >= ?1
         GROUP BY dow, hour",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Cycle time: pairs issue_created → issue_completed by source_id.
pub fn query_cycle_times(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(String, f64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-W%W', comp.occurred_at) AS week,
                (julianday(comp.occurred_at) - julianday(c.occurred_at)) * 24.0 AS hours
         FROM activities c
         JOIN activities comp
           ON c.source = comp.source
           AND c.source_id = comp.source_id
           AND c.kind = 'issue_created'
           AND comp.kind = 'issue_completed'
         WHERE comp.occurred_at >= ?1
           AND hours > 0",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    rows.collect()
}

/// Weekly project distribution.
pub fn query_project_distribution(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(String, String, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-W%W', occurred_at) AS week,
                COALESCE(project, 'Other') AS proj,
                COUNT(*) AS cnt
         FROM activities WHERE occurred_at >= ?1
         GROUP BY week, proj ORDER BY week, cnt DESC",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Weekly off-hours ratio (weekend + before 8am/after 8pm).
pub fn query_off_hours_ratio(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(String, i64, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-W%W', occurred_at) AS week,
                COUNT(*) AS total,
                SUM(CASE
                  WHEN CAST(strftime('%w', occurred_at) AS INTEGER) IN (0, 6) THEN 1
                  WHEN CAST(strftime('%H', occurred_at) AS INTEGER) >= 20
                    OR CAST(strftime('%H', occurred_at) AS INTEGER) < 8 THEN 1
                  ELSE 0 END) AS off_hours
         FROM activities WHERE occurred_at >= ?1
         GROUP BY week ORDER BY week",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Weekly message volume (Slack messages + thread replies).
pub fn query_message_volume(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-W%W', occurred_at) AS week, COUNT(*) AS cnt
         FROM activities WHERE occurred_at >= ?1
           AND kind IN ('message_sent', 'thread_replied')
         GROUP BY week ORDER BY week",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    rows.collect()
}

/// Daily activity vectors: (date, kind, count) for clustering.
pub fn query_daily_vectors(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(String, String, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', occurred_at) AS day, kind, COUNT(*) AS cnt
         FROM activities WHERE occurred_at >= ?1
         GROUP BY day, kind ORDER BY day",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Day-of-week project activity: (dow 0-6, project, count) for next-day prediction.
pub fn query_dow_project(
    db: &Database,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<(i32, String, i64)>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%w', occurred_at) AS INTEGER) AS dow,
                COALESCE(project, 'Other') AS proj,
                COUNT(*) AS cnt
         FROM activities WHERE occurred_at >= ?1
         GROUP BY dow, proj",
    )?;
    let rows = stmt.query_map(params![since.to_rfc3339()], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

/// Maps a rusqlite row to an Activity struct.
/// Returns all activities ordered by occurred_at DESC (capped at 2000 for debug use).
pub fn get_all_activities(db: &Database) -> rusqlite::Result<Vec<Activity>> {
    let conn = db.conn.lock().map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_LOCKED),
            Some(format!("mutex poisoned: {e}")),
        )
    })?;
    let mut stmt = conn.prepare(
        "SELECT id, source, source_id, kind, title, description, url, project, occurred_at, metadata, synced_at
         FROM activities
         ORDER BY occurred_at DESC
         LIMIT 2000",
    )?;
    let rows = stmt.query_map([], row_to_activity)?;
    rows.collect()
}

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
