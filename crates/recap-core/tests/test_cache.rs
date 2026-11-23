use chrono::{Duration, Utc};
use recap_core::db::{
    get_cached_summary, invalidate_summaries_older_than, set_cached_summary, Database,
};

fn temp_db() -> Database {
    let path = std::env::temp_dir().join(format!("recap-test-{}.db", ulid::Ulid::new()));
    Database::new(&path).expect("open temp db")
}

fn backdate(db: &Database, key: &str, minutes: i64) {
    let at = (Utc::now() - Duration::minutes(minutes)).to_rfc3339();
    db.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE llm_cache SET created_at = ?1 WHERE cache_key = ?2",
            rusqlite::params![at, key],
        )
        .unwrap();
}

#[test]
fn sync_invalidation_keeps_recent_summaries() {
    let db = temp_db();
    set_cached_summary(&db, "fresh", "new");
    set_cached_summary(&db, "old", "stale");
    backdate(&db, "old", 20);

    invalidate_summaries_older_than(&db, 15);

    assert_eq!(get_cached_summary(&db, "fresh", 60).as_deref(), Some("new"));
    assert_eq!(get_cached_summary(&db, "old", 60), None);
}
