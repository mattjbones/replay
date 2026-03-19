use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, ActivityKind, Source};

use super::{Integration, IntegrationError};

pub struct SlackIntegration {
    client: reqwest::Client,
    token: String,
    user_id: String,
    ignored_channels: Vec<String>,
}

impl SlackIntegration {
    pub fn new(config: AppConfig) -> Self {
        let token = AuthManager::get_token(&Source::Slack)
            .ok()
            .flatten()
            .unwrap_or_default();

        let user_id = config.slack.user_id.unwrap_or_default();
        let ignored_channels = config.slack.ignored_channels;

        let client = reqwest::Client::new();
        Self {
            client,
            token,
            user_id,
            ignored_channels,
        }
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
        let since_ts: Option<f64> = since.and_then(Self::parse_since);

        let mut all_activities: Vec<Activity> = Vec::new();
        let mut latest_ts: Option<String> = None;

        // Page through results, up to 3 pages.
        for page in 1..=3u32 {
            let url = format!(
                "https://slack.com/api/search.messages?query=from:<@{}>&sort=timestamp&sort_dir=desc&count=100&page={}",
                self.user_id, page
            );

            let response = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.token))
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
                return Err(match error.as_str() {
                    "invalid_auth" | "not_authed" => {
                        IntegrationError::Auth(format!("Slack auth error: {error}"))
                    }
                    "ratelimited" => IntegrationError::RateLimit {
                        retry_after_secs: 60,
                    },
                    _ => IntegrationError::Network(format!("Slack API error: {error}")),
                });
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
                if let Some(since_val) = since_ts {
                    if msg_ts_f64 <= since_val {
                        reached_old = true;
                        break;
                    }
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
                    let desc = if text.len() > 200 {
                        format!("{}...", &text[..200])
                    } else {
                        text.clone()
                    };
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
