use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, ActivityKind, Source};

use super::{Integration, IntegrationError};

pub struct GitHubIntegration {
    client: reqwest::Client,
    #[allow(dead_code)]
    token: String,
    username: String,
}

impl GitHubIntegration {
    pub fn new(config: AppConfig) -> Self {
        let token = AuthManager::get_github_token()
            .ok()
            .flatten()
            .unwrap_or_default();

        let username = config.github.username.unwrap_or_default();

        use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("invalid token for header"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github.v3+json"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("recap/0.1"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            token,
            username,
        }
    }
}

// ---------------------------------------------------------------------------
// GitHub Events API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GitHubEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: String,
    repo: Repo,
    created_at: String,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct Repo {
    name: String,
}

// ---------------------------------------------------------------------------
// Integration implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Integration for GitHubIntegration {
    fn source(&self) -> Source {
        Source::GitHub
    }

    async fn fetch_activities(
        &self,
        since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        let since_dt: Option<DateTime<Utc>> = since.and_then(|s| s.parse().ok());

        let mut all_activities: Vec<Activity> = Vec::new();
        let mut latest_cursor: Option<String> = None;

        // Fetch up to 3 pages (GitHub caps public events at 10 pages).
        for page in 1..=3 {
            let url = format!(
                "https://api.github.com/users/{}/events?per_page=100&page={page}",
                self.username
            );

            let response = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| IntegrationError::Network(e.to_string()))?;

            let status = response.status();

            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                // Check for rate limit vs auth error.
                if let Some(remaining) = response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                {
                    if remaining == 0 {
                        let retry_after = response
                            .headers()
                            .get("x-ratelimit-reset")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<i64>().ok())
                            .map(|reset| {
                                let now = Utc::now().timestamp();
                                if reset > now {
                                    (reset - now) as u64
                                } else {
                                    60
                                }
                            })
                            .unwrap_or(60);
                        return Err(IntegrationError::RateLimit {
                            retry_after_secs: retry_after,
                        });
                    }
                }
                let body = response.text().await.unwrap_or_default();
                return Err(IntegrationError::Auth(format!(
                    "GitHub returned {status}: {body}"
                )));
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(IntegrationError::Network(format!(
                    "GitHub returned {status}: {body}"
                )));
            }

            let events: Vec<GitHubEvent> = response
                .json()
                .await
                .map_err(|e| IntegrationError::Parse(e.to_string()))?;

            if events.is_empty() {
                break;
            }

            let mut reached_old = false;
            for event in &events {
                let event_dt: DateTime<Utc> = event
                    .created_at
                    .parse()
                    .map_err(|e: chrono::ParseError| IntegrationError::Parse(e.to_string()))?;

                // Skip events older than our cursor.
                if let Some(ref since) = since_dt {
                    if event_dt <= *since {
                        reached_old = true;
                        break;
                    }
                }

                // Track the most recent event timestamp for the new cursor.
                if latest_cursor.is_none() {
                    latest_cursor = Some(event.created_at.clone());
                }

                let activities = parse_event(event, event_dt);
                all_activities.extend(activities);
            }

            if reached_old {
                break;
            }
        }

        let cursor = latest_cursor.unwrap_or_else(|| {
            since
                .map(|s| s.to_string())
                .unwrap_or_else(|| Utc::now().to_rfc3339())
        });

        Ok((all_activities, cursor))
    }
}

// ---------------------------------------------------------------------------
// Event parsing helpers
// ---------------------------------------------------------------------------

fn parse_event(event: &GitHubEvent, occurred_at: DateTime<Utc>) -> Vec<Activity> {
    match event.event_type.as_str() {
        "PushEvent" => parse_push_event(event, occurred_at),
        "PullRequestEvent" => parse_pull_request_event(event, occurred_at)
            .into_iter()
            .collect(),
        "PullRequestReviewEvent" => parse_pull_request_review_event(event, occurred_at)
            .into_iter()
            .collect(),
        "IssuesEvent" => parse_issues_event(event, occurred_at)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_push_event(event: &GitHubEvent, occurred_at: DateTime<Utc>) -> Vec<Activity> {
    let payload = &event.payload;
    let commits = payload
        .get("commits")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let n = commits.len();
    if n == 0 {
        return Vec::new();
    }

    let title = format!(
        "Pushed {n} commit{} to {}",
        if n == 1 { "" } else { "s" },
        event.repo.name
    );
    let url = format!("https://github.com/{}", event.repo.name);

    let commit_messages: Vec<String> = commits
        .iter()
        .filter_map(|c| {
            c.get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let description = commit_messages.join("\n");

    let mut activity = Activity::new(
        Source::GitHub,
        event.id.clone(),
        ActivityKind::CommitPushed,
        title,
        url,
        occurred_at,
    );
    activity.description = Some(description);
    activity.project = Some(event.repo.name.clone());
    activity.metadata = serde_json::json!({
        "commit_count": n,
        "commits": commits,
    });

    vec![activity]
}

fn parse_pull_request_event(
    event: &GitHubEvent,
    occurred_at: DateTime<Utc>,
) -> Option<Activity> {
    let payload = &event.payload;
    let action = payload
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("");
    let pr = payload.get("pull_request")?;

    let title = pr
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled PR")
        .to_string();
    let url = pr
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    let kind = match action {
        "opened" | "reopened" => ActivityKind::PrOpened,
        "closed" => {
            let merged = pr.get("merged").and_then(|m| m.as_bool()).unwrap_or(false);
            if merged {
                ActivityKind::PrMerged
            } else {
                // Closed without merge -- not tracked.
                return None;
            }
        }
        _ => return None,
    };

    let mut activity = Activity::new(
        Source::GitHub,
        event.id.clone(),
        kind,
        title,
        url,
        occurred_at,
    );
    activity.project = Some(event.repo.name.clone());

    Some(activity)
}

fn parse_pull_request_review_event(
    event: &GitHubEvent,
    occurred_at: DateTime<Utc>,
) -> Option<Activity> {
    let payload = &event.payload;
    let pr = payload.get("pull_request")?;

    let pr_title = pr
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled PR");
    let title = format!("Reviewed: {pr_title}");
    let url = pr
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    let review_state = payload
        .get("review")
        .and_then(|r| r.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();

    let mut activity = Activity::new(
        Source::GitHub,
        event.id.clone(),
        ActivityKind::PrReviewed,
        title,
        url,
        occurred_at,
    );
    activity.project = Some(event.repo.name.clone());
    activity.metadata = serde_json::json!({ "review_state": review_state });

    Some(activity)
}

fn parse_issues_event(
    event: &GitHubEvent,
    occurred_at: DateTime<Utc>,
) -> Option<Activity> {
    let payload = &event.payload;
    let action = payload
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("");

    if action != "opened" {
        return None;
    }

    let issue = payload.get("issue")?;
    let title = issue
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled Issue")
        .to_string();
    let url = issue
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    let mut activity = Activity::new(
        Source::GitHub,
        event.id.clone(),
        ActivityKind::IssueOpened,
        title,
        url,
        occurred_at,
    );
    activity.project = Some(event.repo.name.clone());

    Some(activity)
}
