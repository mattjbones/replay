use chrono::{DateTime, Duration, TimeZone, Utc};
use recap_app::commands::compute_pace;

const KINDS: &[(&str, &str)] = &[("pr_merged", "PRs merged"), ("pr_reviewed", "Reviews")];
const WEIGHTS: &[(&str, f64)] = &[("pr_merged", 3.0), ("pr_reviewed", 1.5)];

// Wednesday 2026-11-25 12:00 UTC: 2.5 days into the week starting Mon 2026-11-23
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 25, 12, 0, 0).unwrap()
}

fn ev(t: DateTime<Utc>, kind: &str) -> (DateTime<Utc>, String) {
    (t, kind.to_string())
}

/// `n` events of `kind` on Tuesday of the week `weeks_ago` weeks back.
fn tuesday(weeks_ago: i64, kind: &str, n: usize) -> Vec<(DateTime<Utc>, String)> {
    let t = Utc.with_ymd_and_hms(2026, 11, 24, 10, 0, 0).unwrap() - Duration::weeks(weeks_ago);
    vec![ev(t, kind); n]
}

#[test]
fn compares_with_the_same_point_in_previous_weeks() {
    let mut events = Vec::new();
    events.extend(tuesday(0, "pr_merged", 6)); // this week so far
    events.extend(tuesday(1, "pr_merged", 4)); // last week by now
    for w in 1..=4 {
        events.extend(tuesday(w, "pr_reviewed", 10));
    }
    // Last week's Friday is later in the week than "now": must not count
    let friday = Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap();
    events.extend(vec![ev(friday, "pr_merged"); 50]);

    let pace = compute_pace(&events, KINDS, WEIGHTS, now());
    assert_eq!(pace.day_of_week, 3);

    let merged = &pace.metrics[0];
    assert_eq!((merged.so_far, merged.last_week), (6.0, 4.0));
    assert_eq!(merged.avg_4wk, 1.0); // 4 / 4 weeks
    assert_eq!(merged.status, "ahead");

    let reviews = &pace.metrics[1];
    assert_eq!((reviews.so_far, reviews.avg_4wk), (0.0, 10.0));
    assert_eq!(reviews.status, "behind");
}

#[test]
fn small_differences_early_in_the_week_count_as_on_track() {
    let mut events = tuesday(0, "pr_merged", 1);
    for w in 1..=4 {
        events.extend(tuesday(w, "pr_merged", 2));
    }
    let pace = compute_pace(&events, KINDS, WEIGHTS, now());
    // 1 vs 2 is -50%, but a single item is noise, not a trend
    assert_eq!(pace.metrics[0].status, "on_track");
}

#[test]
fn overall_row_uses_weighted_score() {
    let mut events = tuesday(0, "pr_merged", 2);
    events.extend(tuesday(0, "pr_reviewed", 2));
    let pace = compute_pace(&events, KINDS, WEIGHTS, now());
    assert_eq!(pace.overall.so_far, 9.0); // 2*3 + 2*1.5
}
