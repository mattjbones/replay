use std::collections::HashMap;
use std::sync::Arc;

use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use tauri::State;

use crate::auth::{AuthManager, AuthStatus};
use crate::config::AppConfig;
use crate::db::{get_activities_for_range, get_cached_summary, set_cached_summary,
    invalidate_all_summaries,
    query_weekly_velocity, query_activity_heatmap, query_cycle_times,
    query_project_distribution, query_off_hours_ratio, query_message_volume,
    query_daily_vectors, query_dow_project};
use crate::db::Database;
use crate::digest::build_digest;
use crate::models::*;
use crate::sync::SyncScheduler;

pub struct AppState {
    pub db: Arc<Database>,
    pub config: std::sync::Mutex<AppConfig>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a period string ("day" | "week" | "month") and an optional ISO date string
/// into a `Period` and the corresponding UTC start/end timestamps.
fn parse_period_range(
    period: &str,
    date: Option<&str>,
) -> Result<(Period, chrono::DateTime<Utc>, chrono::DateTime<Utc>), String> {
    let base_date = match date {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|e| format!("invalid date: {e}"))?,
        None => Utc::now().date_naive(),
    };

    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();

    match period {
        "day" => {
            let start = Utc.from_utc_datetime(&base_date.and_time(midnight));
            let end = start + chrono::Duration::days(1);
            Ok((Period::Day(base_date), start, end))
        }
        "week" => {
            use chrono::Datelike;
            let weekday = base_date.weekday().num_days_from_monday();
            let week_start = base_date - chrono::Duration::days(weekday as i64);
            let start = Utc.from_utc_datetime(&week_start.and_time(midnight));
            let end = start + chrono::Duration::weeks(1);
            Ok((Period::Week(week_start), start, end))
        }
        "month" => {
            use chrono::Datelike;
            let month_start = NaiveDate::from_ymd_opt(base_date.year(), base_date.month(), 1)
                .ok_or("invalid month start")?;
            let next_month = if base_date.month() == 12 {
                NaiveDate::from_ymd_opt(base_date.year() + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(base_date.year(), base_date.month() + 1, 1)
            }
            .ok_or("invalid next month")?;

            let start = Utc.from_utc_datetime(&month_start.and_time(midnight));
            let end = Utc.from_utc_datetime(&next_month.and_time(midnight));
            Ok((Period::Month(month_start), start, end))
        }
        other => Err(format!("unknown period: {other} (expected day, week, or month)")),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_digest(
    state: State<'_, AppState>,
    period: String,
    date: Option<String>,
) -> Result<Digest, String> {
    let (p, start, end) = parse_period_range(&period, date.as_deref())?;
    let activities =
        get_activities_for_range(&state.db, start, end).map_err(|e: rusqlite::Error| e.to_string())?;
    Ok(build_digest(activities, p))
}

#[tauri::command]
pub async fn get_auth_status() -> Result<AuthStatus, String> {
    Ok(AuthManager::get_auth_status())
}

#[tauri::command]
pub async fn save_token(source: String, token: String) -> Result<(), String> {
    let src: Source = source.parse()?;
    AuthManager::set_token(&src, &token)
}

#[tauri::command]
pub async fn save_slack_refresh_token(token: String) -> Result<(), String> {
    AuthManager::set_slack_refresh_token(&token)
}

#[tauri::command]
pub async fn save_anthropic_key(key: String) -> Result<(), String> {
    AuthManager::set_anthropic_key(&key)
}

/// Exchange a Slack refresh token for an access token.
/// Requires client_id and client_secret from the Slack app config.
#[tauri::command]
pub async fn exchange_slack_refresh_token(
    refresh_token: String,
    client_id: String,
    client_secret: String,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://slack.com/api/oauth.v2.access")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("failed to parse response: {e}"))?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("unknown");
        return Err(format!("Slack token exchange failed: {err}"));
    }

    let access_token = body.get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("no access_token in response")?
        .to_string();

    // Store the access token
    AuthManager::set_token(&Source::Slack, &access_token)
        .map_err(|e| format!("failed to store access token: {e}"))?;

    // Store new refresh token if rotated
    if let Some(new_refresh) = body.get("refresh_token").and_then(|v| v.as_str()) {
        let _ = AuthManager::set_slack_refresh_token(new_refresh);
    } else {
        // Store the original refresh token
        let _ = AuthManager::set_slack_refresh_token(&refresh_token);
    }

    Ok(format!("Slack connected! Access token starts with {}...", &access_token[..12.min(access_token.len())]))
}

#[tauri::command]
pub async fn get_all_activities(
    state: State<'_, AppState>,
) -> Result<Vec<Activity>, String> {
    crate::db::get_all_activities(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.conn.lock().map_err(|e| e.to_string())?;

    let act_count: i64 = conn.query_row("SELECT COUNT(*) FROM activities", [], |r| r.get(0))
        .unwrap_or(0);
    let cursor_count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_cursors", [], |r| r.get(0))
        .unwrap_or(0);
    let llm_count: i64 = conn.query_row("SELECT COUNT(*) FROM llm_cache", [], |r| r.get(0))
        .unwrap_or(0);

    tracing::info!("clear_cache: deleting {act_count} activities, {cursor_count} sync cursors, {llm_count} llm cache entries");

    conn.execute_batch(
        "DELETE FROM activities; DELETE FROM sync_cursors; DELETE FROM llm_cache;"
    ).map_err(|e| e.to_string())?;

    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM activities", [], |r| r.get(0))
        .unwrap_or(-1);
    tracing::info!("clear_cache: done. activities remaining: {remaining}");

    Ok(())
}

#[tauri::command]
pub async fn trigger_sync(state: State<'_, AppState>) -> Result<String, String> {
    let db = Arc::clone(&state.db);
    let config = state.config.lock().map_err(|e| e.to_string())?.clone();

    let scheduler = SyncScheduler::new(Arc::clone(&db), config);
    scheduler.run_once().await;

    // Invalidate LLM summaries so they regenerate with fresh data
    // (important when working hours or other settings change).
    invalidate_all_summaries(&db);

    Ok("sync complete".to_string())
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
pub async fn update_config(
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<(), String> {
    config.save();
    *state.config.lock().map_err(|e| e.to_string())? = config;
    Ok(())
}

#[tauri::command]
pub async fn get_llm_summary(
    state: State<'_, AppState>,
    period: String,
    date: Option<String>,
) -> Result<Option<String>, String> {
    let (p, start, end) = parse_period_range(&period, date.as_deref())?;

    // Build a cache key from period + date range.
    let config = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cache_key = format!("summary:{}:{}", period, start.to_rfc3339());
    let ttl = config.ttl.warm_minutes;

    // Check cache first.
    if let Some(cached) = get_cached_summary(&state.db, &cache_key, ttl) {
        return Ok(Some(cached));
    }

    // Fetch activities for the range.
    let activities =
        get_activities_for_range(&state.db, start, end).map_err(|e: rusqlite::Error| e.to_string())?;

    if activities.is_empty() {
        return Ok(None);
    }

    let digest = build_digest(activities, p);

    // Generate summary via claude CLI (preferred) or Anthropic API (fallback).
    let summary = crate::llm::generate_summary(&config.llm, &digest).await?;

    // Cache the result.
    set_cached_summary(&state.db, &cache_key, &summary);

    Ok(Some(summary))
}

// ---------------------------------------------------------------------------
// Chart data
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ChartData {
    pub labels: Vec<String>,
    pub datasets: HashMap<String, Vec<usize>>,
}

#[tauri::command]
pub async fn get_chart_data(
    state: State<'_, AppState>,
    period: String,
    date: Option<String>,
) -> Result<ChartData, String> {
    let (_p, start, end) = parse_period_range(&period, date.as_deref())?;
    let activities =
        get_activities_for_range(&state.db, start, end).map_err(|e| e.to_string())?;

    // Build day-by-day buckets
    let mut labels = Vec::new();
    let mut day_map: HashMap<String, HashMap<String, usize>> = HashMap::new();

    let mut current = start;
    while current < end {
        let label = current.format("%a %d").to_string();
        labels.push(label.clone());
        day_map.insert(label, HashMap::new());
        current = current + chrono::Duration::days(1);
    }

    let series = ["merges", "reviews", "commits", "issues", "messages"];

    for a in &activities {
        let day_label = a.occurred_at.format("%a %d").to_string();
        if let Some(bucket) = day_map.get_mut(&day_label) {
            let key = match a.kind {
                ActivityKind::PrMerged => "merges",
                ActivityKind::PrReviewed => "reviews",
                ActivityKind::CommitPushed => "commits",
                ActivityKind::PrOpened | ActivityKind::IssueOpened |
                ActivityKind::IssueCreated | ActivityKind::IssueCompleted |
                ActivityKind::IssueUpdated | ActivityKind::IssueCommented |
                ActivityKind::IssuePrioritized => "issues",
                ActivityKind::MessageSent | ActivityKind::ThreadReplied |
                ActivityKind::ReactionAdded => "messages",
                _ => "issues",
            };
            *bucket.entry(key.to_string()).or_insert(0) += 1;
        }
    }

    let mut datasets = HashMap::new();
    for s in &series {
        let data: Vec<usize> = labels
            .iter()
            .map(|l| day_map.get(l).and_then(|b| b.get(*s)).copied().unwrap_or(0))
            .collect();
        datasets.insert(s.to_string(), data);
    }

    Ok(ChartData { labels, datasets })
}

// ---------------------------------------------------------------------------
// Feature breakdown
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct FeatureBreakdown {
    pub project: String,
    pub count: usize,
    pub kinds: HashMap<String, usize>,
}

#[tauri::command]
pub async fn get_feature_breakdown(
    state: State<'_, AppState>,
    period: String,
    date: Option<String>,
) -> Result<Vec<FeatureBreakdown>, String> {
    let (_p, start, end) = parse_period_range(&period, date.as_deref())?;
    let activities =
        get_activities_for_range(&state.db, start, end).map_err(|e| e.to_string())?;

    let mut projects: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for a in &activities {
        let project = a.project.clone().unwrap_or_else(|| "Other".to_string());
        let kinds = projects.entry(project).or_default();
        *kinds.entry(a.kind.to_string()).or_insert(0) += 1;
    }

    let mut result: Vec<FeatureBreakdown> = projects
        .into_iter()
        .map(|(project, kinds)| {
            let count = kinds.values().sum();
            FeatureBreakdown { project, count, kinds }
        })
        .collect();

    result.sort_by(|a, b| b.count.cmp(&a.count));
    Ok(result)
}

// ---------------------------------------------------------------------------
// Standup
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_standup(
    state: State<'_, AppState>,
    date: Option<String>,
) -> Result<Option<String>, String> {
    let base_date = match date.as_deref() {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|e| format!("invalid date: {e}"))?,
        None => Utc::now().date_naive(),
    };

    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    let today_start = Utc.from_utc_datetime(&base_date.and_time(midnight));
    let today_end = today_start + chrono::Duration::days(1);

    let config = state.config.lock().map_err(|e| e.to_string())?.clone();
    let cache_key = format!("standup:v2:{}", base_date);
    let ttl = config.ttl.warm_minutes;
    if let Some(cached) = get_cached_summary(&state.db, &cache_key, ttl) {
        return Ok(Some(cached));
    }

    let today_activities =
        get_activities_for_range(&state.db, today_start, today_end).map_err(|e| e.to_string())?;

    // Fetch open Linear tickets (urgent + high priority only) and open GitHub PRs
    let (open_tickets, open_prs) = tokio::join!(
        crate::integrations::linear::fetch_open_tickets(),
        crate::integrations::github::fetch_open_prs(&config),
    );

    let urgent_tickets: Vec<_> = open_tickets
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.priority >= 1 && t.priority <= 2) // 1=urgent, 2=high
        .collect();

    let open_prs = open_prs.unwrap_or_default();

    if today_activities.is_empty() && urgent_tickets.is_empty() && open_prs.is_empty() {
        return Ok(None);
    }

    let format_activities = |acts: &[Activity]| -> String {
        acts.iter()
            .map(|a| {
                let project = a.project.as_deref().unwrap_or("");
                format!("[{}] {}: {} ({})", a.source, a.kind, a.title, project)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let format_tickets = urgent_tickets.iter()
        .map(|t| format!("[P{}] {} — {} ({})", t.priority, t.identifier, t.title, t.state))
        .collect::<Vec<_>>()
        .join("\n");

    let format_prs = open_prs.iter()
        .map(|pr| {
            let status = if pr.state == "draft" { "draft" } else { "open" };
            format!("[{}] #{} — {} ({})", status, pr.number, pr.title, pr.repo)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Generate a concise daily standup update in markdown with two sections:\n\
         ## What I Did\n(based on today's activities — what was shipped, reviewed, or completed)\n\
         ## What I'm Working On\n(based on my open PRs and high-priority Linear tickets)\n\n\
         Keep each section to 3-5 bullet points. Be specific with ticket/PR numbers.\n\
         If there's nothing for a section, omit it.\n\n\
         Today's activities:\n{activities}\n\n\
         My open/draft PRs:\n{prs}\n\n\
         My urgent/high-priority Linear tickets:\n{tickets}",
        activities = format_activities(&today_activities),
        prs = if format_prs.is_empty() { "(none)".to_string() } else { format_prs },
        tickets = if format_tickets.is_empty() { "(none)".to_string() } else { format_tickets },
    );

    let summary = crate::llm::generate_from_prompt(&config.llm, &prompt).await?;
    set_cached_summary(&state.db, &cache_key, &summary);
    Ok(Some(summary))
}

// ---------------------------------------------------------------------------
// Linear open tickets
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_open_tickets() -> Result<Vec<crate::integrations::linear::OpenTicket>, String> {
    crate::integrations::linear::fetch_open_tickets().await
}

#[tauri::command]
pub async fn get_open_prs(
    state: State<'_, AppState>,
) -> Result<Vec<crate::integrations::github::OpenPr>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?.clone();
    crate::integrations::github::fetch_open_prs(&config).await
}

#[tauri::command]
pub async fn get_github_issues(
    state: State<'_, AppState>,
) -> Result<Vec<crate::integrations::github::GitHubIssue>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?.clone();
    crate::integrations::github::fetch_github_issues(&config).await
}

#[tauri::command]
pub async fn get_trends_ai_summary(
    state: State<'_, AppState>,
    prompt: String,
) -> Result<Option<String>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?.clone();
    match crate::llm::generate_from_prompt(&config.llm, &prompt).await {
        Ok(summary) => Ok(Some(summary)),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub async fn get_heatmap_activities(
    state: State<'_, AppState>,
    dow: i32,
    hour: i32,
) -> Result<Vec<Activity>, String> {
    let since = Utc::now() - chrono::Duration::weeks(12);
    crate::db::get_activities_for_dow_hour(&state.db, since, dow, hour)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Trends & ML
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TrendsData {
    pub velocity: VelocityData,
    pub heatmap: Vec<HeatmapCell>,
    pub cycle_time: Vec<WeeklyAvg>,
    pub focus: FocusData,
    pub prediction: PredictionData,
    pub burnout: BurnoutData,
    pub anomalies: Vec<AnomalyWeek>,
    pub day_clusters: DayClusterData,
    pub project_prediction: Vec<ProjectPrediction>,
    pub productivity: ProductivityData,
}

#[derive(Serialize)]
pub struct VelocityData {
    pub weeks: Vec<String>,
    pub series: HashMap<String, Vec<f64>>,
    pub trend_slopes: HashMap<String, f64>,
}

#[derive(Serialize)]
pub struct HeatmapCell {
    pub day: i32,
    pub hour: i32,
    pub count: i64,
}

#[derive(Serialize)]
pub struct WeeklyAvg {
    pub week: String,
    pub avg_hours: f64,
}

#[derive(Serialize)]
pub struct FocusData {
    pub weeks: Vec<String>,
    pub projects: HashMap<String, Vec<f64>>,
    pub fragmentation_index: Vec<f64>,
}

#[derive(Serialize)]
pub struct PredictionData {
    pub weeks_ahead: Vec<String>,
    pub forecasts: HashMap<String, Vec<f64>>,
    pub confidence: String,
}

#[derive(Serialize)]
pub struct BurnoutData {
    pub weeks: Vec<String>,
    pub off_hours_pct: Vec<f64>,
    pub message_volume: Vec<f64>,
    pub trend_direction: String,
}

#[derive(Serialize)]
pub struct AnomalyWeek {
    pub week: String,
    pub kind: String,
    pub value: f64,
    pub z_score: f64,
    pub direction: String, // "high" or "low"
}

#[derive(Serialize)]
pub struct DayClusterData {
    pub clusters: Vec<DayCluster>,
    pub days: Vec<ClassifiedDay>,
}

#[derive(Serialize)]
pub struct DayCluster {
    pub name: String,
    pub centroid: Vec<f64>,  // [commits, prs, reviews, issues, messages]
    pub count: usize,
}

#[derive(Serialize)]
pub struct ClassifiedDay {
    pub date: String,
    pub cluster: String,
}

#[derive(Serialize)]
pub struct ProjectPrediction {
    pub project: String,
    pub probability: f64,
}

#[derive(Serialize)]
pub struct ProductivityData {
    pub weeks: Vec<String>,
    pub scores: Vec<f64>,
    pub current_score: f64,
    pub trend: String,  // "improving", "declining", "stable"
    pub baseline_avg: f64,
}

/// If `weeks` has fewer than 12 entries, prepend one zero-data week before the
/// earliest week so charts don't start right at the y-axis edge.
/// Returns the label for the inserted week (if any) so callers can pad series too.
fn pad_weeks_to_min(weeks: &mut Vec<String>, min_weeks: usize) -> Option<String> {
    if weeks.len() >= min_weeks || weeks.is_empty() {
        return None;
    }
    // Parse the earliest week label "YYYY-WNN" and step back one week
    let first = &weeks[0];
    if let (Some(year), Some(wnum)) = (
        first.get(..4).and_then(|s| s.parse::<i32>().ok()),
        first.get(6..).and_then(|s| s.parse::<u32>().ok()),
    ) {
        let (py, pw) = if wnum <= 1 { (year - 1, 52) } else { (year, wnum - 1) };
        let label = format!("{py}-W{pw:02}");
        weeks.insert(0, label.clone());
        Some(label)
    } else {
        None
    }
}

// --- ML helpers ---

fn linear_regression(ys: &[f64]) -> (f64, f64) {
    let n = ys.len() as f64;
    if n < 2.0 {
        return (0.0, ys.first().copied().unwrap_or(0.0));
    }
    let x_mean = (n - 1.0) / 2.0;
    let y_mean = ys.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (i, y) in ys.iter().enumerate() {
        let x = i as f64;
        num += (x - x_mean) * (y - y_mean);
        den += (x - x_mean) * (x - x_mean);
    }
    let slope = if den > 0.0 { num / den } else { 0.0 };
    let intercept = y_mean - slope * x_mean;
    (slope, intercept)
}

/// Holt-Winters double exponential smoothing for forecasting.
/// Returns `ahead` future values.
fn holt_winters_forecast(ys: &[f64], ahead: usize, alpha: f64, beta: f64) -> Vec<f64> {
    if ys.is_empty() { return vec![0.0; ahead]; }
    if ys.len() == 1 { return vec![ys[0]; ahead]; }
    let mut level = ys[0];
    let mut trend = ys[1] - ys[0];
    for &y in &ys[1..] {
        let prev_level = level;
        level = alpha * y + (1.0 - alpha) * (prev_level + trend);
        trend = beta * (level - prev_level) + (1.0 - beta) * trend;
    }
    (1..=ahead).map(|i| (level + trend * i as f64).max(0.0)).collect()
}

/// Z-score anomaly detection. Returns indices where |z| > threshold.
fn detect_anomalies(ys: &[f64], threshold: f64) -> Vec<(usize, f64)> {
    let n = ys.len() as f64;
    if n < 3.0 { return Vec::new(); }
    let mean = ys.iter().sum::<f64>() / n;
    let variance = ys.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    if std_dev < 0.001 { return Vec::new(); }
    ys.iter().enumerate()
        .filter_map(|(i, y)| {
            let z = (y - mean) / std_dev;
            if z.abs() > threshold { Some((i, z)) } else { None }
        })
        .collect()
}

/// K-means clustering on f64 vectors. Returns (assignments, centroids).
fn kmeans(data: &[Vec<f64>], k: usize, max_iter: usize) -> (Vec<usize>, Vec<Vec<f64>>) {
    if data.is_empty() || k == 0 { return (Vec::new(), Vec::new()); }
    let dim = data[0].len();
    let k = k.min(data.len());
    // Init centroids: evenly spaced from data
    let step = data.len().max(1) / k.max(1);
    let mut centroids: Vec<Vec<f64>> = (0..k)
        .map(|i| data[(i * step).min(data.len() - 1)].clone())
        .collect();
    let mut assignments = vec![0usize; data.len()];

    for _ in 0..max_iter {
        // Assign
        let mut changed = false;
        for (i, point) in data.iter().enumerate() {
            let nearest = centroids.iter().enumerate()
                .min_by(|(_, a), (_, b)| {
                    let da: f64 = a.iter().zip(point).map(|(x, y)| (x - y).powi(2)).sum();
                    let db: f64 = b.iter().zip(point).map(|(x, y)| (x - y).powi(2)).sum();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            if assignments[i] != nearest { changed = true; }
            assignments[i] = nearest;
        }
        if !changed { break; }
        // Recompute centroids
        for c in 0..k {
            let members: Vec<&Vec<f64>> = data.iter().zip(&assignments)
                .filter(|(_, &a)| a == c)
                .map(|(d, _)| d)
                .collect();
            if members.is_empty() { continue; }
            let n = members.len() as f64;
            centroids[c] = (0..dim)
                .map(|d| members.iter().map(|m| m[d]).sum::<f64>() / n)
                .collect();
        }
    }
    (assignments, centroids)
}

/// Naive Bayes: P(project | dow) using frequency counts.
fn naive_bayes_predict(
    dow_project: &[(i32, String, i64)],
    target_dow: i32,
) -> Vec<(String, f64)> {
    let total_for_dow: i64 = dow_project.iter()
        .filter(|(d, _, _)| *d == target_dow)
        .map(|(_, _, c)| c)
        .sum();
    if total_for_dow == 0 { return Vec::new(); }
    let mut probs: Vec<(String, f64)> = dow_project.iter()
        .filter(|(d, _, _)| *d == target_dow)
        .map(|(_, proj, cnt)| (proj.clone(), *cnt as f64 / total_for_dow as f64))
        .collect();
    probs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    probs.truncate(5);
    probs
}

#[tauri::command]
pub async fn get_trends_data(
    state: State<'_, AppState>,
) -> Result<TrendsData, String> {
    let since = Utc::now() - chrono::Duration::weeks(12);

    // Run all queries
    let velocity_rows = query_weekly_velocity(&state.db, since).map_err(|e| e.to_string())?;
    let heatmap_rows = query_activity_heatmap(&state.db, since).map_err(|e| e.to_string())?;
    let cycle_rows = query_cycle_times(&state.db, since).map_err(|e| e.to_string())?;
    let project_rows = query_project_distribution(&state.db, since).map_err(|e| e.to_string())?;
    let offhours_rows = query_off_hours_ratio(&state.db, since).map_err(|e| e.to_string())?;
    let msg_rows = query_message_volume(&state.db, since).map_err(|e| e.to_string())?;
    let daily_rows = query_daily_vectors(&state.db, since).map_err(|e| e.to_string())?;
    let dow_proj_rows = query_dow_project(&state.db, since).map_err(|e| e.to_string())?;

    // --- Velocity ---
    let key_kinds = ["pr_merged", "issue_completed", "commit_pushed", "pr_reviewed"];
    let mut week_set: Vec<String> = velocity_rows.iter().map(|(w, _, _)| w.clone()).collect();
    week_set.sort();
    week_set.dedup();
    pad_weeks_to_min(&mut week_set, 12);

    let mut vel_series: HashMap<String, Vec<f64>> = HashMap::new();
    for kind in &key_kinds {
        let values: Vec<f64> = week_set.iter().map(|w| {
            velocity_rows.iter()
                .filter(|(rw, rk, _)| rw == w && rk == *kind)
                .map(|(_, _, c)| *c as f64)
                .sum()
        }).collect();
        vel_series.insert(kind.to_string(), values);
    }
    // The padded week has no matching rows so its sum is already 0 — no extra fixup needed.

    let mut trend_slopes: HashMap<String, f64> = HashMap::new();
    for (kind, values) in &vel_series {
        let (slope, _) = linear_regression(values);
        trend_slopes.insert(kind.clone(), slope);
    }

    let velocity = VelocityData {
        weeks: week_set.clone(),
        series: vel_series.clone(),
        trend_slopes: trend_slopes.clone(),
    };

    // --- Anomaly Detection (Z-score on weekly velocity) ---
    let mut anomalies: Vec<AnomalyWeek> = Vec::new();
    for kind in &key_kinds {
        if let Some(values) = vel_series.get(*kind) {
            for (idx, z) in detect_anomalies(values, 1.5) {
                if idx < week_set.len() {
                    anomalies.push(AnomalyWeek {
                        week: week_set[idx].clone(),
                        kind: kind.to_string(),
                        value: values[idx],
                        z_score: (z * 100.0).round() / 100.0,
                        direction: if z > 0.0 { "high".to_string() } else { "low".to_string() },
                    });
                }
            }
        }
    }
    anomalies.sort_by(|a, b| b.z_score.abs().partial_cmp(&a.z_score.abs()).unwrap_or(std::cmp::Ordering::Equal));

    // --- Heatmap ---
    let heatmap: Vec<HeatmapCell> = heatmap_rows.into_iter()
        .map(|(day, hour, count)| HeatmapCell { day, hour, count })
        .collect();

    // --- Cycle Time ---
    let mut ct_map: HashMap<String, Vec<f64>> = HashMap::new();
    for (week, hours) in &cycle_rows {
        ct_map.entry(week.clone()).or_default().push(*hours);
    }
    let mut cycle_time: Vec<WeeklyAvg> = ct_map.into_iter().map(|(week, hours)| {
        let avg = hours.iter().sum::<f64>() / hours.len() as f64;
        WeeklyAvg { week, avg_hours: avg }
    }).collect();
    cycle_time.sort_by(|a, b| a.week.cmp(&b.week));
    if cycle_time.len() < 12 && !cycle_time.is_empty() {
        let mut ct_weeks: Vec<String> = cycle_time.iter().map(|c| c.week.clone()).collect();
        if let Some(label) = pad_weeks_to_min(&mut ct_weeks, 12) {
            cycle_time.insert(0, WeeklyAvg { week: label, avg_hours: 0.0 });
        }
    }

    // --- Focus ---
    let mut focus_weeks: Vec<String> = project_rows.iter().map(|(w, _, _)| w.clone()).collect();
    focus_weeks.sort();
    focus_weeks.dedup();
    pad_weeks_to_min(&mut focus_weeks, 12);

    let mut proj_totals: HashMap<String, i64> = HashMap::new();
    for (_, proj, cnt) in &project_rows {
        *proj_totals.entry(proj.clone()).or_default() += cnt;
    }
    let mut top_projects: Vec<(String, i64)> = proj_totals.into_iter().collect();
    top_projects.sort_by(|a, b| b.1.cmp(&a.1));
    let top_names: Vec<String> = top_projects.iter().take(6).map(|(n, _)| n.clone()).collect();

    let mut focus_projects: HashMap<String, Vec<f64>> = HashMap::new();
    let mut frag_index: Vec<f64> = Vec::new();
    for w in &focus_weeks {
        let mut active_count = 0u32;
        for name in &top_names {
            let cnt = project_rows.iter()
                .filter(|(rw, rp, _)| rw == w && rp == name)
                .map(|(_, _, c)| *c as f64)
                .sum::<f64>();
            focus_projects.entry(name.clone()).or_insert_with(|| vec![0.0; focus_weeks.len()]);
            let idx = focus_weeks.iter().position(|fw| fw == w).unwrap();
            focus_projects.get_mut(name).unwrap()[idx] = cnt;
            if cnt > 0.0 { active_count += 1; }
        }
        frag_index.push(active_count as f64);
    }

    let focus = FocusData {
        weeks: focus_weeks,
        projects: focus_projects,
        fragmentation_index: frag_index,
    };

    // --- Holt-Winters Forecasting (3 weeks ahead) ---
    let n_ahead = 3;
    let mut forecasts: HashMap<String, Vec<f64>> = HashMap::new();
    let mut forecast_weeks: Vec<String> = Vec::new();
    for i in 1..=n_ahead {
        forecast_weeks.push(format!("W+{i}"));
    }
    for kind in &key_kinds {
        if let Some(values) = vel_series.get(*kind) {
            let fc = holt_winters_forecast(values, n_ahead, 0.3, 0.1);
            forecasts.insert(kind.to_string(), fc.iter().map(|v| (v * 10.0).round() / 10.0).collect());
        }
    }
    let confidence = if week_set.len() >= 8 { "high" }
        else if week_set.len() >= 4 { "medium" }
        else { "low" };
    let prediction = PredictionData {
        weeks_ahead: forecast_weeks,
        forecasts,
        confidence: confidence.to_string(),
    };

    // --- Day Clustering (K-means, k=3) ---
    let cluster_kinds = ["commit_pushed", "pr_merged", "pr_reviewed", "issue_completed", "message_sent"];
    let mut day_set: Vec<String> = daily_rows.iter().map(|(d, _, _)| d.clone()).collect();
    day_set.sort();
    day_set.dedup();

    let data_points: Vec<Vec<f64>> = day_set.iter().map(|day| {
        cluster_kinds.iter().map(|kind| {
            daily_rows.iter()
                .filter(|(d, k, _)| d == day && k == *kind)
                .map(|(_, _, c)| *c as f64)
                .sum()
        }).collect()
    }).collect();

    let (assignments, centroids) = kmeans(&data_points, 3, 20);

    // Name clusters by dominant dimension
    let dim_names = ["Coding", "PRs", "Reviews", "Issues", "Comms"];
    let clusters: Vec<DayCluster> = centroids.iter().enumerate().map(|(i, c)| {
        let dominant = c.iter().enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        let name = format!("{} Day", dim_names.get(dominant).unwrap_or(&"Mixed"));
        let count = assignments.iter().filter(|&&a| a == i).count();
        DayCluster { name, centroid: c.iter().map(|v| (v * 10.0).round() / 10.0).collect(), count }
    }).collect();

    let cluster_names: Vec<String> = clusters.iter().map(|c| c.name.clone()).collect();
    let days: Vec<ClassifiedDay> = day_set.iter().zip(&assignments).map(|(date, &a)| {
        ClassifiedDay {
            date: date.clone(),
            cluster: cluster_names.get(a).cloned().unwrap_or_default(),
        }
    }).collect();

    let day_clusters = DayClusterData { clusters, days };

    // --- Project Prediction (Naive Bayes: P(project | tomorrow's dow)) ---
    let tomorrow_dow = (Utc::now() + chrono::Duration::days(1))
        .format("%w")
        .to_string()
        .parse::<i32>()
        .unwrap_or(1);
    let proj_preds = naive_bayes_predict(&dow_proj_rows, tomorrow_dow);
    let project_prediction: Vec<ProjectPrediction> = proj_preds.into_iter()
        .map(|(project, probability)| ProjectPrediction {
            project,
            probability: (probability * 1000.0).round() / 1000.0,
        })
        .collect();

    // --- Productivity Score ---
    // Weighted composite: PRs merged (3) + issues completed (2) + reviews (1.5) + commits (0.5)
    let weights: &[(&str, f64)] = &[
        ("pr_merged", 3.0), ("issue_completed", 2.0),
        ("pr_reviewed", 1.5), ("commit_pushed", 0.5),
    ];
    let weekly_scores: Vec<f64> = week_set.iter().enumerate().map(|(i, _)| {
        weights.iter().map(|(kind, w)| {
            vel_series.get(*kind).and_then(|v| v.get(i)).copied().unwrap_or(0.0) * w
        }).sum()
    }).collect();

    let baseline_avg = if weekly_scores.is_empty() { 0.0 }
        else { weekly_scores.iter().sum::<f64>() / weekly_scores.len() as f64 };
    let current_score = weekly_scores.last().copied().unwrap_or(0.0);
    let (score_slope, _) = linear_regression(&weekly_scores);
    let prod_trend = if score_slope > 0.5 { "improving" }
        else if score_slope < -0.5 { "declining" }
        else { "stable" };

    let productivity = ProductivityData {
        weeks: week_set.clone(),
        scores: weekly_scores.iter().map(|s| (s * 10.0).round() / 10.0).collect(),
        current_score: (current_score * 10.0).round() / 10.0,
        trend: prod_trend.to_string(),
        baseline_avg: (baseline_avg * 10.0).round() / 10.0,
    };

    // --- Burnout ---
    let mut burnout_weeks: Vec<String> = offhours_rows.iter().map(|(w, _, _)| w.clone()).collect();
    burnout_weeks.sort();
    pad_weeks_to_min(&mut burnout_weeks, 12);

    let off_hours_pct: Vec<f64> = burnout_weeks.iter().map(|w| {
        offhours_rows.iter()
            .find(|(rw, _, _)| rw == w)
            .map(|(_, total, off)| if *total > 0 { *off as f64 / *total as f64 * 100.0 } else { 0.0 })
            .unwrap_or(0.0)
    }).collect();

    let msg_volume: Vec<f64> = burnout_weeks.iter().map(|w| {
        msg_rows.iter()
            .find(|(rw, _)| rw == w)
            .map(|(_, c)| *c as f64)
            .unwrap_or(0.0)
    }).collect();

    let (oh_slope, _) = linear_regression(&off_hours_pct);
    let trend_direction = if oh_slope > 1.0 { "increasing" }
        else if oh_slope < -1.0 { "decreasing" }
        else { "stable" };

    let burnout = BurnoutData {
        weeks: burnout_weeks,
        off_hours_pct,
        message_volume: msg_volume,
        trend_direction: trend_direction.to_string(),
    };

    Ok(TrendsData {
        velocity, heatmap, cycle_time, focus, prediction, burnout,
        anomalies, day_clusters, project_prediction, productivity,
    })
}
