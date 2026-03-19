use crate::config::AppConfig;
use crate::db::{get_activities_for_range, Database};
use crate::models::Source;
use std::sync::Arc;

use chrono::{Local, TimeZone, Utc};
use tauri_plugin_notification::NotificationExt;

/// Spawn a background task that sends a daily reminder notification at the configured time.
pub fn start_daily_reminder(app_handle: tauri::AppHandle, db: Arc<Database>, config: AppConfig) {
    let time_str = config.schedule.daily_reminder_time.clone();

    let (hour, minute) = parse_hhmm(&time_str).unwrap_or_else(|| {
        tracing::warn!(
            "invalid daily_reminder_time '{time_str}', defaulting to 17:00"
        );
        (17, 0)
    });

    tauri::async_runtime::spawn(async move {
        loop {
            let duration = duration_until_next(hour, minute);
            tracing::info!(
                "next daily reminder in {} seconds",
                duration.as_secs()
            );
            tokio::time::sleep(duration).await;

            let summary = build_summary(&db);

            app_handle
                .notification()
                .builder()
                .title("Recap - Daily Digest")
                .body(&summary)
                .show()
                .unwrap_or_else(|e| tracing::error!("notification failed: {e}"));
        }
    });
}

/// Parse a "HH:MM" string into (hour, minute). Returns `None` on invalid input.
fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.splitn(2, ':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    if hour < 24 && minute < 60 {
        Some((hour, minute))
    } else {
        None
    }
}

/// Calculate the tokio duration until the next occurrence of `hour:minute` in local time.
fn duration_until_next(hour: u32, minute: u32) -> tokio::time::Duration {
    let now = Local::now();
    let today_target = now
        .date_naive()
        .and_hms_opt(hour, minute, 0)
        .expect("valid hms");
    let today_target = Local
        .from_local_datetime(&today_target)
        .single()
        .expect("unambiguous local time");

    let target = if today_target > now {
        today_target
    } else {
        // Already passed today; schedule for tomorrow.
        let tomorrow = now.date_naive().succ_opt().expect("valid date");
        let tomorrow_target = tomorrow.and_hms_opt(hour, minute, 0).expect("valid hms");
        Local
            .from_local_datetime(&tomorrow_target)
            .single()
            .expect("unambiguous local time")
    };

    let delta = target.signed_duration_since(now);
    tokio::time::Duration::from_secs(delta.num_seconds().max(1) as u64)
}

/// Query today's activities and build a human-readable summary string.
fn build_summary(db: &Database) -> String {
    let now = Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let today_start = chrono::DateTime::<Utc>::from_naive_utc_and_offset(today_start, Utc);
    let tomorrow_start = today_start + chrono::Duration::days(1);

    let activities = match get_activities_for_range(db, today_start, tomorrow_start) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("failed to query activities for daily reminder: {e}");
            return "Could not fetch today's activities.".to_string();
        }
    };

    let total = activities.len();
    let github = activities.iter().filter(|a| a.source == Source::GitHub).count();
    let linear = activities.iter().filter(|a| a.source == Source::Linear).count();
    let slack = activities.iter().filter(|a| a.source == Source::Slack).count();
    let notion = activities.iter().filter(|a| a.source == Source::Notion).count();

    format!(
        "{total} activities today: {github} from GitHub, {linear} from Linear, {slack} from Slack, {notion} from Notion"
    )
}
