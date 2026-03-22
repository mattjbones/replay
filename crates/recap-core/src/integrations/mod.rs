pub mod github;
pub mod linear;
pub mod notion;
pub mod slack;

use async_trait::async_trait;

use crate::models::Activity;

#[async_trait]
pub trait Integration: Send + Sync {
    /// Returns the source this integration fetches from.
    fn source(&self) -> crate::models::Source;

    /// Fetch activities since the given cursor (ISO8601 timestamp or pagination cursor).
    /// Returns (activities, new_cursor).
    async fn fetch_activities(
        &self,
        since: Option<&str>,
    ) -> Result<(Vec<Activity>, String), IntegrationError>;
}

#[derive(Debug)]
pub enum IntegrationError {
    Auth(String),
    RateLimit { retry_after_secs: u64 },
    Network(String),
    Parse(String),
}

impl std::fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntegrationError::Auth(msg) => write!(f, "authentication error: {msg}"),
            IntegrationError::RateLimit { retry_after_secs } => {
                write!(f, "rate limited, retry after {retry_after_secs}s")
            }
            IntegrationError::Network(msg) => write!(f, "network error: {msg}"),
            IntegrationError::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for IntegrationError {}
