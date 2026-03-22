use chrono::Utc;
use recap_core::models::*;

#[test]
fn source_display_roundtrips() {
    let sources = [Source::Linear, Source::GitHub, Source::Slack, Source::Notion];
    for source in &sources {
        let s = source.to_string();
        let parsed: Source = s.parse().expect("should parse back");
        assert_eq!(&parsed, source);
    }
}

#[test]
fn source_from_str_rejects_unknown() {
    let result: Result<Source, _> = "foobar".parse();
    assert!(result.is_err());
}

#[test]
fn activity_kind_serialization() {
    let kind = ActivityKind::PrMerged;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(json, r#""pr_merged""#);

    let deserialized: ActivityKind = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, kind);
}

#[test]
fn activity_kind_display_matches_serde() {
    let kind = ActivityKind::CommitPushed;
    assert_eq!(kind.to_string(), "commit_pushed");
}

#[test]
fn activity_kind_from_str() {
    let kind: ActivityKind = "issue_completed".parse().unwrap();
    assert_eq!(kind, ActivityKind::IssueCompleted);
}

#[test]
fn activity_new_populates_defaults() {
    let activity = Activity::new(
        Source::GitHub,
        "gh-123".to_string(),
        ActivityKind::PrMerged,
        "My PR".to_string(),
        "https://github.com/org/repo/pull/123".to_string(),
        Utc::now(),
    );

    assert!(!activity.id.is_empty());
    assert!(activity.description.is_none());
    assert!(activity.project.is_none());
    assert_eq!(activity.metadata, serde_json::Value::Null);
}

#[test]
fn activity_serializes_to_json() {
    let activity = Activity::new(
        Source::Linear,
        "lin-456".to_string(),
        ActivityKind::IssueCreated,
        "New issue".to_string(),
        "https://linear.app/team/LIN-456".to_string(),
        Utc::now(),
    );

    let json = serde_json::to_value(&activity).unwrap();
    assert_eq!(json["source"], "linear");
    assert_eq!(json["kind"], "issue_created");
    assert_eq!(json["title"], "New issue");
}

#[test]
fn digest_stats_default_is_empty() {
    let stats = DigestStats::default();
    assert_eq!(stats.total_activities, 0);
    assert!(stats.by_source.is_empty());
    assert!(stats.by_kind.is_empty());
}

#[test]
fn period_variants_serialize_correctly() {
    let day = Period::Day(chrono::NaiveDate::from_ymd_opt(2026, 3, 22).unwrap());
    let json = serde_json::to_value(&day).unwrap();
    assert!(json.get("Day").is_some());

    let week = Period::Week(chrono::NaiveDate::from_ymd_opt(2026, 3, 16).unwrap());
    let json = serde_json::to_value(&week).unwrap();
    assert!(json.get("Week").is_some());
}
