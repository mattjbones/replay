use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, ActivityKind, Source};

use super::{Integration, IntegrationError};

const LINEAR_API_URL: &str = "https://api.linear.app/graphql";

const ISSUES_QUERY: &str = r#"
query($after: String, $since: DateTimeOrDuration) {
  viewer {
    assignedIssues(
      first: 50
      after: $after
      filter: { updatedAt: { gte: $since } }
      orderBy: updatedAt
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        id
        identifier
        title
        url
        state { name type }
        priority
        priorityLabel
        team { name }
        createdAt
        updatedAt
        history(first: 10, orderBy: createdAt) {
          nodes {
            id
            createdAt
            fromState { name type }
            toState { name type }
          }
        }
      }
    }
  }
}
"#;

pub struct LinearIntegration {
    client: reqwest::Client,
    token: String,
}

impl LinearIntegration {
    pub fn new(_config: AppConfig) -> Self {
        let token = AuthManager::get_token(&Source::Linear)
            .ok()
            .flatten()
            .unwrap_or_default();

        let client = reqwest::Client::new();
        Self { client, token }
    }

    async fn execute_query(
        &self,
        after: Option<&str>,
        since: Option<&str>,
    ) -> Result<serde_json::Value, IntegrationError> {
        let mut variables = serde_json::Map::new();
        if let Some(cursor) = after {
            variables.insert("after".to_string(), serde_json::Value::String(cursor.to_string()));
        }
        if let Some(since_val) = since {
            variables.insert("since".to_string(), serde_json::Value::String(since_val.to_string()));
        }

        let body = serde_json::json!({
            "query": ISSUES_QUERY,
            "variables": variables,
        });

        let response = self
            .client
            .post(LINEAR_API_URL)
            .header("Authorization", &self.token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| IntegrationError::Network(e.to_string()))?;

        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            let body_text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::Auth(format!(
                "Linear returned {status}: {body_text}"
            )));
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            return Err(IntegrationError::RateLimit {
                retry_after_secs: retry_after,
            });
        }

        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::Network(format!(
                "Linear returned {status}: {body_text}"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| IntegrationError::Parse(e.to_string()))?;

        // Check for GraphQL-level errors.
        if let Some(errors) = json.get("errors").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let messages: Vec<&str> = errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect();
                return Err(IntegrationError::Parse(format!(
                    "GraphQL errors: {}",
                    messages.join("; ")
                )));
            }
        }

        Ok(json)
    }
}

#[async_trait]
impl Integration for LinearIntegration {
    fn source(&self) -> Source {
        Source::Linear
    }

    async fn fetch_activities(
        &self,
        since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        let mut all_activities: Vec<Activity> = Vec::new();
        let mut oldest_updated_at: Option<String> = None;
        let mut after_cursor: Option<String> = None;

        loop {
            let json = self
                .execute_query(after_cursor.as_deref(), since)
                .await?;

            let assigned_issues = &json["data"]["viewer"]["assignedIssues"];
            let nodes = assigned_issues["nodes"]
                .as_array()
                .ok_or_else(|| IntegrationError::Parse("missing nodes array".to_string()))?;

            if nodes.is_empty() {
                break;
            }

            for issue in nodes {
                let activities = parse_issue(issue);
                all_activities.extend(activities);

                // Track the oldest updatedAt for cursor.
                if let Some(updated) = issue["updatedAt"].as_str() {
                    match &oldest_updated_at {
                        None => oldest_updated_at = Some(updated.to_string()),
                        Some(existing) => {
                            if updated < existing.as_str() {
                                oldest_updated_at = Some(updated.to_string());
                            }
                        }
                    }
                }
            }

            let page_info = &assigned_issues["pageInfo"];
            let has_next_page = page_info["hasNextPage"].as_bool().unwrap_or(false);
            if has_next_page {
                after_cursor = page_info["endCursor"]
                    .as_str()
                    .map(|s| s.to_string());
            } else {
                break;
            }
        }

        let cursor = oldest_updated_at.unwrap_or_else(|| {
            since
                .map(|s| s.to_string())
                .unwrap_or_else(|| Utc::now().to_rfc3339())
        });

        Ok((all_activities, cursor))
    }
}

