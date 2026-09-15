use chrono::{TimeZone, Utc};
use recap_core::db::{
    get_all_sync_cursors, get_latest_sync_time, search_activities, update_sync_cursor,
    upsert_activity, Database,
};
use recap_core::models::{Activity, ActivityKind, Source};

fn temp_db() -> Database {
    let path = std::env::temp_dir().join(format!("recap-test-{}.db", ulid::Ulid::new()));
    Database::new(&path).expect("open temp db")
}

fn activity(source_id: &str, title: &str) -> Activity {
    Activity::new(
        Source::GitHub,
        source_id.to_string(),
        ActivityKind::PrMerged,
        title.to_string(),
        format!("https://example.com/{source_id}"),
        Utc.with_ymd_and_hms(2026, 9, 15, 9, 0, 0).unwrap(),
    )
}

#[test]
fn search_treats_like_wildcards_as_literals() {
    let db = temp_db();
    upsert_activity(&db, &activity("a", "100% done")).unwrap();
    upsert_activity(&db, &activity("b", "100 percent done")).unwrap();
    upsert_activity(&db, &activity("c", "snake_case name")).unwrap();
    upsert_activity(&db, &activity("d", "snakeXcase name")).unwrap();
    upsert_activity(&db, &activity("e", "back\\slash")).unwrap();

    let pct = search_activities(&db, "100%").unwrap();
    assert_eq!(pct.iter().map(|a| a.source_id.as_str()).collect::<Vec<_>>(), ["a"]);

    let underscore = search_activities(&db, "snake_case").unwrap();
    assert_eq!(underscore.iter().map(|a| a.source_id.as_str()).collect::<Vec<_>>(), ["c"]);

    let backslash = search_activities(&db, "back\\slash").unwrap();
    assert_eq!(backslash.iter().map(|a| a.source_id.as_str()).collect::<Vec<_>>(), ["e"]);
}

#[test]
fn search_is_case_insensitive_substring() {
    let db = temp_db();
    upsert_activity(&db, &activity("a", "Fix Graphite merge detection")).unwrap();
    let hits = search_activities(&db, "graphite").unwrap();
    assert_eq!(hits.len(), 1);
    assert!(search_activities(&db, "nomatch").unwrap().is_empty());
}

#[test]
fn latest_sync_time_is_none_on_empty_table_and_max_after_updates() {
    let db = temp_db();
    assert_eq!(get_latest_sync_time(&db), None);
    assert!(get_all_sync_cursors(&db).unwrap().is_empty());

    let before = Utc::now();
    update_sync_cursor(&db, &Source::GitHub, "cursor-1");
    update_sync_cursor(&db, &Source::Linear, "cursor-2");

    let latest = get_latest_sync_time(&db).expect("latest sync present");
    assert!(latest >= before - chrono::Duration::seconds(1));

    let cursors = get_all_sync_cursors(&db).unwrap();
    assert_eq!(cursors.len(), 2);
    assert!(cursors.iter().any(|(s, c, _)| s == "github" && c == "cursor-1"));
}
