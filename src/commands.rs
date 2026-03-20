use std::collections::HashMap;
use std::sync::Arc;

use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use tauri::State;

use crate::auth::{AuthManager, AuthStatus};
use crate::config::AppConfig;
use crate::db::{get_activities_for_range, get_cached_summary, set_cached_summary};
use crate::db::Database;
use crate::digest::build_digest;
use crate::models::*;
use crate::sync::SyncScheduler;

pub struct AppState {
    pub db: Arc<Database>,
    pub config: AppConfig,
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
    let config = state.config.clone();

    let scheduler = SyncScheduler::new(db, config);
    scheduler.run_once().await;

    Ok("sync complete".to_string())
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.clone())
}

#[tauri::command]
pub async fn update_config(config: AppConfig) -> Result<(), String> {
    config.save();
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
    let cache_key = format!("summary:{}:{}", period, start.to_rfc3339());
    let ttl = state.config.ttl.warm_minutes;

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
    let summary = crate::llm::generate_summary(&state.config.llm, &digest).await?;

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

    // Also get yesterday for context
    let yesterday_start = today_start - chrono::Duration::days(1);

    let cache_key = format!("standup:{}", base_date);
    let ttl = state.config.ttl.warm_minutes;
    if let Some(cached) = get_cached_summary(&state.db, &cache_key, ttl) {
        return Ok(Some(cached));
    }

    let today_activities =
        get_activities_for_range(&state.db, today_start, today_end).map_err(|e| e.to_string())?;
    let yesterday_activities =
        get_activities_for_range(&state.db, yesterday_start, today_start).map_err(|e| e.to_string())?;

    if today_activities.is_empty() && yesterday_activities.is_empty() {
        return Ok(None);
    }

    let format_list = |acts: &[Activity]| -> String {
        acts.iter()
            .map(|a| {
                let project = a.project.as_deref().unwrap_or("");
                format!("[{}] {}: {} ({})", a.source, a.kind, a.title, project)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "Generate a concise standup update in markdown with two sections:\n\
         ## What I Did\n(based on yesterday's and today's completed activities)\n\
         ## What I Will Do\n(infer from in-progress work, open PRs, started issues)\n\n\
         Keep each section to 3-5 bullet points. Be specific with ticket/PR numbers.\n\n\
         Yesterday's activities:\n{}\n\nToday's activities:\n{}",
        format_list(&yesterday_activities),
        format_list(&today_activities),
    );

    let result = generate_standup_via_cli(&prompt).await;
    match result {
        Ok(summary) => {
            set_cached_summary(&state.db, &cache_key, &summary);
            Ok(Some(summary))
        }
        Err(e) => Err(e),
    }
}

async fn generate_standup_via_cli(prompt: &str) -> Result<String, String> {
    let prompt = prompt.to_string();
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new("claude")
            .args(["--print", &prompt])
            .output()
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
    .map_err(|e| format!("failed to run claude CLI: {e}"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("claude CLI error: {stderr}"));
    }

    let output = String::from_utf8_lossy(&result.stdout).trim().to_string();
    if output.is_empty() {
        return Err("empty response from claude".to_string());
    }
    Ok(output)
}
