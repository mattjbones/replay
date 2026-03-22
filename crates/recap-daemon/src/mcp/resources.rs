use std::sync::Arc;

use chrono::{Datelike, NaiveTime, TimeZone, Utc};
use rmcp::{
    ErrorData as McpError,
    model::*,
};

use recap_core::auth::AuthManager;
use recap_core::config::AppConfig;
use recap_core::db::Database;

/// Build the list of static resources exposed by this server.
pub fn list() -> Vec<Resource> {
    vec![
        Resource::new(
            RawResource::new("recap://digest/today", "today_digest")
                .with_description("Today's activity digest with stats")
                .with_mime_type("application/json"),
            None,
        ),
        Resource::new(
            RawResource::new("recap://digest/week", "week_digest")
                .with_description("This week's activity digest with stats")
                .with_mime_type("application/json"),
            None,
        ),
        Resource::new(
            RawResource::new("recap://status", "status")
                .with_description("Current auth and sync status")
                .with_mime_type("application/json"),
            None,
        ),
    ]
}

/// Read a resource by URI.
pub fn read(
    uri: &str,
    db: &Arc<Database>,
    _config: &AppConfig,
) -> Result<ReadResourceResult, McpError> {
    match uri {
        "recap://digest/today" => read_digest_today(uri, db),
        "recap://digest/week" => read_digest_week(uri, db),
        "recap://status" => read_status(uri),
        _ => Err(McpError::resource_not_found(
            format!("unknown resource: {uri}"),
            None,
        )),
    }
}

fn read_digest_today(uri: &str, db: &Arc<Database>) -> Result<ReadResourceResult, McpError> {
    let today = Utc::now().date_naive();
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    let start = Utc.from_utc_datetime(&today.and_time(midnight));
    let end = start + chrono::Duration::days(1);

    let activities = recap_core::db::get_activities_for_range(db, start, end)
        .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;

    let digest = recap_core::digest::build_digest(
        activities,
        recap_core::models::Period::Day(today),
    );

    let json = serde_json::to_string_pretty(&digest)
        .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;

    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(json, uri).with_mime_type("application/json"),
    ]))
}

fn read_digest_week(uri: &str, db: &Arc<Database>) -> Result<ReadResourceResult, McpError> {
    let today = Utc::now().date_naive();
    let weekday = today.weekday().num_days_from_monday();
    let week_start = today - chrono::Duration::days(weekday as i64);
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    let start = Utc.from_utc_datetime(&week_start.and_time(midnight));
    let end = start + chrono::Duration::weeks(1);

    let activities = recap_core::db::get_activities_for_range(db, start, end)
        .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;

    let digest = recap_core::digest::build_digest(
        activities,
        recap_core::models::Period::Week(week_start),
    );

    let json = serde_json::to_string_pretty(&digest)
        .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;

    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(json, uri).with_mime_type("application/json"),
    ]))
}

fn read_status(uri: &str) -> Result<ReadResourceResult, McpError> {
    let auth = AuthManager::get_auth_status();
    let json = serde_json::to_string_pretty(&auth)
        .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;

    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(json, uri).with_mime_type("application/json"),
    ]))
}
