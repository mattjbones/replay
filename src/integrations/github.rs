use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, ActivityKind, Source};

use super::{Integration, IntegrationError};

// ---------------------------------------------------------------------------
// Shared helpers — eliminate duplication across GitHub API call sites
// ---------------------------------------------------------------------------

/// Build an authenticated `reqwest::Client` with GitHub API headers and resolve the username.
/// Returns `(client, username)` or an error message.
pub fn github_authenticated_client(config: &AppConfig) -> Result<(reqwest::Client, String), String> {
    let token = AuthManager::get_github_token()
        .ok()
        .flatten()
        .ok_or_else(|| "GitHub not connected".to_string())?;

    if token.is_empty() {
        return Err("GitHub token is empty".to_string());
    }

    let username = resolve_github_username(config)
        .ok_or_else(|| "No GitHub username configured and `gh api user` failed".to_string())?;

    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| format!("Invalid token for header: {e}"))?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github.v3+json"),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("recap/0.1"));

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build client: {e}"))?;

    Ok((client, username))
}

/// Resolve the GitHub username: config value → `gh api user` CLI fallback → None.
fn resolve_github_username(config: &AppConfig) -> Option<String> {
    if let Some(ref u) = config.github.username {
        if !u.is_empty() {
            return Some(u.clone());
        }
    }
    // Fall back to gh CLI (check common paths for bundled .app)
    let gh_path = crate::auth::find_gh_binary()?;
    std::process::Command::new(&gh_path)
        .args(["api", "user", "--jq", ".login"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        })
}

/// Format a `since` timestamp for the GitHub Search API, falling back to `days` ago
/// when no cursor is available (first sync).
fn since_or_default(since: Option<DateTime<Utc>>, days: i64) -> String {
    since
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

/// Extract `"owner/repo"` from a GitHub API `repository_url`
/// (e.g. `"https://api.github.com/repos/acme/widgets"` → `"acme/widgets"`).
fn repo_name_from_url(url: &str) -> String {
    let parts: Vec<&str> = url.rsplit('/').take(2).collect();
    if parts.len() == 2 {
        format!("{}/{}", parts[1], parts[0])
    } else {
        url.to_string()
    }
}

// ---------------------------------------------------------------------------
// Rate-limit handling helpers
// ---------------------------------------------------------------------------

/// Threshold below which we log a warning about remaining rate-limit quota.
const RATE_LIMIT_WARN_THRESHOLD: u64 = 5;

/// Maximum number of retries when we hit a rate limit (403).
const RATE_LIMIT_MAX_RETRIES: u32 = 2;

/// Extract rate-limit metadata from response headers.
struct RateLimitInfo {
    remaining: Option<u64>,
    reset_timestamp: Option<i64>,
}

impl RateLimitInfo {
    fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let reset_timestamp = headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok());

        Self {
            remaining,
            reset_timestamp,
        }
    }

    /// Log a warning if remaining requests are low but not zero.
    fn warn_if_low(&self, context: &str) {
        if let Some(remaining) = self.remaining {
            if remaining > 0 && remaining <= RATE_LIMIT_WARN_THRESHOLD {
                tracing::warn!(
                    "github: {context} — rate limit nearly exhausted ({remaining} requests remaining)"
                );
            }
        }
    }

    /// Compute how many seconds to wait before retrying, based on `X-RateLimit-Reset`
    /// or `Retry-After` header (the latter takes priority if present).
    fn retry_after_secs(&self, headers: &reqwest::header::HeaderMap) -> u64 {
        // Prefer Retry-After header (seconds) if present
        if let Some(secs) = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            return secs.min(120); // cap at 2 minutes
        }

        // Fall back to X-RateLimit-Reset (unix timestamp)
        if let Some(reset) = self.reset_timestamp {
            let now = Utc::now().timestamp();
            if reset > now {
                return ((reset - now) as u64).min(120);
            }
        }

        // Default: wait 60 seconds
        60
    }
}

