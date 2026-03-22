use chrono::Utc;
use recap::digest::build_digest;
use recap::models::*;

fn make_activity(source: Source, kind: ActivityKind, title: &str) -> Activity {
    Activity::new(
        source,
        format!("id-{}", title),
        kind,
        title.to_string(),
        "https://example.com".to_string(),
        Utc::now(),
    )
}

#[test]
fn build_digest_with_no_activities() {
    let digest = build_digest(
        vec![],
        Period::Day(chrono::NaiveDate::from_ymd_opt(2026, 3, 22).unwrap()),
    );

    assert_eq!(digest.stats.total_activities, 0);
    assert!(digest.activities.is_empty());
    assert!(digest.stats.by_source.is_empty());
    assert!(digest.stats.by_kind.is_empty());
    assert!(digest.llm_summary.is_none());
}

#[test]
fn build_digest_counts_by_source() {
    let activities = vec![
        make_activity(Source::GitHub, ActivityKind::PrMerged, "PR 1"),
        make_activity(Source::GitHub, ActivityKind::CommitPushed, "Commit 1"),
        make_activity(Source::Linear, ActivityKind::IssueCreated, "Issue 1"),
    ];

    let digest = build_digest(
        activities,
        Period::Day(chrono::NaiveDate::from_ymd_opt(2026, 3, 22).unwrap()),
    );

    assert_eq!(digest.stats.total_activities, 3);
    assert_eq!(digest.stats.by_source.get("github"), Some(&2));
    assert_eq!(digest.stats.by_source.get("linear"), Some(&1));
}

#[test]
fn build_digest_counts_by_kind() {
    let activities = vec![
        make_activity(Source::GitHub, ActivityKind::PrMerged, "PR 1"),
        make_activity(Source::GitHub, ActivityKind::PrMerged, "PR 2"),
        make_activity(Source::GitHub, ActivityKind::PrReviewed, "Review 1"),
        make_activity(Source::Linear, ActivityKind::IssueCompleted, "Done"),
    ];

    let digest = build_digest(
        activities,
        Period::Week(chrono::NaiveDate::from_ymd_opt(2026, 3, 16).unwrap()),
    );

    assert_eq!(digest.stats.by_kind.get("pr_merged"), Some(&2));
    assert_eq!(digest.stats.by_kind.get("pr_reviewed"), Some(&1));
    assert_eq!(digest.stats.by_kind.get("issue_completed"), Some(&1));
}

#[test]
fn build_digest_preserves_activities() {
    let activities = vec![
        make_activity(Source::GitHub, ActivityKind::CommitPushed, "Commit 1"),
    ];

    let digest = build_digest(
        activities,
        Period::Month(chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
    );

    assert_eq!(digest.activities.len(), 1);
    assert_eq!(digest.activities[0].title, "Commit 1");
}
