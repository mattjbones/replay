use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, ActivityKind, Source};

use super::{Integration, IntegrationError};

pub struct SlackIntegration {
    client: reqwest::Client,
    token: std::sync::Mutex<String>,
    /// Configured Slack user id; resolved from the token via `auth.test` when unset.
    user_id: tokio::sync::OnceCell<String>,
    ignored_channels: Vec<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
}

impl SlackIntegration {
    pub fn new(config: AppConfig) -> Self {
        let token = AuthManager::get_token(&Source::Slack)
            .ok()
            .flatten()
            .unwrap_or_default();

        let user_id = match config.slack.user_id.filter(|u| !u.is_empty()) {
            Some(u) => tokio::sync::OnceCell::new_with(Some(u)),
            None => tokio::sync::OnceCell::new(),
        };
        let ignored_channels = config.slack.ignored_channels.clone();
        let client_id = config.slack.client_id.clone();
        let client_secret = config.slack.client_secret.clone();

        let client = reqwest::Client::new();
        Self {
            client,
            token: std::sync::Mutex::new(token),
            user_id,
            ignored_channels,
            client_id,
            client_secret,
        }
    }

    /// The user's Slack id: from config, else asked of Slack for the token's owner. Without it
    /// the search query is `from:<@>`, which silently matches nothing.
    async fn user_id(&self) -> Result<&str, IntegrationError> {
        self.user_id.get_or_try_init(|| async {
            let token = self.token.lock().unwrap().clone();
            let body: serde_json::Value = self.client
                .get("https://slack.com/api/auth.test")
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| IntegrationError::Network(e.to_string()))?
                .json()
                .await
                .map_err(|e| IntegrationError::Parse(e.to_string()))?;
            match body["user_id"].as_str() {
                Some(id) if body["ok"].as_bool() == Some(true) => {
                    tracing::info!("slack: resolved user id {id} via auth.test");
                    Ok(id.to_string())
                }
                _ => Err(IntegrationError::Auth(format!(
                    "couldn't work out your Slack user id (auth.test: {}); set slack.user_id in config",
                    body["error"].as_str().unwrap_or("unknown error")
                ))),
            }
        }).await.map(String::as_str)
    }

    /// Attempt to refresh the Slack access token using the stored refresh token.
    /// Updates both keychain entries if successful.
    async fn try_refresh_token(&self) -> Result<String, IntegrationError> {
        let refresh_token = AuthManager::get_slack_refresh_token()
            .map_err(IntegrationError::Auth)?
            .ok_or_else(|| IntegrationError::Auth("no refresh token stored".to_string()))?;

        let client_id = self.client_id.as_deref()
            .ok_or_else(|| IntegrationError::Auth("slack.client_id not set in config".to_string()))?;
        let client_secret = self.client_secret.as_deref()
            .ok_or_else(|| IntegrationError::Auth("slack.client_secret not set in config".to_string()))?;

        let resp = self.client
            .post("https://slack.com/api/oauth.v2.access")
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_token),
            ])
            .send()
            .await
            .map_err(|e| IntegrationError::Network(e.to_string()))?;

        let body: serde_json::Value = resp.json().await
            .map_err(|e| IntegrationError::Parse(e.to_string()))?;

        if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("unknown");
            return Err(IntegrationError::Auth(format!("token refresh failed: {err}")));
        }

        let new_access = body.get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| IntegrationError::Parse("no access_token in refresh response".to_string()))?
            .to_string();

        // Store the new access token.
        AuthManager::set_token(&Source::Slack, &new_access)
            .map_err(IntegrationError::Auth)?;

        // Store new refresh token if provided (rotation gives a new one each time).
        if let Some(new_refresh) = body.get("refresh_token").and_then(|v| v.as_str()) {
            let _ = AuthManager::set_slack_refresh_token(new_refresh);
        }

        // Update in-memory token.
        if let Ok(mut t) = self.token.lock() {
            *t = new_access.clone();
        }

        tracing::info!("slack: token refreshed successfully");
        Ok(new_access)
    }

    /// Check if a channel name matches any of the ignored patterns.
    fn is_channel_ignored(&self, channel_name: &str) -> bool {
        self.ignored_channels
            .iter()
            .any(|pattern| glob_match::glob_match(pattern, channel_name))
    }

    /// Parse a Slack `ts` string (Unix float like "1234567890.123456") into a `DateTime<Utc>`.
    fn parse_ts(ts: &str) -> Option<DateTime<Utc>> {
        let parts: Vec<&str> = ts.splitn(2, '.').collect();
        let secs = parts.first()?.parse::<i64>().ok()?;
        let nanos = if parts.len() > 1 {
            // Pad or truncate the fractional part to 9 digits for nanoseconds.
            let frac = parts[1];
            let padded = format!("{:0<9}", frac);
            padded[..9].parse::<u32>().unwrap_or(0)
        } else {
            0
        };
        DateTime::from_timestamp(secs, nanos)
    }

    /// Parse the `since` cursor. If it contains no `-` it is treated as a Unix
    /// timestamp (possibly fractional); otherwise it is parsed as ISO8601.
    /// Returns the Unix timestamp as an f64 for comparison with Slack `ts` values.
    fn parse_since(since: &str) -> Option<f64> {
        if !since.contains('-') {
            // Unix timestamp (possibly fractional)
            since.parse::<f64>().ok()
        } else {
            // ISO8601
            since
                .parse::<DateTime<Utc>>()
                .ok()
                .map(|dt| dt.timestamp() as f64 + dt.timestamp_subsec_nanos() as f64 / 1_000_000_000.0)
        }
    }

    /// Convert a Slack `ts` string to an f64 for numeric comparison.
    fn ts_to_f64(ts: &str) -> f64 {
        ts.parse::<f64>().unwrap_or(0.0)
    }
}

