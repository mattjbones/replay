mod service;

use clap::Parser;

#[derive(Parser)]
#[command(name = "recap-daemon", about = "Recap background sync daemon")]
enum Cli {
    /// Start background sync loop (for launchd)
    Service,
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
    // Initialise tracing (logs to stdout/stderr, captured by launchd when running as a service)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli {
        Cli::Service => service::run_service().await?,
        Cli::Sync => service::run_once().await?,
        Cli::Install => service::launchd::install()?,
        Cli::Uninstall => service::launchd::uninstall()?,
        Cli::Status => service::launchd::status()?,
    }

    Ok(())
}
