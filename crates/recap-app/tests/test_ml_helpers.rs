use recap_app::commands::{
    build_off_hours_rows, detect_anomalies, holt_winters_forecast, kmeans, linear_regression,
    naive_bayes_predict,
};
use recap_core::config::WorkingHoursConfig;
use recap_core::models::{Activity, ActivityKind, Source};
use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// linear_regression
// ---------------------------------------------------------------------------

#[test]
fn linear_regression_empty_input() {
    let (slope, intercept) = linear_regression(&[]);
    assert_eq!(slope, 0.0);
    assert_eq!(intercept, 0.0);
}

#[test]
fn linear_regression_single_value() {
    let (slope, intercept) = linear_regression(&[5.0]);
    assert_eq!(slope, 0.0);
    assert_eq!(intercept, 5.0);
}

#[test]
fn linear_regression_perfect_line() {
    // y = 2x + 1  => points (0,1), (1,3), (2,5), (3,7)
    let (slope, intercept) = linear_regression(&[1.0, 3.0, 5.0, 7.0]);
    assert!((slope - 2.0).abs() < 1e-10);
    assert!((intercept - 1.0).abs() < 1e-10);
}

#[test]
fn linear_regression_flat_line() {
    let (slope, _) = linear_regression(&[3.0, 3.0, 3.0, 3.0]);
    assert!((slope).abs() < 1e-10);
}

