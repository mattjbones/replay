use chrono::{TimeZone, Utc};
use recap_app::commands::{in_progress_week, score_days};

fn weeks(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|s| s.to_string()).collect()
}

#[test]
fn last_week_is_in_progress_when_it_is_the_current_week() {
    // Monday 2026-11-23 is in %W week 47
    let now = Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap();
    let w = weeks(&["2026-W45", "2026-W46", "2026-W47"]);
    assert_eq!(in_progress_week(&w, now), Some((2, 1)));

    // Sunday is day 7 of the same week
    let sun = Utc.with_ymd_and_hms(2026, 11, 29, 12, 0, 0).unwrap();
    assert_eq!(in_progress_week(&w, sun), Some((2, 7)));
}

#[test]
fn no_in_progress_week_when_the_current_week_has_no_data() {
    let now = Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap();
    assert_eq!(in_progress_week(&weeks(&["2026-W45", "2026-W46"]), now), None);
}

#[test]
fn score_days_weights_kinds_within_the_day_range() {
    let rows = vec![
        ("2026-11-16".to_string(), "pr_merged".to_string(), 2),
        ("2026-11-22".to_string(), "pr_reviewed".to_string(), 4),
        ("2026-11-23".to_string(), "pr_merged".to_string(), 1), // outside range
        ("2026-11-20".to_string(), "message_sent".to_string(), 50), // unweighted
    ];
    let weights = [("pr_merged", 3.0), ("pr_reviewed", 1.5)];
    // [16th, 23rd): 2*3 + 4*1.5
    assert_eq!(score_days(&rows, "2026-11-16", "2026-11-23", &weights), 12.0);
}
