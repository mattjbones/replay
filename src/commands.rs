use std::sync::Arc;

use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use tauri::State;

use crate::auth::{AuthManager, AuthStatus};
use crate::config::AppConfig;
use crate::db::{get_activities_for_range, get_cached_summary};
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

    // If LLM is not enabled, return None immediately.
    if !state.config.llm.enabled {
        return Ok(None);
    }

    // Fetch activities for the range.
    let activities =
        get_activities_for_range(&state.db, start, end).map_err(|e: rusqlite::Error| e.to_string())?;
    let _digest = build_digest(activities, p);

    // TODO: Call Claude API with the digest to generate a summary.
    // For now, return None until the LLM integration is implemented.
    Ok(None)
}
