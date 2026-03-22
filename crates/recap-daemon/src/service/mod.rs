pub mod launchd;

use std::sync::Arc;

use recap_core::config::AppConfig;
use recap_core::db::Database;
use recap_core::sync::SyncScheduler;

/// Run the background sync loop indefinitely (for use under launchd).
pub async fn run_service() -> anyhow::Result<()> {
    let config = AppConfig::load();
    let db_path = AppConfig::db_path();

    tracing::info!("opening database at {}", db_path.display());
    let db = Arc::new(
        Database::new(&db_path).map_err(|e| anyhow::anyhow!("failed to open database: {e}"))?,
    );

    let scheduler = SyncScheduler::new(db, config);

    tracing::info!("starting background sync service");
    scheduler.start().await;

    // start() loops forever; we only reach here on shutdown.
    Ok(())
}

/// Run a single sync pass and exit.
pub async fn run_once() -> anyhow::Result<()> {
    let config = AppConfig::load();
    let db_path = AppConfig::db_path();

    tracing::info!("opening database at {}", db_path.display());
    let db = Arc::new(
        Database::new(&db_path).map_err(|e| anyhow::anyhow!("failed to open database: {e}"))?,
    );

    let scheduler = SyncScheduler::new(db, config);

    tracing::info!("running one-shot sync");
    scheduler.run_once().await;
    tracing::info!("sync pass complete");

    Ok(())
}
