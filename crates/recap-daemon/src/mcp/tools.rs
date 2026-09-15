use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::wrapper::Parameters,
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use recap_core::config::AppConfig;
use recap_core::db::Database;
use recap_core::time::parse_period_range;

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

/// Cooldown in seconds between manual sync triggers.
const SYNC_COOLDOWN_SECS: i64 = 60;

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
    /// When `trigger_sync` last *attempted* a pass, successful or not.
    /// Serialised through a mutex so concurrent calls cannot both pass the cooldown.
    last_sync_attempt: Arc<tokio::sync::Mutex<Option<Instant>>>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

impl RecapServer {
    pub fn new(db: Arc<Database>, config: AppConfig) -> Self {
        Self {
            db,
            config,
            last_sync_attempt: Arc::new(tokio::sync::Mutex::new(None)),
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
        // Serialize config, then strip sensitive fields before exposing to MCP clients.
        let mut value = serde_json::to_value(&self.config)
            .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;
        if let Some(slack) = value.get_mut("slack").and_then(|v| v.as_object_mut()) {
            slack.remove("client_id");
            slack.remove("client_secret");
        }
        let text = serde_json::to_string_pretty(&value)
            .map_err(|e| McpError::internal_error(format!("serialization error: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    /// Trigger an immediate sync pass.
    #[tool(description = "Trigger an immediate sync pass across all connected integrations.")]
    async fn trigger_sync(&self) -> Result<CallToolResult, McpError> {
        // Cooldown is keyed on the last *attempt*, held in-process. Keying on
        // `sync_cursors.last_sync` never binds on a fresh DB or when every
        // integration is failing, since that column only advances on success.
        let mut last_attempt = self.last_sync_attempt.lock().await;
        if let Some(last) = *last_attempt {
            let elapsed = last.elapsed().as_secs() as i64;
            if elapsed < SYNC_COOLDOWN_SECS {
                let remaining = SYNC_COOLDOWN_SECS - elapsed;
                return Ok(CallToolResult::success(vec![Content::text(
                    format!("sync was run recently, try again in {remaining}s"),
                )]));
            }
        }
        *last_attempt = Some(Instant::now());

        let before = recap_core::db::get_latest_sync_time(&self.db);
        let scheduler = recap_core::sync::SyncScheduler::new(
            Arc::clone(&self.db),
            self.config.clone(),
        );
        scheduler.run_once().await;
        let after = recap_core::db::get_latest_sync_time(&self.db);

        // Only throw away cached LLM summaries if a source actually advanced;
        // run_once itself skips sources whose data is still within the hot TTL.
        if after > before {
            recap_core::db::invalidate_all_summaries(&self.db);
            Ok(CallToolResult::success(vec![Content::text("sync complete: new data fetched")]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                "sync complete: all sources already fresh, nothing fetched",
            )]))
        }
    }

    /// Search activities by text query.
    #[tool(description = "Substring search over activity title and description (case-insensitive LIKE, not full-text). Returns up to 100 most recent matches.")]
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
