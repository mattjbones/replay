use std::sync::Arc;
use chrono::{DateTime, Utc};
use tokio::task::JoinSet;

use crate::auth::AuthManager;
use crate::config::AppConfig;
use crate::db::{get_sync_cursor, is_cache_fresh, update_sync_cursor, batch_upsert_activities, cleanup_expired_cache, set_sync_status, mark_days_fetched, is_range_fetched};
use crate::db::Database;
use crate::integrations::{Integration, IntegrationError};
use crate::models::Source;

/// What one integration is asked to fetch in a sync pass.
enum Job {
    /// Everything new since this cursor.
    Incremental(String),
    /// Exactly this time range.
    Range(DateTime<Utc>, DateTime<Utc>),
}

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

        // Collect work items for integrations whose cache is stale.
        let mut work: Vec<(usize, Job)> = Vec::new();
        for (idx, integration) in self.integrations.iter().enumerate() {
            let source = integration.source();
            if is_cache_fresh(&self.db, &source, ttl) {
                tracing::debug!("cache fresh for {source}, skipping");
                continue;
            }
            // With no cursor (first run, or after a cache clear) only pick up the last day:
            // history is filled in week by week by `backfill`, not in one huge request.
            let cursor = get_sync_cursor(&self.db, &source)
                .unwrap_or_else(|| (Utc::now() - chrono::Duration::days(1)).to_rfc3339());
            work.push((idx, Job::Incremental(cursor)));
        }

        if work.is_empty() {
            tracing::debug!("all integrations are fresh, nothing to sync");
            return;
        }

        self.run_work(work).await;

        // Clean up expired LLM cache entries
        cleanup_expired_cache(&self.db, self.config.ttl.cold_minutes);
    }

    /// Re-fetch exactly `since..until` from every integration, ignoring cache freshness and
    /// the stored cursors. Fetched activities overwrite stored ones; nothing is deleted.
    /// When every integration succeeds the range's days are marked fetched. Returns whether
    /// every integration succeeded.
    pub async fn refresh_range(&self, since: DateTime<Utc>, until: DateTime<Utc>) -> bool {
        let until = until.min(Utc::now());
        if since >= until {
            return true;
        }
        let work = (0..self.integrations.len()).map(|idx| (idx, Job::Range(since, until))).collect();
        let all_ok = self.run_work(work).await;
        if all_ok {
            mark_days_fetched(&self.db, since, until);
        }
        all_ok
    }

    /// Fill in history newest-first, one week at a time, skipping weeks already fetched.
    /// `on_week` is called after each week with (weeks done, total); returning false stops.
    pub async fn backfill(&self, weeks: u32, on_week: impl Fn(u32, u32) -> bool) {
        let this_week = crate::time::parse_period_range("week", None).map(|(_, start, _)| start);
        let Ok(this_week) = this_week else { return };
        for i in 0..weeks {
            let start = this_week - chrono::Duration::weeks(i as i64);
            let end = start + chrono::Duration::weeks(1);
            if !is_range_fetched(&self.db, start, end) {
                tracing::info!("backfill: week {} of {weeks} ({start})", i + 1);
                // One retry after a pause: failures here are usually rate limits.
                if !self.refresh_range(start, end).await {
                    tracing::warn!("backfill: week {} failed, retrying in 60s", i + 1);
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    self.refresh_range(start, end).await;
                }
            }
            if !on_week(i + 1, weeks) {
                return;
            }
        }
    }

    /// Fan out `work` across integrations concurrently and store the results.
    /// Returns true if every job succeeded.
    async fn run_work(&self, work: Vec<(usize, Job)>) -> bool {
        // Fan-out: run each integration concurrently via JoinSet.
        type SyncResult = (
            Source,
            Arc<Database>,
            Job,
            Result<(Vec<crate::models::Activity>, String), IntegrationError>,
        );
        let mut join_set: JoinSet<SyncResult> = JoinSet::new();
        // Which source each task is for, so a panicking task still gets an amber light.
        let mut task_sources = std::collections::HashMap::new();

        for (idx, job) in work {
            let db = Arc::clone(&self.db);
            let source = self.integrations[idx].source();

            // SAFETY: self.integrations[idx] lives for the duration of this method.
            // We join all tasks before returning, so the pointer remains valid.
            // Integration: Send + Sync, so concurrent &self access is safe.
            let handle = IntegrationHandle {
                ptr: &*self.integrations[idx] as *const dyn Integration,
            };

            let task_source = source.clone();
            let abort = join_set.spawn(async move {
                // SAFETY: pointer is valid -- we join before run_work returns.
                let integration = unsafe { handle.as_ref() };
                let result = match &job {
                    Job::Incremental(cursor) => integration.fetch_activities(Some(cursor)).await,
                    Job::Range(since, until) => integration
                        .fetch_range(*since, *until)
                        .await
                        .map(|acts| (acts, String::new())),
                };
                (source, db, job, result)
            });
            task_sources.insert(abort.id(), task_source);
        }

        let mut all_ok = true;
        while let Some(outcome) = join_set.join_next().await {
            match outcome {
                Ok((source, db, job, Ok((mut activities, new_cursor)))) => {
                    if let Job::Range(since, until) = job {
                        // Only overwrite what's inside the range; anything outside may have
                        // been built from partial data.
                        activities.retain(|a| a.occurred_at >= since && a.occurred_at < until);
                    }
                    let count = activities.len();
                    match batch_upsert_activities(&db, &activities) {
                        Ok(()) => set_sync_status(&db, &source, true, None),
                        Err(e) => {
                            all_ok = false;
                            tracing::error!("failed to batch upsert activities from {source}: {e}");
                            set_sync_status(&db, &source, false, Some(&format!("failed to save activities: {e}")));
                        }
                    }
                    if let Job::Incremental(_) = job {
                        update_sync_cursor(&db, &source, &new_cursor);
                    }
                    tracing::info!("synced {count} activities from {source}");
                }
                Ok((source, db, _, Err(IntegrationError::RateLimit { retry_after_secs }))) => {
                    all_ok = false;
                    tracing::warn!("{source}: rate limited, retry after {retry_after_secs}s");
                    set_sync_status(&db, &source, false, Some(&format!("rate limited, retry after {retry_after_secs}s")));
                }
                Ok((source, db, _, Err(e))) => {
                    all_ok = false;
                    tracing::error!("sync error for {source}: {e}");
                    set_sync_status(&db, &source, false, Some(&e.to_string()));
                }
                Err(join_err) => {
                    all_ok = false;
                    tracing::error!("sync task panicked: {join_err}");
                    if let Some(source) = task_sources.get(&join_err.id()) {
                        set_sync_status(&self.db, source, false, Some(&format!("crashed: {join_err}")));
                    }
                }
            }
        }
        all_ok
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
