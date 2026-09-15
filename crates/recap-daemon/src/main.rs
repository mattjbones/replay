mod mcp;
mod service;

use std::sync::Arc;

use clap::Parser;

use recap_core::config::AppConfig;
use recap_core::db::Database;

#[derive(Parser)]
#[command(name = "recap-daemon", about = "Recap background sync daemon and MCP server")]
enum Cli {
    /// Start background sync loop (for launchd)
    Service,
    /// Start MCP server (stdio transport)
    Mcp,
    /// Run one sync pass and exit
    Sync,
    /// Install macOS LaunchAgent
    Install,
    /// Remove macOS LaunchAgent
    Uninstall,
    /// Print status info
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // MCP servers communicate over stdio, so tracing must go to stderr to
    // avoid corrupting the JSON-RPC stream. Under launchd stderr is captured
    // to daemon.err.log, so this is right for the service too.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli {
        Cli::Service => service::run_service().await?,
        Cli::Mcp => {
            let config = AppConfig::load();
            let db_path = AppConfig::db_path();
            let db = Arc::new(Database::new(&db_path).map_err(|e| {
                anyhow::anyhow!("failed to open database at {}: {e}", db_path.display())
            })?);
            tracing::info!("starting MCP server on stdio");
            mcp::run_stdio(db, config).await?;
        }
        Cli::Sync => service::run_once().await?,
        Cli::Install => service::launchd::install()?,
        Cli::Uninstall => service::launchd::uninstall()?,
        Cli::Status => service::launchd::status()?,
    }

    Ok(())
}
