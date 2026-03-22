use std::sync::Arc;

use chrono::{Datelike, NaiveDate, NaiveTime, TimeZone, Utc};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::wrapper::Parameters,
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use recap_core::config::AppConfig;
use recap_core::db::Database;

use super::resources;

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DigestParams {
    /// The time period: "day", "week", or "month"
    pub period: String,
    /// Optional ISO date (YYYY-MM-DD). Defaults to today.
    pub date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ActivitiesParams {
    /// The time period: "day", "week", or "month"
    pub period: String,
    /// Optional ISO date (YYYY-MM-DD). Defaults to today.
    pub date: Option<String>,
    /// Optional source filter: "github", "linear", "slack", "notion"
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// The search query string (matched against title and description)
    pub query: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_period_range(
    period: &str,
    date: Option<&str>,
) -> Result<(recap_core::models::Period, chrono::DateTime<Utc>, chrono::DateTime<Utc>), String> {
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
            Ok((recap_core::models::Period::Day(base_date), start, end))
        }
        "week" => {
            let weekday = base_date.weekday().num_days_from_monday();
            let week_start = base_date - chrono::Duration::days(weekday as i64);
            let start = Utc.from_utc_datetime(&week_start.and_time(midnight));
            let end = start + chrono::Duration::weeks(1);
            Ok((recap_core::models::Period::Week(week_start), start, end))
        }
        "month" => {
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
            Ok((recap_core::models::Period::Month(month_start), start, end))
        }
        other => Err(format!("unknown period: {other} (expected day, week, or month)")),
    }
}

fn json_result(value: &impl Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn err_result(msg: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![Content::text(msg)]))
}

// ---------------------------------------------------------------------------
// Trend summary types (serializable for MCP output)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TrendsSummary {
    velocity: Vec<VelocityWeek>,
    heatmap: Vec<HeatmapCell>,
    cycle_times: Vec<CycleTimeWeek>,
}

#[derive(Serialize)]
struct VelocityWeek {
    week: String,
    kind: String,
    count: i64,
}

#[derive(Serialize)]
struct HeatmapCell {
    day_of_week: i32,
    hour: i32,
    count: i64,
}

#[derive(Serialize)]
struct CycleTimeWeek {
    week: String,
    avg_hours: f64,
}

// ---------------------------------------------------------------------------
// Server struct (combines tools + resources + ServerHandler)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RecapServer {
    db: Arc<Database>,
    config: AppConfig,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