// ---------------------------------------------------------------------------
// Issue parsing helpers
// ---------------------------------------------------------------------------

fn parse_issue(issue: &serde_json::Value) -> Vec<Activity> {
    let mut activities = Vec::new();

    let identifier = issue["identifier"].as_str().unwrap_or_default().to_string();
    let title = issue["title"].as_str().unwrap_or("Untitled").to_string();
    let url = issue["url"].as_str().unwrap_or_default().to_string();
    let team_name = issue["team"]["name"].as_str().map(|s| s.to_string());
    let state_type = issue["state"]["type"].as_str().unwrap_or_default();
    let created_at_str = issue["createdAt"].as_str().unwrap_or_default();
    let updated_at_str = issue["updatedAt"].as_str().unwrap_or_default();

    let updated_at: DateTime<Utc> = updated_at_str
        .parse()
        .unwrap_or_else(|_| Utc::now());

    let created_at: Option<DateTime<Utc>> = created_at_str.parse().ok();

    // Determine the primary activity kind from the current state.
    let mut primary_emitted = false;

    if state_type == "completed" {
        let mut activity = Activity::new(
            Source::Linear,
            identifier.clone(),
            ActivityKind::IssueCompleted,
            title.clone(),
            url.clone(),
            updated_at,
        );
        activity.project = team_name.clone();
        activity.metadata = build_metadata(issue);
        activities.push(activity);
        primary_emitted = true;
    } else if state_type == "started" {
        // If the issue was recently created (created and updated within a small window),
        // treat it as IssueCreated.
        let is_recently_created = created_at
            .map(|c| (updated_at - c).num_minutes().abs() < 5)
            .unwrap_or(false);

        if is_recently_created {
            let mut activity = Activity::new(
                Source::Linear,
                identifier.clone(),
                ActivityKind::IssueCreated,
                title.clone(),
                url.clone(),
                updated_at,
            );
            activity.project = team_name.clone();
            activity.metadata = build_metadata(issue);
            activities.push(activity);
            primary_emitted = true;
        }
    }

    // Process state transitions from history.
    if let Some(history_nodes) = issue["history"]["nodes"].as_array() {
        for entry in history_nodes {
            let from_type = entry["fromState"]["type"].as_str().unwrap_or_default();
            let to_type = entry["toState"]["type"].as_str().unwrap_or_default();

            // Skip if no actual state change.
            if to_type.is_empty() || from_type == to_type {
                continue;
            }

            let history_id = entry["id"].as_str().unwrap_or_default();
            let transition_at: DateTime<Utc> = entry["createdAt"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(updated_at);

            let kind = if to_type == "completed" {
                // Skip if we already emitted a completed activity for this issue.
                if primary_emitted
                    && activities
                        .iter()
                        .any(|a| a.kind == ActivityKind::IssueCompleted)
                {
                    continue;
                }
                ActivityKind::IssueCompleted
            } else {
                ActivityKind::IssueUpdated
            };

            let to_name = entry["toState"]["name"].as_str().unwrap_or(to_type);
            let transition_title = format!("{title} → {to_name}");
            let source_id = format!("{identifier}#{history_id}");

            let mut activity = Activity::new(
                Source::Linear,
                source_id,
                kind,
                transition_title,
                url.clone(),
                transition_at,
            );
            activity.project = team_name.clone();
            activity.metadata = serde_json::json!({
                "from_state": from_type,
                "to_state": to_type,
                "issue_identifier": identifier,
            });
            activities.push(activity);
        }
    }

    // If nothing was emitted yet, emit a generic update for the issue.
    if activities.is_empty() && !primary_emitted {
        let mut activity = Activity::new(
            Source::Linear,
            identifier.clone(),
            ActivityKind::IssueUpdated,
            title,
            url,
            updated_at,
        );
        activity.project = team_name;
        activity.metadata = build_metadata(issue);
        activities.push(activity);
    }

    activities
}

fn build_metadata(issue: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "priority": issue["priority"],
        "priority_label": issue["priorityLabel"],
        "state": issue["state"]["name"],
        "state_type": issue["state"]["type"],
    })
}

