use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use rusty_apple_mail_mcp::{cli, config::MailConfig, server::MailMcpServer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .compact()
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();

    // Build config from CLI args or environment
    let config = if let (Some(mail_dir), Some(mail_version), Some(_account)) =
        (cli.mail_directory, cli.mail_version, cli.account)
    {
        MailConfig::from_parts(mail_dir, mail_version)?
    } else {
        MailConfig::from_env()?
    };

    match cli.command {
        Some(cli::Command::ListAccounts(args)) => {
            cli::commands::list_accounts(&config, args.include_mailboxes)?;
        }
        Some(cli::Command::ListMailboxes(_args)) => {
            cli::commands::list_mailboxes(&config, None)?;
        }
        Some(cli::Command::Search(args)) => {
            cli::commands::search_messages(&config, args)?;
        }
        Some(cli::Command::GetMessage(args)) => {
            cli::commands::get_message(&config, args)?;
        }
        Some(cli::Command::GetAttachment(args)) => {
            cli::commands::get_attachment(&config, args)?;
        }
        None => {
            // Run as MCP server (default behavior)
            tracing::info!(
                "starting server (mail_directory={}, mail_version={})",
                config.mail_directory.display(),
                config.mail_version
            );

            let handler = MailMcpServer::new(config)?;
            let transport = rmcp::transport::io::stdio();

            handler.serve(transport).await?.waiting().await?;
        }
    }

    Ok(())
}