impl RecapServer {
    pub fn new(db: Arc<Database>, config: AppConfig) -> Self {
        Self {
            db,
            config,
            tool_router: Self::tool_router(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool definitions (generates Self::tool_router())
// ---------------------------------------------------------------------------

#[tool_router]
impl RecapServer {
    /// Get a digest (summary with stats) for a time period.
    #[tool(description = "Get a digest (summary with stats) for a time period. Returns activity counts grouped by source and kind.")]
    async fn get_digest(
        &self,
        params: Parameters<DigestParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;
        let (period, start, end) = parse_period_range(&params.period, params.date.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;

        let activities = recap_core::db::get_activities_for_range(&self.db, start, end)
            .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;

        let digest = recap_core::digest::build_digest(activities, period);
        json_result(&digest)
    }

    /// Get activities for a time period, optionally filtered by source.
    #[tool(description = "Get activities for a time period, optionally filtered by source (github, linear, slack, notion).")]
    async fn get_activities(
        &self,
        params: Parameters<ActivitiesParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;
        let (_period, start, end) = parse_period_range(&params.period, params.date.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;

        let activities = if let Some(ref source_str) = params.source {
            let source: recap_core::models::Source = source_str.parse()
                .map_err(|e: String| McpError::invalid_params(e, None))?;
            recap_core::db::get_activities_by_source(&self.db, &source, start, end)
                .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?
        } else {
            recap_core::db::get_activities_for_range(&self.db, start, end)
                .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?
        };

        json_result(&activities)
    }

    /// Get trend data for the last 12 weeks.
    #[tool(description = "Get trend data including weekly velocity, activity heatmap, and cycle times for the last 12 weeks.")]
    async fn get_trends(&self) -> Result<CallToolResult, McpError> {
        let since = Utc::now() - chrono::Duration::weeks(12);

        let velocity_raw = recap_core::db::query_weekly_velocity(&self.db, since)
            .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;

        let heatmap_raw = recap_core::db::query_activity_heatmap(&self.db, since)
            .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;

        let cycle_raw = recap_core::db::query_cycle_times(&self.db, since)
            .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;

        let velocity: Vec<VelocityWeek> = velocity_raw
            .into_iter()
            .map(|(week, kind, count)| VelocityWeek { week, kind, count })
            .collect();

        let heatmap: Vec<HeatmapCell> = heatmap_raw
            .into_iter()
            .map(|(day_of_week, hour, count)| HeatmapCell { day_of_week, hour, count })
            .collect();

        let mut week_sums: std::collections::HashMap<String, (f64, usize)> =
            std::collections::HashMap::new();
        for (week, hours) in cycle_raw {
            let entry = week_sums.entry(week).or_insert((0.0, 0));
            entry.0 += hours;
            entry.1 += 1;
        }
        let cycle_times: Vec<CycleTimeWeek> = week_sums
            .into_iter()
            .map(|(week, (total, count))| CycleTimeWeek {
                week,
                avg_hours: total / count as f64,
            })
            .collect();

        let summary = TrendsSummary { velocity, heatmap, cycle_times };
        json_result(&summary)
    }

    /// Get open GitHub pull requests.
    #[tool(description = "Get open GitHub pull requests authored by the configured user.")]
    async fn get_open_prs(&self) -> Result<CallToolResult, McpError> {
        match recap_core::integrations::github::fetch_open_prs(&self.config).await {
            Ok(prs) => json_result(&prs),
            Err(e) => err_result(e),
        }
    }

    /// Get open Linear tickets assigned to you.
    #[tool(description = "Get open Linear tickets assigned to you, sorted by priority.")]
    async fn get_open_tickets(&self) -> Result<CallToolResult, McpError> {
        match recap_core::integrations::linear::fetch_open_tickets().await {
            Ok(tickets) => json_result(&tickets),
            Err(e) => err_result(e),
        }
    }

    /// Get open GitHub issues assigned to the configured user.
    #[tool(description = "Get open GitHub issues assigned to the configured user.")]
    async fn get_github_issues(&self) -> Result<CallToolResult, McpError> {
        match recap_core::integrations::github::fetch_github_issues(&self.config).await {
            Ok(issues) => json_result(&issues),
            Err(e) => err_result(e),
        }
    }

    /// Check which integrations are connected.
    #[tool(description = "Check which integrations are connected (GitHub, Linear, Slack, Notion, Anthropic).")]
    async fn get_auth_status(&self) -> Result<CallToolResult, McpError> {
        let status = recap_core::auth::AuthManager::get_auth_status();
        json_result(&status)
    }

    /// Get the current Recap configuration.
    #[tool(description = "Get the current Recap configuration (schedule, integrations, working hours, etc.).")]
    async fn get_config(&self) -> Result<CallToolResult, McpError> {
        json_result(&self.config)
    }

    /// Trigger an immediate sync pass.
    #[tool(description = "Trigger an immediate sync pass across all connected integrations.")]
    async fn trigger_sync(&self) -> Result<CallToolResult, McpError> {
        let scheduler = recap_core::sync::SyncScheduler::new(
            Arc::clone(&self.db),
            self.config.clone(),
        );
        scheduler.run_once().await;
        recap_core::db::invalidate_all_summaries(&self.db);
        Ok(CallToolResult::success(vec![Content::text("sync complete")]))
    }

    /// Search activities by text query.
    #[tool(description = "Search activities by text query (matches against title and description).")]
    async fn search_activities(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let params = params.0;
        let activities = recap_core::db::search_activities(&self.db, &params.query)
            .map_err(|e| McpError::internal_error(format!("db error: {e}"), None))?;
        json_result(&activities)
    }
}

// ---------------------------------------------------------------------------
// ServerHandler impl (tools are auto-wired via #[tool_handler])
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for RecapServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            meta: None,
            next_cursor: None,
            resources: resources::list(),
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        resources::read(request.uri.as_str(), &self.db, &self.config)
    }
}
