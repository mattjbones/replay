pub mod launchd;

use std::sync::Arc;

use recap_core::config::AppConfig;
use recap_core::db::Database;
use recap_core::sync::SyncScheduler;

/// Load config, open the database, and build a SyncScheduler.
fn init_scheduler() -> anyhow::Result<SyncScheduler> {
    let config = AppConfig::load();
    let db_path = AppConfig::db_path();

    tracing::info!("opening database at {}", db_path.display());
    let db = Arc::new(
        Database::new(&db_path).map_err(|e| anyhow::anyhow!("failed to open database: {e}"))?,
    );

    Ok(SyncScheduler::new(db, config))
}

/// Run the background sync loop indefinitely (for use under launchd).
///
/// Listens for SIGTERM and SIGINT so the daemon shuts down gracefully
/// when launchd (or a user) sends a termination signal.
pub async fn run_service() -> anyhow::Result<()> {
    let scheduler = init_scheduler()?;

    tracing::info!("starting background sync service");

    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    tokio::select! {
        _ = scheduler.start() => {
            // start() loops forever; we only reach here if it somehow exits.
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("received SIGINT, initiating graceful shutdown");
        }
        _ = sigterm.recv() => {
            tracing::info!("received SIGTERM, initiating graceful shutdown");
        }
    }

    tracing::info!("shutdown complete");
    Ok(())
}

/// Run a single sync pass and exit.
pub async fn run_once() -> anyhow::Result<()> {
    let scheduler = init_scheduler()?;

    tracing::info!("running one-shot sync");
    scheduler.run_once().await;
    tracing::info!("sync pass complete");

    Ok(())
}
