use chrono::{Duration, Utc};
use rusqlite::params;

use crate::models::Source;

use super::Database;

/// Checks whether the sync cursor for a given source is still fresh
/// (i.e., last_sync is within `ttl_minutes` of now).
pub fn is_cache_fresh(db: &Database, source: &Source, ttl_minutes: i64) -> bool {
    let conn = db.conn.lock().unwrap();
    let cutoff = (Utc::now() - Duration::minutes(ttl_minutes))
        .to_rfc3339();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sync_cursors WHERE source = ?1 AND last_sync > ?2",
            params![source.to_string(), cutoff],
            |row| row.get(0),
        )
        .unwrap_or(0);

    count > 0
}

/// Returns a cached LLM summary if it exists and is within the TTL.
pub fn get_cached_summary(db: &Database, cache_key: &str, ttl_minutes: i64) -> Option<String> {
    let conn = db.conn.lock().unwrap();
    let cutoff = (Utc::now() - Duration::minutes(ttl_minutes))
        .to_rfc3339();

    conn.query_row(
        "SELECT summary FROM llm_cache WHERE cache_key = ?1 AND created_at > ?2",
        params![cache_key, cutoff],
        |row| row.get(0),
    )
    .ok()
}

/// Inserts or replaces a cached LLM summary.
pub fn set_cached_summary(db: &Database, cache_key: &str, summary: &str) {
    let conn = db.conn.lock().unwrap();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT OR REPLACE INTO llm_cache (cache_key, summary, created_at) VALUES (?1, ?2, ?3)",
        params![cache_key, summary, now],
    )
    .expect("failed to set cached summary");
}