#[test]
fn linear_regression_negative_slope() {
    // y = -1x + 4 => (0,4), (1,3), (2,2), (3,1)
    let (slope, intercept) = linear_regression(&[4.0, 3.0, 2.0, 1.0]);
    assert!((slope - (-1.0)).abs() < 1e-10);
    assert!((intercept - 4.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// holt_winters_forecast
// ---------------------------------------------------------------------------

#[test]
fn holt_winters_empty_returns_zeros() {
    let result = holt_winters_forecast(&[], 3, 0.3, 0.1);
    assert_eq!(result.len(), 3);
    assert!(result.iter().all(|&v| v == 0.0));
}

#[test]
fn holt_winters_single_value_repeats() {
    let result = holt_winters_forecast(&[5.0], 3, 0.3, 0.1);
    assert_eq!(result.len(), 3);
    assert!(result.iter().all(|&v| v == 5.0));
}

#[test]
fn holt_winters_increasing_trend_produces_increasing_forecast() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let forecast = holt_winters_forecast(&data, 3, 0.3, 0.1);
    assert_eq!(forecast.len(), 3);
    // Each forecast value should be greater than the previous
    assert!(forecast[0] > 8.0);
    assert!(forecast[1] > forecast[0]);
    assert!(forecast[2] > forecast[1]);
}

#[test]
fn holt_winters_forecasts_are_non_negative() {
    let data = vec![3.0, 2.0, 1.0, 0.5, 0.1];
    let forecast = holt_winters_forecast(&data, 5, 0.3, 0.1);
    assert!(forecast.iter().all(|&v| v >= 0.0));
}

// ---------------------------------------------------------------------------
// detect_anomalies
// ---------------------------------------------------------------------------

#[test]
fn detect_anomalies_empty_input() {
    let result = detect_anomalies(&[], 1.5);
    assert!(result.is_empty());
}

#[test]
fn detect_anomalies_too_few_points() {
    let result = detect_anomalies(&[1.0, 2.0], 1.5);
    assert!(result.is_empty());
}

#[test]
fn detect_anomalies_uniform_data_has_none() {
    let result = detect_anomalies(&[5.0, 5.0, 5.0, 5.0, 5.0], 1.5);
    assert!(result.is_empty());
}

#[test]
fn detect_anomalies_finds_outlier() {
    let data = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 100.0];
    let anomalies = detect_anomalies(&data, 1.5);
    assert!(!anomalies.is_empty());
    // The last element (index 8) should be flagged
    assert!(anomalies.iter().any(|(idx, _)| *idx == 8));
}

// ---------------------------------------------------------------------------
// kmeans
// ---------------------------------------------------------------------------

#[test]
fn kmeans_empty_data() {
    let (assignments, centroids) = kmeans(&[], 3, 20);
    assert!(assignments.is_empty());
    assert!(centroids.is_empty());
}

#[test]
fn kmeans_k_zero() {
    let data = vec![vec![1.0, 2.0]];
    let (assignments, centroids) = kmeans(&data, 0, 20);
    assert!(assignments.is_empty());
    assert!(centroids.is_empty());
}

#[test]
fn kmeans_single_cluster() {
    let data = vec![
        vec![1.0, 0.0],
        vec![1.1, 0.1],
        vec![0.9, -0.1],
    ];
    let (assignments, centroids) = kmeans(&data, 1, 20);
    assert_eq!(assignments.len(), 3);
    assert_eq!(centroids.len(), 1);
    // All points should be assigned to cluster 0
    assert!(assignments.iter().all(|&a| a == 0));
}

#[test]
fn kmeans_two_clear_clusters() {
    let data = vec![
        vec![0.0, 0.0],
        vec![0.1, 0.1],
        vec![0.2, 0.0],
        vec![10.0, 10.0],
        vec![10.1, 10.1],
        vec![10.2, 10.0],
    ];
    let (assignments, _centroids) = kmeans(&data, 2, 50);
    // First 3 should be in one cluster, last 3 in another
    assert_eq!(assignments[0], assignments[1]);
    assert_eq!(assignments[1], assignments[2]);
    assert_eq!(assignments[3], assignments[4]);
    assert_eq!(assignments[4], assignments[5]);
    assert_ne!(assignments[0], assignments[3]);
}

// ---------------------------------------------------------------------------
// naive_bayes_predict
// ---------------------------------------------------------------------------

#[test]
fn naive_bayes_empty_data() {
    let result = naive_bayes_predict(&[], 1);
    assert!(result.is_empty());
}

#[test]
fn naive_bayes_no_data_for_target_dow() {
    let data = vec![
        (1, "project-a".to_string(), 5),
    ];
    // Querying for dow=2, which has no data
    let result = naive_bayes_predict(&data, 2);
    assert!(result.is_empty());
}

#[test]
fn naive_bayes_returns_correct_probabilities() {
    let data = vec![
        (1, "project-a".to_string(), 7),
        (1, "project-b".to_string(), 3),
    ];
    let result = naive_bayes_predict(&data, 1);
    assert_eq!(result.len(), 2);
    // project-a should have 0.7, project-b should have 0.3
    assert_eq!(result[0].0, "project-a");
    assert!((result[0].1 - 0.7).abs() < 1e-10);
    assert_eq!(result[1].0, "project-b");
    assert!((result[1].1 - 0.3).abs() < 1e-10);
}

#[test]
fn naive_bayes_truncates_to_five() {
    let data: Vec<(i32, String, i64)> = (0..10)
        .map(|i| (1, format!("project-{}", i), 10 - i as i64))
        .collect();
    let result = naive_bayes_predict(&data, 1);
    assert_eq!(result.len(), 5);
}

// ---------------------------------------------------------------------------
// build_off_hours_rows
// ---------------------------------------------------------------------------

fn test_activity(occurred_at: &str) -> Activity {
    Activity {
        id: "test-id".to_string(),
        source: Source::GitHub,
        source_id: format!("src-{occurred_at}"),
        kind: ActivityKind::CommitPushed,
        title: "test".to_string(),
        description: None,
        url: "https://example.com".to_string(),
        project: Some("proj".to_string()),
        occurred_at: DateTime::parse_from_rfc3339(occurred_at)
            .unwrap()
            .with_timezone(&Utc),
        metadata: serde_json::Value::Null,
        synced_at: Utc::now(),
    }
}

#[test]
fn off_hours_counts_weekends_and_after_hours() {
    let activities = vec![
        test_activity("2026-03-16T10:00:00Z"), // Monday, in-hours
        test_activity("2026-03-16T21:00:00Z"), // Monday, after-hours
        test_activity("2026-03-21T12:00:00Z"), // Saturday
    ];

    let cfg = WorkingHoursConfig {
        work_start: "09:00".to_string(),
        work_end: "17:00".to_string(),
        working_days: vec!["Mon".to_string(), "Tue".to_string(), "Wed".to_string(), "Thu".to_string(), "Fri".to_string()],
        timezone: "UTC".to_string(),
    };

    let rows = build_off_hours_rows(&activities, &cfg);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 3); // total
    assert_eq!(rows[0].2, 2); // off-hours
}

#[test]
fn off_hours_respects_timezone_conversion() {
    let activities = vec![
        test_activity("2026-01-06T08:30:00Z"), // London 08:30 (off-hours)
        test_activity("2026-01-06T09:30:00Z"), // London 09:30 (in-hours)
    ];

    let cfg = WorkingHoursConfig {
        work_start: "09:00".to_string(),
        work_end: "17:00".to_string(),
        working_days: vec!["Mon".to_string(), "Tue".to_string(), "Wed".to_string(), "Thu".to_string(), "Fri".to_string()],
        timezone: "Europe/London".to_string(),
    };

    let rows = build_off_hours_rows(&activities, &cfg);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 2);
    assert_eq!(rows[0].2, 1);
}