/// Send a GET request with rate-limit awareness: logs warnings when quota is low,
/// and retries with backoff on 403 rate-limit responses.
///
/// Returns the successful `reqwest::Response` or an error.
async fn rate_limited_get(
    client: &reqwest::Client,
    url: &str,
    context: &str,
) -> Result<reqwest::Response, IntegrationError> {
    let mut retries = 0u32;

    loop {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| IntegrationError::Network(e.to_string()))?;

        let status = response.status();
        let rate_info = RateLimitInfo::from_headers(response.headers());

        // Check for rate-limit (403 with remaining == 0, or 429)
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if let Some(0) = rate_info.remaining {
                let wait = rate_info.retry_after_secs(response.headers());

                if retries < RATE_LIMIT_MAX_RETRIES {
                    retries += 1;
                    tracing::warn!(
                        "github: {context} — rate limited (attempt {retries}/{RATE_LIMIT_MAX_RETRIES}), \
                         waiting {wait}s before retry"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }

                tracing::error!(
                    "github: {context} — rate limited, exhausted {RATE_LIMIT_MAX_RETRIES} retries"
                );
                return Err(IntegrationError::RateLimit {
                    retry_after_secs: wait,
                });
            }

            // 429 without remaining==0 header — still a rate limit
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = rate_info.retry_after_secs(response.headers());
                if retries < RATE_LIMIT_MAX_RETRIES {
                    retries += 1;
                    tracing::warn!(
                        "github: {context} — 429 rate limited (attempt {retries}/{RATE_LIMIT_MAX_RETRIES}), \
                         waiting {wait}s before retry"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    continue;
                }
                return Err(IntegrationError::RateLimit {
                    retry_after_secs: wait,
                });
            }
        }

        // Not rate-limited — log a warning if quota is running low
        rate_info.warn_if_low(context);

        return Ok(response);
    }
}

/// Convenience wrapper that returns a simpler `Result<reqwest::Response, String>` for
/// the standalone functions (`fetch_open_prs`, `fetch_github_issues`) that don't use
/// `IntegrationError`.
async fn rate_limited_get_simple(
    client: &reqwest::Client,
    url: &str,
    context: &str,
) -> Result<reqwest::Response, String> {
    rate_limited_get(client, url, context)
        .await
        .map_err(|e| format!("{e}"))
}
pub struct GitHubIntegration {
    client: reqwest::Client,
    #[allow(dead_code)]
    token: String,
    username: String,
}

impl GitHubIntegration {
    pub fn new(config: AppConfig) -> Option<Self> {
        let token = AuthManager::get_github_token()
            .ok()
            .flatten()
            .unwrap_or_default();

        if token.is_empty() {
            tracing::warn!("github: no token available, skipping");
            return None;
        }

        let (client, username) = match github_authenticated_client(&config) {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("github: {e}, skipping");
                return None;
            }
        };

        tracing::info!("github: using username '{username}'");

        Some(Self {
            client,
            token,
            username,
        })
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

            let context = format!("events page {page}");
            let response = rate_limited_get(&self.client, &url, &context).await?;

            let status = response.status();

