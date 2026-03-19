use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, ActivityKind, Source};

use super::{Integration, IntegrationError};

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";
const MAX_PAGES: usize = 2;

/// Threshold in seconds: if `last_edited_time - created_time` <= this value
/// the page is considered newly created rather than edited.
const CREATED_THRESHOLD_SECS: i64 = 60;

pub struct NotionIntegration {
    client: reqwest::Client,
    #[allow(dead_code)]
    token: String,
}

impl NotionIntegration {
    pub fn new(_config: AppConfig) -> Self {
        let token = AuthManager::get_token(&Source::Notion)
            .ok()
            .flatten()
            .unwrap_or_default();

        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

        let mut headers = HeaderMap::new();
        if !token.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .expect("invalid token for header"),
            );
        }
        headers.insert(
            reqwest::header::HeaderName::from_static("notion-version"),
            HeaderValue::from_static(NOTION_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build reqwest client");

        Self { client, token }
    }
}

// ---------------------------------------------------------------------------
// Notion Search API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<NotionPage>,
    has_more: bool,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotionPage {
    id: String,
    url: String,
    created_time: String,
    last_edited_time: String,
    properties: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Integration implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Integration for NotionIntegration {
    fn source(&self) -> Source {
        Source::Notion
    }

    async fn fetch_activities(
        &self,
        since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        let since_dt: Option<DateTime<Utc>> = since.and_then(|s| s.parse().ok());

        let mut all_activities: Vec<Activity> = Vec::new();
        let mut latest_cursor: Option<String> = None;
        let mut next_cursor: Option<String> = None;

        for _page_num in 0..MAX_PAGES {
            let mut body = serde_json::json!({
                "filter": { "property": "object", "value": "page" },
                "sort": { "direction": "descending", "timestamp": "last_edited_time" },
                "page_size": 50
            });

            if let Some(ref cursor) = next_cursor {
                body.as_object_mut()
                    .unwrap()
                    .insert("start_cursor".to_string(), serde_json::json!(cursor));
            }

            let url = format!("{NOTION_API_BASE}/search");

            let response = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| IntegrationError::Network(e.to_string()))?;

            let status = response.status();

            if status == reqwest::StatusCode::UNAUTHORIZED {
                let body_text = response.text().await.unwrap_or_default();
                return Err(IntegrationError::Auth(format!(
                    "Notion returned {status}: {body_text}"
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
                    "Notion returned {status}: {body_text}"
                )));
            }

            let search_resp: SearchResponse = response
                .json()
                .await
                .map_err(|e| IntegrationError::Parse(e.to_string()))?;

            if search_resp.results.is_empty() {
                break;
            }

            let mut reached_old = false;
            for page in &search_resp.results {
                let edited_dt: DateTime<Utc> = page
                    .last_edited_time
                    .parse()
                    .map_err(|e: chrono::ParseError| IntegrationError::Parse(e.to_string()))?;

                // Stop if we've gone past our since boundary.
                if let Some(ref since) = since_dt {
                    if edited_dt <= *since {
                        reached_old = true;
                        break;
                    }
                }

                // Track the most recent timestamp for the cursor.
                if latest_cursor.is_none() {
                    latest_cursor = Some(page.last_edited_time.clone());
                }

                let created_dt: DateTime<Utc> = page
                    .created_time
                    .parse()
                    .unwrap_or(edited_dt);

                let diff_secs = (edited_dt - created_dt).num_seconds().abs();
                let kind = if diff_secs <= CREATED_THRESHOLD_SECS {
                    ActivityKind::PageCreated
                } else {
                    ActivityKind::PageEdited
                };

                let title = extract_title(&page.properties);

                let activity = Activity::new(
                    Source::Notion,
                    page.id.clone(),
                    kind,
                    title,
                    page.url.clone(),
                    edited_dt,
                );

                all_activities.push(activity);
            }

            if reached_old || !search_resp.has_more {
                break;
            }

            next_cursor = search_resp.next_cursor;
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
// Helpers
// ---------------------------------------------------------------------------

/// Extract the page title from the properties object.
///
/// Notion stores the title in a property of type `"title"`. Common names are
/// `"title"`, `"Name"`, and `"Title"`. We try those first, then fall back to
/// scanning all properties for one with a `"title"` array.
fn extract_title(properties: &serde_json::Value) -> String {
    // Try well-known property names first.
    for key in &["title", "Name", "Title"] {
        if let Some(text) = extract_title_from_prop(properties.get(key)) {
            return text;
        }
    }

    // Fall back: iterate all properties looking for one with type "title".
    if let Some(obj) = properties.as_object() {
        for (_key, value) in obj {
            if value.get("type").and_then(|t| t.as_str()) == Some("title") {
                if let Some(text) = extract_title_from_prop(Some(value)) {
                    return text;
                }
            }
        }
    }

    "Untitled".to_string()
}

/// Given a single property value, try to pull out `title[0].plain_text`.
fn extract_title_from_prop(prop: Option<&serde_json::Value>) -> Option<String> {
    let prop = prop?;
    let title_array = prop.get("title")?.as_array()?;
    let first = title_array.first()?;
    let text = first.get("plain_text")?.as_str()?;
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}
