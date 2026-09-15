pub mod launchd;

use std::sync::Arc;
use std::time::Duration;

use recap_core::config::AppConfig;
use recap_core::db::Database;
use recap_core::sync::SyncScheduler;

use tokio::signal::unix::{signal, SignalKind};

/// Open the database at the configured path.
fn open_db() -> anyhow::Result<Arc<Database>> {
    let db_path = AppConfig::db_path();
    tracing::info!("opening database at {}", db_path.display());
    Ok(Arc::new(
        Database::new(&db_path).map_err(|e| anyhow::anyhow!("failed to open database: {e}"))?,
    ))
}

/// Run the background sync loop indefinitely (for use under launchd).
///
/// Each pass rebuilds the scheduler from a fresh `AppConfig`, so integrations
/// connected (or tokens rotated) while the daemon is running are picked up on
/// the next pass without a restart.
///
/// SIGTERM / SIGINT are honoured between passes. A pass that is already
/// running is allowed to finish: `SyncScheduler::run_once` spawns tasks that
/// borrow the scheduler and must be joined before it is dropped.
pub async fn run_service() -> anyhow::Result<()> {
    let db = open_db()?;

    // Install both listeners up front so a signal that arrives mid-pass is
    // not lost; it is observed as soon as the pass completes.
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tracing::info!("starting background sync service");

    loop {
        let config = AppConfig::load();
        let interval_mins = config.schedule.sync_interval_minutes.max(1);
        let scheduler = SyncScheduler::new(Arc::clone(&db), config);

        scheduler.run_once().await;
        tracing::info!("sync pass complete; next pass in {interval_mins}m");

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval_mins * 60)) => {}
            _ = sigint.recv() => {
                tracing::info!("received SIGINT, shutting down");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, shutting down");
                break;
            }
        }
    }

    tracing::info!("shutdown complete");
    Ok(())
}

/// Run a single sync pass and exit.
pub async fn run_once() -> anyhow::Result<()> {
    let db = open_db()?;
    let scheduler = SyncScheduler::new(db, AppConfig::load());

    tracing::info!("running one-shot sync");
    scheduler.run_once().await;
    tracing::info!("sync pass complete");

    Ok(())
}