            if status == reqwest::StatusCode::UNAUTHORIZED {
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

        // Supplement with authored PRs (catches merges by Graphite, bots, etc.)
        let pr_activities = self.fetch_authored_prs(since_dt).await.unwrap_or_else(|e| {
            tracing::warn!("github: failed to fetch authored PRs: {e}");
            Vec::new()
        });

        // Supplement with issues the user is involved in (Search API)
        let issue_activities = self.fetch_involved_issues(since_dt).await.unwrap_or_else(|e| {
            tracing::warn!("github: failed to fetch involved issues: {e}");
            Vec::new()
        });

        // Deduplicate: only add activities whose source_id we haven't seen
        let existing_ids: std::collections::HashSet<String> = all_activities
            .iter()
            .map(|a| a.source_id.clone())
            .collect();
        for activity in pr_activities.into_iter().chain(issue_activities) {
            if !existing_ids.contains(&activity.source_id) {
                if latest_cursor.is_none() || Some(activity.occurred_at.to_rfc3339()) > latest_cursor {
                    latest_cursor = Some(activity.occurred_at.to_rfc3339());
                }
                all_activities.push(activity);
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

impl GitHubIntegration {
    /// Fetch PRs authored by the user via the Search API.
    /// This catches PRs merged by Graphite or other bots that don't appear in the events feed.
    async fn fetch_authored_prs(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Activity>, IntegrationError> {
        let since_str = since_or_default(since, 90);

        let query = format!(
            "author:{} type:pr updated:>{}",
            self.username, since_str
        );
        let url = format!(
            "https://api.github.com/search/issues?q={}&sort=updated&order=desc&per_page=100",
            urlencoding::encode(&query)
        );

        let response = rate_limited_get(&self.client, &url, "search authored PRs").await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(IntegrationError::Network(format!(
                "GitHub search returned {body}"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| IntegrationError::Parse(e.to_string()))?;

        let empty_vec = Vec::new();
        let items = json["items"]
            .as_array()
            .unwrap_or(&empty_vec);

        // First pass: parse items and identify which closed PRs need Graphite checks
        struct PrCandidate {
            number: u64,
            title: String,
            html_url: String,
            repo_name: String,
            updated_str: String,
            merged_at_present: bool,
            state: String,
        }

        let mut candidates: Vec<PrCandidate> = Vec::new();
        for item in items {
            let number = item["number"].as_u64().unwrap_or(0);
            let title = item["title"].as_str().unwrap_or("Untitled PR").to_string();
            let html_url = item["html_url"].as_str().unwrap_or_default().to_string();
            let state = item["state"].as_str().unwrap_or_default().to_string();
            let repo_url = item["repository_url"].as_str().unwrap_or_default();
            let repo_name = repo_name_from_url(repo_url);
            let updated_str = item["updated_at"].as_str().unwrap_or_default().to_string();
            let merged_at_present = item["pull_request"]
                .get("merged_at")
                .and_then(|v| v.as_str())
                .is_some();

            candidates.push(PrCandidate { number, title, html_url, repo_name, updated_str, merged_at_present, state });
        }

        // Second pass: check closed PRs for Graphite merges in parallel (cap at 20)
        let closed_prs: Vec<&PrCandidate> = candidates.iter()
            .filter(|c| c.state == "closed" && !c.merged_at_present)
            .take(20)
            .collect();

        let mut graphite_merged: std::collections::HashSet<u64> = std::collections::HashSet::new();

        if !closed_prs.is_empty() {
            tracing::info!("github: checking {} closed PRs for Graphite merges in parallel", closed_prs.len());
            let mut join_set = tokio::task::JoinSet::new();
            for c in &closed_prs {
                let client = self.client.clone();
                let repo = c.repo_name.clone();
                let number = c.number;
                join_set.spawn(async move {
                    let merged = check_graphite_merge_static(&client, &repo, number).await;
                    (number, merged)
                });
            }
            while let Some(result) = join_set.join_next().await {
                if let Ok((number, true)) = result {
                    tracing::info!("github: PR #{number} detected as Graphite merge");
                    graphite_merged.insert(number);
                }
            }
        }

        // Third pass: build activities
        let mut activities = Vec::new();
        for c in &candidates {
            let kind = if c.merged_at_present {
                ActivityKind::PrMerged
            } else if c.state == "open" {
                ActivityKind::PrOpened
            } else if c.state == "closed" && graphite_merged.contains(&c.number) {
                ActivityKind::PrMerged
            } else {
                continue;
            };

            let occurred_at: DateTime<Utc> = c.updated_str
                .parse()
                .unwrap_or_else(|_| Utc::now());

            let source_id = format!("pr:{}:{}", c.repo_name, c.number);
            let (cc_type, cc_scope) = parse_conventional_commit(&c.title);

            let mut activity = Activity::new(
                Source::GitHub,
                source_id,
                kind,
                c.title.clone(),
                c.html_url.clone(),
                occurred_at,
            );
            activity.project = Some(c.repo_name.clone());
            activity.metadata = serde_json::json!({
                "pr_number": c.number,
                "cc_type": cc_type,
                "cc_scope": cc_scope,
            });

            activities.push(activity);
        }

        tracing::info!("github: found {} authored PRs via search", activities.len());
        Ok(activities)
    }

    /// Fetch issues the user is involved in (assigned, authored, mentioned) via the Search API.
    /// Captures issue lifecycle events that the Events API misses.
    async fn fetch_involved_issues(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Activity>, IntegrationError> {
        let since_str = since_or_default(since, 90);

        let query = format!(
            "involves:{} type:issue updated:>{}",
            self.username, since_str
        );
        let encoded_query = urlencoding::encode(&query);

        let mut all_items: Vec<serde_json::Value> = Vec::new();

        // Paginate up to 3 pages (300 issues max)
        for page in 1..=3 {
            let url = format!(
                "https://api.github.com/search/issues?q={}&sort=updated&order=desc&per_page=100&page={page}",
                encoded_query
            );

            let context = format!("search involved issues page {page}");
            let response = rate_limited_get(&self.client, &url, &context).await?;

            if !response.status().is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(IntegrationError::Network(format!(
                    "GitHub issue search returned {body}"
                )));
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| IntegrationError::Parse(e.to_string()))?;

            let empty_vec = Vec::new();
            let items = json["items"]
                .as_array()
                .unwrap_or(&empty_vec);

            if items.is_empty() {
                break;
            }
            all_items.extend(items.iter().cloned());

            // If we got fewer than 100, no more pages
            if items.len() < 100 {
                break;
            }
        }

        let mut activities = Vec::new();
        for item in &all_items {
            // Skip pull requests (search API returns PRs as issues too)
            if item.get("pull_request").is_some() {
                continue;
            }

            let number = item["number"].as_u64().unwrap_or(0);
            let title = item["title"].as_str().unwrap_or("Untitled Issue").to_string();
            let html_url = item["html_url"].as_str().unwrap_or_default().to_string();
            let state = item["state"].as_str().unwrap_or_default().to_string();
            let repo_url = item["repository_url"].as_str().unwrap_or_default();
            let repo_name = repo_name_from_url(repo_url);

            // Use closed_at for closed issues, created_at for open ones
            let (kind, timestamp_str) = if state == "closed" {
                let closed_at = item["closed_at"].as_str().unwrap_or_default();
                (ActivityKind::IssueOpened, if closed_at.is_empty() {
                    item["updated_at"].as_str().unwrap_or_default()
                } else {
                    closed_at
                })
            } else {
                (ActivityKind::IssueOpened, item["created_at"].as_str().unwrap_or_default())
            };

            let occurred_at: DateTime<Utc> = timestamp_str
                .parse()
                .unwrap_or_else(|_| Utc::now());

            let source_id = format!("issue:{}:{}", repo_name, number);

            let mut activity = Activity::new(
                Source::GitHub,
                source_id,
                kind,
                title,
                html_url,
                occurred_at,
            );
            activity.project = Some(repo_name);
            activity.metadata = serde_json::json!({
                "issue_number": number,
                "state": state,
            });

            activities.push(activity);
        }

        tracing::info!("github: found {} involved issues via search", activities.len());
        Ok(activities)
    }

    /// Check if a closed PR was actually merged via Graphite's merge queue.
    /// Graphite closes the PR (without setting merged=true) and deletes the head branch.
    /// If the head branch is gone (404), it was merged.
    // Instance method kept for API compatibility, delegates to static
    #[allow(dead_code)]
    async fn check_graphite_merge(&self, repo: &str, pr_number: u64) -> bool {
        check_graphite_merge_static(&self.client, repo, pr_number).await
    }
}

/// Check if a closed PR was merged via Graphite's merge queue.
/// Static function so it can be spawned into a JoinSet for parallel execution.
async fn check_graphite_merge_static(client: &reqwest::Client, repo: &str, pr_number: u64) -> bool {
    tracing::debug!("github: checking if PR #{pr_number} in {repo} was Graphite-merged");
    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}");
    let context = format!("check graphite merge PR #{pr_number}");
    let resp = match rate_limited_get(client, &url, &context).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("github: failed to fetch PR #{pr_number}: {e}");
            return false;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!("github: PR #{pr_number} returned {}", resp.status());
        return false;
    }
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return false,
    };

    // If the Pulls API says merged, trust it
    if json.get("merged").and_then(|v| v.as_bool()) == Some(true) {
        tracing::info!("github: PR #{pr_number} — Pulls API says merged=true");
        return true;
    }

    // Check if head branch was deleted (Graphite deletes after merge)
    let head_ref = match json["head"]["ref"].as_str() {
        Some(r) => r,
        None => return false,
    };
    let branch_url = format!("https://api.github.com/repos/{repo}/git/ref/heads/{head_ref}");
    let branch_context = format!("check branch ref for PR #{pr_number}");
    let branch_deleted = match rate_limited_get(client, &branch_url, &branch_context).await {
        Ok(r) => r.status() == reqwest::StatusCode::NOT_FOUND,
        Err(_) => false,
    };
    tracing::info!(
        "github: PR #{pr_number} — merged=false, branch '{head_ref}' deleted={branch_deleted}",
    );
    branch_deleted
}

/// Parse a conventional commit prefix from a PR title.
/// Returns (type, scope) e.g. "feat(work): title" → ("feat", "work")
fn parse_conventional_commit(title: &str) -> (Option<String>, Option<String>) {
    // Match: type(scope): ... OR type: ...
    // Types: feat, fix, chore, perf, refactor, docs, test, ci, build, style, revert
    let title_trimmed = title.trim();
    let valid_types = [
        "feat", "fix", "chore", "perf", "refactor", "docs",
        "test", "ci", "build", "style", "revert",
    ];

    // Try type(scope): pattern
    if let Some(colon_pos) = title_trimmed.find(':') {
        let prefix = &title_trimmed[..colon_pos];
        if let Some(paren_start) = prefix.find('(') {
            let cc_type = &prefix[..paren_start];
            if valid_types.contains(&cc_type.to_lowercase().as_str()) {
                let scope = prefix[paren_start + 1..].trim_end_matches(')');
                return (
                    Some(cc_type.to_lowercase()),
                    if scope.is_empty() { None } else { Some(scope.to_string()) },
                );
            }
        } else {
            // Try type: pattern (no scope)
            let cc_type = prefix.trim();
            if valid_types.contains(&cc_type.to_lowercase().as_str()) {
                return (Some(cc_type.to_lowercase()), None);
            }
        }
    }

    (None, None)
}

// ---------------------------------------------------------------------------
// Event parsing helpers
// ---------------------------------------------------------------------------

fn parse_event(event: &GitHubEvent, occurred_at: DateTime<Utc>) -> Vec<Activity> {
    match event.event_type.as_str() {
        "PushEvent" => parse_push_event(event, occurred_at),
        // PullRequestEvent skipped — Search API (fetch_authored_prs) is the
        // single source of truth for PR status, including Graphite merges.
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

    // Extract the branch ref (e.g. "refs/heads/feat/my-feature" → "feat/my-feature").
    // Skip pushes to default branches — those are merge commits, not direct work.
    let full_ref = payload.get("ref").and_then(|r| r.as_str()).unwrap_or("");
    let branch = full_ref.strip_prefix("refs/heads/").unwrap_or(full_ref);
    if matches!(branch, "main" | "master" | "develop" | "dev") {
        return Vec::new();
    }

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
    let url = format!("https://github.com/{}/tree/{}", event.repo.name, branch);

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
    // Store count, SHAs, and branch for PR association
    let commit_shas: Vec<&str> = commits
        .iter()
        .filter_map(|c| c.get("sha").and_then(|s| s.as_str()))
        .collect();
    activity.metadata = serde_json::json!({
        "commit_count": n,
        "commit_shas": commit_shas,
        "branch": branch,
    });

    vec![activity]
}

// parse_pull_request_event removed — Search API handles all PR status

fn parse_pull_request_review_event(
    event: &GitHubEvent,
    occurred_at: DateTime<Utc>,
) -> Option<Activity> {
    let payload = &event.payload;
    let pr = payload.get("pull_request")?;

    let pr_number = payload
        .get("number")
        .and_then(|n| n.as_u64())
        .or_else(|| pr.get("number").and_then(|n| n.as_u64()));

    let pr_title = pr
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            match pr_number {
                Some(n) => format!("PR #{n}", ),
                None => "a PR".to_string(),
            }
        });
    let title = format!("Reviewed: {pr_title} in {}", event.repo.name);
    let url = pr
        .get("html_url")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            match pr_number {
                Some(n) => format!("https://github.com/{}/pull/{n}", event.repo.name),
                None => format!("https://github.com/{}", event.repo.name),
            }
        });

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

    let kind = match action {
        "opened" => ActivityKind::IssueOpened,
        "closed" => ActivityKind::IssueOpened, // still IssueOpened kind — state tracked in metadata
        _ => return None,
    };

    let issue = payload.get("issue")?;
    let number = issue.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
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

    // Use a stable source_id so Search API and Events API deduplicate
    let source_id = format!("issue:{}:{}", event.repo.name, number);

    let mut activity = Activity::new(
        Source::GitHub,
        source_id,
        kind,
        title,
        url,
        occurred_at,
    );
    activity.project = Some(event.repo.name.clone());
    activity.metadata = serde_json::json!({
        "issue_number": number,
        "state": if action == "closed" { "closed" } else { "open" },
    });

    Some(activity)
}

// ---------------------------------------------------------------------------
// Open / Draft PRs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubLabel {
    pub name: String,
    pub color: String, // hex color without '#', e.g. "d73a4a"
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenPr {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub state: String,       // "open" or "draft"
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<GitHubLabel>,
    pub review_status: String, // "approved", "changes_requested", "review_required", ""
    pub additions: u64,
    pub deletions: u64,
    pub cc_type: Option<String>,
    pub cc_scope: Option<String>,
}

/// Fetch all open and draft PRs authored by the configured user.
pub async fn fetch_open_prs(config: &crate::config::AppConfig) -> Result<Vec<OpenPr>, String> {
    let (client, username_owned) = github_authenticated_client(config)?;
    let username = username_owned.as_str();

    let query = format!("author:{username} type:pr state:open");
    let url = format!(
        "https://api.github.com/search/issues?q={}&sort=updated&order=desc&per_page=100",
        urlencoding::encode(&query)
    );

    let response = rate_limited_get_simple(&client, &url, "search open PRs").await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("GitHub search error: {text}"));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    let empty_vec = Vec::new();
    let items = json["items"].as_array().unwrap_or(&empty_vec);

