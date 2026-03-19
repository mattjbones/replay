use async_trait::async_trait;
use chrono::Utc;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, Source};

use super::{Integration, IntegrationError};

pub struct SlackIntegration {
    #[allow(dead_code)]
    client: reqwest::Client,
    #[allow(dead_code)]
    token: String,
    #[allow(dead_code)]
    user_id: String,
    #[allow(dead_code)]
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
}

#[async_trait]
impl Integration for SlackIntegration {
    fn source(&self) -> Source {
        Source::Slack
    }

    async fn fetch_activities(
        &self,
        _since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        // TODO: Implement Slack Web API integration.
        let cursor = Utc::now().to_rfc3339();
        Ok((Vec::new(), cursor))
    }
}