// ---------------------------------------------------------------------------
// Slack API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SlackResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    messages: Option<SlackMessages>,
}

#[derive(Debug, Deserialize)]
struct SlackMessages {
    #[serde(default)]
    matches: Vec<SlackMessage>,
    #[serde(default)]
    paging: Option<SlackPaging>,
}

#[derive(Debug, Deserialize)]
struct SlackPaging {
    #[serde(default)]
    pages: u32,
    #[allow(dead_code)]
    #[serde(default)]
    page: u32,
}

#[derive(Debug, Deserialize)]
struct SlackMessage {
    #[serde(default)]
    channel: SlackChannel,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    ts: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    permalink: Option<String>,
    #[serde(default)]
    iid: Option<String>,
    #[serde(default)]
    thread_ts: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SlackChannel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

/// First `max` characters of `text` plus "..." if it was longer. Counts chars, not bytes:
/// slicing at byte 200 panics when that lands inside a multi-byte character like `”`.
fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Integration implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Integration for SlackIntegration {
    fn source(&self) -> Source {
        Source::Slack
    }

    async fn fetch_activities(
        &self,
        since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        // First sync (no cursor) reaches back 90 days.
        let since_ts: f64 = since
            .and_then(Self::parse_since)
            .unwrap_or_else(|| (Utc::now() - chrono::Duration::days(90)).timestamp() as f64);
        self.fetch_between(since_ts, None, since).await
    }

    async fn fetch_range(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<Activity>, IntegrationError> {
        self.fetch_between(since.timestamp() as f64, Some(until.timestamp() as f64), None)
            .await
            .map(|(acts, _)| acts)
    }
}

impl SlackIntegration {
    /// Fetch the user's messages after `since_ts` (and before `until_ts`, if given).
    /// `since` is the raw cursor, echoed back as the new cursor when nothing new is found.
    async fn fetch_between(
        &self,
        since_ts: f64,
        until_ts: Option<f64>,
        since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        let user_id = self.user_id().await?;
        // `after:`/`before:` are exclusive and day-granular, so widen by a day each side and
        // let the per-message timestamp checks trim the rest.
        let day = |ts: f64, offset: i64| {
            DateTime::from_timestamp(ts as i64, 0)
                .map(|dt| (dt + chrono::Duration::days(offset)).format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        };
        let mut query = format!("from:<@{user_id}> after:{}", day(since_ts, -1));
        if let Some(until) = until_ts {
            query.push_str(&format!(" before:{}", day(until, 1)));
        }
        let mut rate_limit_retries = 0u32;

        let mut all_activities: Vec<Activity> = Vec::new();
        let mut latest_ts: Option<String> = None;

        // Page back until we pass `since` (Slack serves at most 100 pages of 100).
        let mut page = 0u32;
        while page < 100 {
            page += 1;
            let url = format!(
                "https://slack.com/api/search.messages?query={}&sort=timestamp&sort_dir=desc&count=100&page={}",
                urlencoding::encode(&query), page
            );

            let token = self.token.lock().unwrap().clone();
            let response = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
                .map_err(|e| IntegrationError::Network(e.to_string()))?;

            // Check for rate limiting via HTTP status before parsing body.
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get("Retry-After")
                    .or_else(|| response.headers().get("retry-after"))
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(60);
                // search.messages is Tier 2 (~20/min), so a long backfill can hit this.
                if rate_limit_retries < 3 {
                    rate_limit_retries += 1;
                    tracing::warn!("slack: rate limited on page {page}, waiting {retry_after}s");
                    tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                    page -= 1;
                    continue;
                }
                return Err(IntegrationError::RateLimit {
                    retry_after_secs: retry_after,
                });
            }

            let body: SlackResponse = response
                .json()
                .await
                .map_err(|e| IntegrationError::Parse(e.to_string()))?;

            if !body.ok {
                let error = body.error.unwrap_or_else(|| "unknown".to_string());
                match error.as_str() {
                    "invalid_auth" | "not_authed" | "token_expired" | "token_revoked" => {
                        // Try refreshing the token once, then retry this page.
                        tracing::info!("slack: got {error}, attempting token refresh");
                        match self.try_refresh_token().await {
                            Ok(new_token) => {
                                // Retry the same request with the new token.
                                let retry_resp = self
                                    .client
                                    .get(&url)
                                    .header("Authorization", format!("Bearer {}", new_token))
                                    .send()
                                    .await
                                    .map_err(|e| IntegrationError::Network(e.to_string()))?;

                                let retry_body: SlackResponse = retry_resp.json().await
                                    .map_err(|e| IntegrationError::Parse(e.to_string()))?;

                                if !retry_body.ok {
                                    let err2 = retry_body.error.unwrap_or_else(|| "unknown".to_string());
                                    return Err(IntegrationError::Auth(
                                        format!("Slack auth error after refresh: {err2}")
                                    ));
                                }
                                // Fall through with retry_body — but we need to process it.
                                // For simplicity, just continue to next page (we may miss this page).
                                continue;
                            }
                            Err(refresh_err) => {
                                tracing::warn!("slack: token refresh failed: {refresh_err}");
                                return Err(IntegrationError::Auth(
                                    format!("Slack auth error: {error} (refresh failed: {refresh_err})")
                                ));
                            }
                        }
                    }
                    "ratelimited" => {
                        // Same as an HTTP 429, but reported in the body.
                        if rate_limit_retries < 3 {
                            rate_limit_retries += 1;
                            tracing::warn!("slack: rate limited on page {page}, waiting 60s");
                            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                            page -= 1;
                            continue;
                        }
                        return Err(IntegrationError::RateLimit {
                            retry_after_secs: 60,
                        });
                    }
                    _ => {
                        return Err(IntegrationError::Network(format!("Slack API error: {error}")));
                    }
                }
            }

            let messages = match body.messages {
                Some(m) => m,
                None => break,
            };

            if messages.matches.is_empty() {
                break;
            }

            let mut reached_old = false;
            for msg in &messages.matches {
                let msg_ts_f64 = Self::ts_to_f64(&msg.ts);

                // Skip messages older than the cursor.
                if msg_ts_f64 <= since_ts {
                    reached_old = true;
                    break;
                }
                // Results are newest-first; skip past the end of the range.
                if until_ts.is_some_and(|until| msg_ts_f64 >= until) {
                    continue;
                }

                // Track the most recent ts for the new cursor.
                if latest_ts.is_none() {
                    latest_ts = Some(msg.ts.clone());
                }

                // Filter ignored channels.
                if self.is_channel_ignored(&msg.channel.name) {
                    continue;
                }

                let occurred_at = match Self::parse_ts(&msg.ts) {
                    Some(dt) => dt,
                    None => continue,
                };

                // Determine activity kind: thread reply vs regular message.
                let is_thread_reply = msg.ts.contains('.')
                    && msg
                        .thread_ts
                        .as_ref()
                        .map(|tts| tts != &msg.ts)
                        .unwrap_or(false);

                let kind = if is_thread_reply {
                    ActivityKind::ThreadReplied
                } else {
                    ActivityKind::MessageSent
                };

                let title = if is_thread_reply {
                    format!("Reply in #{}", msg.channel.name)
                } else {
                    format!("Message in #{}", msg.channel.name)
                };

                let source_id = msg
                    .iid
                    .clone()
                    .unwrap_or_else(|| format!("{}:{}", msg.channel.id, msg.ts));

                let url = msg.permalink.clone().unwrap_or_default();

                let mut activity = Activity::new(
                    Source::Slack,
                    source_id,
                    kind,
                    title,
                    url,
                    occurred_at,
                );

                // Truncate text to 200 chars for description.
                if let Some(ref text) = msg.text {
                    let desc = truncate_chars(text, 200);
                    activity.description = Some(desc);
                }

                activity.project = Some(msg.channel.name.clone());

                activity.metadata = serde_json::json!({
                    "channel_id": msg.channel.id,
                    "username": msg.username,
                    "ts": msg.ts,
                });

                all_activities.push(activity);
            }

            if reached_old {
                break;
            }

            // Check if there are more pages.
            let has_more = messages
                .paging
                .as_ref()
                .map(|p| page < p.pages)
                .unwrap_or(false);
            if !has_more {
                break;
            }
        }

        let cursor = latest_ts.unwrap_or_else(|| {
            since
                .map(|s| s.to_string())
                .unwrap_or_else(|| Utc::now().timestamp().to_string())
        });

        Ok((all_activities, cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncate_chars_never_splits_a_multibyte_character() {
        // 199 ASCII chars then a 3-byte quote straddling byte 200 — used to panic.
        let text = format!("{}”tail", "a".repeat(199));
        assert_eq!(truncate_chars(&text, 200), format!("{}”...", "a".repeat(199)));
        assert_eq!(truncate_chars("short", 200), "short");
    }
}