    let mut prs: Vec<OpenPr> = Vec::new();

    for item in items {
        let number = item["number"].as_u64().unwrap_or(0);
        let title = item["title"].as_str().unwrap_or("Untitled").to_string();
        let html_url = item["html_url"].as_str().unwrap_or_default().to_string();
        let repo_url = item["repository_url"].as_str().unwrap_or_default();
        let repo = repo_name_from_url(repo_url);
        let created_at = item["created_at"].as_str().unwrap_or_default().to_string();
        let updated_at = item["updated_at"].as_str().unwrap_or_default().to_string();
        let is_draft = item["draft"].as_bool().unwrap_or(false);

        let labels: Vec<GitHubLabel> = item["labels"]
            .as_array()
            .unwrap_or(&empty_vec)
            .iter()
            .filter_map(|l| {
                let name = l["name"].as_str()?.to_string();
                let color = l["color"].as_str().unwrap_or("ededed").to_string();
                Some(GitHubLabel { name, color })
            })
            .collect();

        let (cc_type, cc_scope) = parse_conventional_commit(&title);

        prs.push(OpenPr {
            number,
            title,
            url: html_url,
            repo,
            state: if is_draft { "draft".to_string() } else { "open".to_string() },
            created_at,
            updated_at,
            labels,
            review_status: String::new(), // would need a separate API call per PR
            additions: 0,
            deletions: 0,
            cc_type,
            cc_scope,
        });
    }