// ---------------------------------------------------------------------------
// Open tickets (all assigned, non-completed/cancelled)
// ---------------------------------------------------------------------------

const OPEN_ISSUES_QUERY: &str = r#"
query($after: String) {
  viewer {
    assignedIssues(
      first: 50
      after: $after
      filter: {
        state: { type: { nin: ["completed", "canceled", "cancelled"] } }
      }
      orderBy: updatedAt
    ) {
      pageInfo { hasNextPage endCursor }
      nodes {
        id
        identifier
        title
        url
        state { name type }
        priority
        priorityLabel
        team { name }
        createdAt
        updatedAt
      }
    }
  }
}
"#;

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenTicket {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: String,
    pub state_type: String,
    pub priority: i64,
    pub priority_label: String,
    pub team: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Fetch all open issues assigned to the viewer, sorted by priority.
pub async fn fetch_open_tickets() -> Result<Vec<OpenTicket>, String> {
    let token = AuthManager::get_token(&Source::Linear)
        .ok()
        .flatten()
        .ok_or_else(|| "Linear not connected".to_string())?;

    let client = reqwest::Client::new();
    let mut tickets: Vec<OpenTicket> = Vec::new();
    let mut after_cursor: Option<String> = None;

    for _ in 0..5 {
        let mut variables = serde_json::Map::new();
        if let Some(ref cursor) = after_cursor {
            variables.insert("after".to_string(), serde_json::Value::String(cursor.clone()));
        }

        let body = serde_json::json!({
            "query": OPEN_ISSUES_QUERY,
            "variables": variables,
        });

        let response = client
            .post(LINEAR_API_URL)
            .header("Authorization", &token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Linear request failed: {e}"))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Linear returned error: {text}"));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Linear response: {e}"))?;

        let assigned = &json["data"]["viewer"]["assignedIssues"];
        let nodes = assigned["nodes"]
            .as_array()
            .ok_or_else(|| "missing nodes in Linear response".to_string())?;

        if nodes.is_empty() {
            break;
        }

        for issue in nodes {
            tickets.push(OpenTicket {
                identifier: issue["identifier"].as_str().unwrap_or_default().to_string(),
                title: issue["title"].as_str().unwrap_or("Untitled").to_string(),
                url: issue["url"].as_str().unwrap_or_default().to_string(),
                state: issue["state"]["name"].as_str().unwrap_or_default().to_string(),
                state_type: issue["state"]["type"].as_str().unwrap_or_default().to_string(),
                priority: issue["priority"].as_i64().unwrap_or(4),
                priority_label: issue["priorityLabel"].as_str().unwrap_or("No priority").to_string(),
                team: issue["team"]["name"].as_str().unwrap_or_default().to_string(),
                created_at: issue["createdAt"].as_str().unwrap_or_default().to_string(),
                updated_at: issue["updatedAt"].as_str().unwrap_or_default().to_string(),
            });
        }

        let page_info = &assigned["pageInfo"];
        if page_info["hasNextPage"].as_bool() == Some(true) {
            after_cursor = page_info["endCursor"].as_str().map(|s| s.to_string());
        } else {
            break;
        }
    }

    // Sort by priority (1=urgent, 2=high, 3=medium, 4=low, 0=none)
    tickets.sort_by(|a, b| {
        let pa = if a.priority == 0 { 5 } else { a.priority };
        let pb = if b.priority == 0 { 5 } else { b.priority };
        pa.cmp(&pb)
    });

    Ok(tickets)
}
