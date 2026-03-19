use async_trait::async_trait;
use chrono::Utc;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::models::{Activity, Source};

use super::{Integration, IntegrationError};

pub struct NotionIntegration {
    #[allow(dead_code)]
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

        let client = reqwest::Client::new();
        Self { client, token }
    }
}

#[async_trait]
impl Integration for NotionIntegration {
    fn source(&self) -> Source {
        Source::Notion
    }

    async fn fetch_activities(
        &self,
        _since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError> {
        // TODO: Implement Notion API integration.
        let cursor = Utc::now().to_rfc3339();
        Ok((Vec::new(), cursor))
    }
}