    Ok(prs)
}

// ---------------------------------------------------------------------------
// GitHub Issues (assigned to user)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<GitHubLabel>,
    pub comments: u64,
}

/// Fetch open issues assigned to the configured user.
pub async fn fetch_github_issues(config: &crate::config::AppConfig) -> Result<Vec<GitHubIssue>, String> {
    let (client, username_owned) = github_authenticated_client(config)?;
    let username = username_owned.as_str();

    let query = format!("assignee:{username} type:issue state:open");
    let url = format!(
        "https://api.github.com/search/issues?q={}&sort=updated&order=desc&per_page=100",
        urlencoding::encode(&query)
    );

    let response = rate_limited_get_simple(&client, &url, "search assigned issues").await?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("GitHub search error: {text}"));
    }

    let json: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    let empty_vec = Vec::new();
    let items = json["items"].as_array().unwrap_or(&empty_vec);

    let mut issues: Vec<GitHubIssue> = Vec::new();

    for item in items {
        // Skip pull requests (the search API returns PRs as issues too)
        if item.get("pull_request").is_some() {
            continue;
        }

        let number = item["number"].as_u64().unwrap_or(0);
        let title = item["title"].as_str().unwrap_or("Untitled").to_string();
        let html_url = item["html_url"].as_str().unwrap_or_default().to_string();
        let repo_url = item["repository_url"].as_str().unwrap_or_default();
        let repo = repo_name_from_url(repo_url);
        let created_at = item["created_at"].as_str().unwrap_or_default().to_string();
        let updated_at = item["updated_at"].as_str().unwrap_or_default().to_string();
        let comments = item["comments"].as_u64().unwrap_or(0);

        let labels: Vec<GitHubLabel> = item["labels"]
            .as_array()
            .unwrap_or(&empty_vec)
            .iter()
            .filter_map(|l| {
                let name = l["name"].as_str()?.to_string();
                let color = l["color"].as_str().unwrap_or("ededed").to_string();
                Some(GitHubLabel { name, color })
            })
            .collect();

        issues.push(GitHubIssue {
            number,
            title,
            url: html_url,
            repo,
            state: "open".to_string(),
            created_at,
            updated_at,
            labels,
            comments,
        });
    }

    Ok(issues)
}
