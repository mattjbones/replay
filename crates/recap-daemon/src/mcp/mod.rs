pub mod resources;
pub mod tools;

use std::sync::Arc;

use rmcp::ServiceExt;

use recap_core::config::AppConfig;
use recap_core::db::Database;

pub use tools::RecapServer;

/// Start the MCP server on stdio transport.
pub async fn run_stdio(db: Arc<Database>, config: AppConfig) -> anyhow::Result<()> {
    let server = RecapServer::new(db, config);
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await
        .inspect_err(|e| tracing::error!("MCP server error: {e}"))?;
    service.waiting().await?;
    Ok(())
}
