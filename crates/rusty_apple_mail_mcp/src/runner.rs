//! Application runner - contains the main execution logic.
//!
//! This module provides the `run()` function called from `main.rs`.
//! All business logic is here, making it testable without running the binary.

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};
use rusty_apple_mail_core::config::MailConfig;
use crate::server::MailMcpServer;

/// Initialize tracing subscribers for the application.
fn init_tracing() {
    tracing_subscriber::fmt()
        .compact()
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
}

/// Run the application.
///
/// This is the main entry point called from `main.rs`.
/// All business logic is centralized here for testability.
pub async fn run() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();

    // Build config from CLI args or environment
    let config = if let (Some(mail_dir), Some(mail_version), Some(_account)) =
        (cli.mail_directory, cli.mail_version, cli.account)
    {
        MailConfig::from_parts(mail_dir, mail_version)?
    } else {
        MailConfig::from_env()?
    };

    match cli.command {
        Some(Command::ListAccounts(args)) => {
            crate::cli::commands::list_accounts(&config, args.include_mailboxes)?;
        }
        Some(Command::ListMailboxes(_args)) => {
            crate::cli::commands::list_mailboxes(&config, None)?;
        }
        Some(Command::Search(args)) => {
            crate::cli::commands::search_messages(&config, args)?;
        }
        Some(Command::GetMessage(args)) => {
            crate::cli::commands::get_message(&config, args)?;
        }
        Some(Command::GetAttachment(args)) => {
            crate::cli::commands::get_attachment(&config, args)?;
        }
        None => {
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
