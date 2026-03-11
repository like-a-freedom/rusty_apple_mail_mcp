use anyhow::Result;
use rmcp::ServiceExt;
use rusty_apple_mail_mcp::{config::MailConfig, server::MailMcpServer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let config = MailConfig::from_env()?;
    tracing::info!(?config.mail_directory, ?config.mail_version, "starting server");

    let handler = MailMcpServer::new(config)?;
    let transport = rmcp::transport::io::stdio();

    handler.serve(transport).await?.waiting().await?;
    Ok(())
}
