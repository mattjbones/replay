use std::sync::Arc;
use tokio::task::JoinSet;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::db::{get_sync_cursor, is_cache_fresh, update_sync_cursor, upsert_activity};
use crate::db::Database;
use crate::integrations::{Integration, IntegrationError};
use crate::models::Source;

pub struct SyncScheduler {
    db: Arc<Database>,
    config: AppConfig,
    integrations: Vec<Box<dyn Integration>>,
}

/// A wrapper around a raw pointer to a trait object that is Send.
///
/// SAFETY: The caller must guarantee the pointee outlives all uses and that
/// concurrent reads are safe. The `Integration` trait requires `Send + Sync`,
/// so concurrent `&self` access is fine.
struct IntegrationHandle {
    ptr: *const dyn Integration,
}

unsafe impl Send for IntegrationHandle {}
unsafe impl Sync for IntegrationHandle {}

impl IntegrationHandle {
    /// SAFETY: The returned reference must not outlive the pointee.
    unsafe fn as_ref(&self) -> &dyn Integration {
        &*self.ptr
    }
}

impl SyncScheduler {
    /// Build a new scheduler, registering only the integrations whose tokens are available.
    pub fn new(db: Arc<Database>, config: AppConfig) -> Self {
        let mut integrations: Vec<Box<dyn Integration>> = Vec::new();

        // Each integration constructor retrieves its own token internally,
        // but we only instantiate if a token is actually available.
        if let Some(gh) = crate::integrations::github::GitHubIntegration::new(config.clone()) {
            integrations.push(Box::new(gh));
        }

        if AuthManager::get_token(&Source::Linear).ok().flatten().is_some() {
            integrations.push(Box::new(
                crate::integrations::linear::LinearIntegration::new(config.clone()),
            ));
        }

        if AuthManager::get_token(&Source::Slack).ok().flatten().is_some() {
            integrations.push(Box::new(
                crate::integrations::slack::SlackIntegration::new(config.clone()),
            ));
        }

        if AuthManager::get_token(&Source::Notion).ok().flatten().is_some() {
            integrations.push(Box::new(
                crate::integrations::notion::NotionIntegration::new(config.clone()),
            ));
        }

        Self {
            db,
            config,
            integrations,
        }
    }

    /// Run a single sync pass across all registered integrations concurrently.
    pub async fn run_once(&self) {
        let ttl = self.config.ttl.hot_minutes;

        // Collect work items: (index, cursor) for integrations whose cache is stale.
        let mut work: Vec<(usize, Option<String>)> = Vec::new();
        for (idx, integration) in self.integrations.iter().enumerate() {
            let source = integration.source();
            if is_cache_fresh(&self.db, &source, ttl) {
                tracing::debug!("cache fresh for {source}, skipping");
                continue;
            }
            let cursor = get_sync_cursor(&self.db, &source);
            work.push((idx, cursor));
        }

        if work.is_empty() {
            tracing::debug!("all integrations are fresh, nothing to sync");
            return;
        }

        // Fan-out: run each stale integration concurrently via JoinSet.
        let mut join_set: JoinSet<(
            Source,
            Arc<Database>,
            Result<(Vec<crate::models::Activity>, String), IntegrationError>,
        )> = JoinSet::new();

        for (idx, cursor) in work {
            let db = Arc::clone(&self.db);
            let source = self.integrations[idx].source();
            let cursor_owned = cursor;

            // SAFETY: self.integrations[idx] lives for the duration of this method.
            // We join all tasks before returning, so the pointer remains valid.
            // Integration: Send + Sync, so concurrent &self access is safe.
            let handle = IntegrationHandle {
                ptr: &*self.integrations[idx] as *const dyn Integration,
            };

            join_set.spawn(async move {
                // SAFETY: pointer is valid -- we join before run_once returns.
                let integration = unsafe { handle.as_ref() };
                let result = integration
                    .fetch_activities(cursor_owned.as_deref())
                    .await;
                (source, db, result)
            });
        }

        while let Some(outcome) = join_set.join_next().await {
            match outcome {
                Ok((source, db, Ok((activities, new_cursor)))) => {
                    let count = activities.len();
                    for activity in &activities {
                        if let Err(e) = upsert_activity(&db, activity) {
                            tracing::error!("failed to upsert activity: {e}");
                        }
                    }
                    update_sync_cursor(&db, &source, &new_cursor);
                    tracing::info!("synced {count} activities from {source}");
                }
                Ok((source, _db, Err(IntegrationError::RateLimit { retry_after_secs }))) => {
                    tracing::warn!("{source}: rate limited, retry after {retry_after_secs}s");
                }
                Ok((source, _db, Err(e))) => {
                    tracing::error!("sync error for {source}: {e}");
                }
                Err(join_err) => {
                    tracing::error!("sync task panicked: {join_err}");
                }
            }
        }
    }

    /// Spawn a background loop that calls `run_once` on the configured interval.
    pub async fn start(self) {
        let interval_mins = self.config.schedule.sync_interval_minutes;
        let duration = tokio::time::Duration::from_secs(interval_mins * 60);

        tracing::info!("sync scheduler started, interval = {interval_mins}m");

        // Run immediately on startup, then loop.
        self.run_once().await;

        let mut interval = tokio::time::interval(duration);
        // The first tick completes immediately; we already ran once, so skip it.
        interval.tick().await;

        loop {
            interval.tick().await;
            self.run_once().await;
        }
    }
}
