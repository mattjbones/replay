mod mcp;

use std::sync::Arc;

use clap::Parser;

use recap_core::config::AppConfig;
use recap_core::db::Database;

#[derive(Parser)]
#[command(name = "recap-daemon")]
enum Cli {
    /// Start MCP server (stdio transport)
    Mcp,
    /// Run one sync pass and exit
    Sync,
    /// Print status info
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // MCP servers communicate over stdio, so we must send tracing output to
    // stderr to avoid corrupting the JSON-RPC stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::load();
    let db_path = AppConfig::db_path();
    let db = Arc::new(
        Database::new(&db_path)
            .map_err(|e| anyhow::anyhow!("failed to open database at {}: {e}", db_path.display()))?,
    );

    match cli {
        Cli::Mcp => {
            tracing::info!("starting MCP server on stdio");
            mcp::run_stdio(db, config).await?;
        }
        Cli::Sync => {
            tracing::info!("running one-shot sync");
            let scheduler = recap_core::sync::SyncScheduler::new(Arc::clone(&db), config);
            scheduler.run_once().await;
            tracing::info!("sync complete");
        }
        Cli::Status => {
            let status = recap_core::auth::AuthManager::get_auth_status();
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
    }

    Ok(())
}
