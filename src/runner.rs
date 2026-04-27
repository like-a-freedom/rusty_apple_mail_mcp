//! Application runner for CLI and stdio server startup.

use std::sync::Once;

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};
use crate::config::{MailConfig, MailConfigOverrides};
use crate::error::MailMcpError;
use crate::server::MailMcpServer;

static TRACING_INIT: Once = Once::new();

/// Initialize tracing subscribers for the application.
fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .compact()
            .with_ansi(false)
            .with_target(false)
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    });
}

/// Run the application.
///
/// This is the main entry point called from `main.rs`.
/// All startup logic is centralized here for testability.
pub async fn run() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let config = build_config(&cli)?;

    match cli.command {
        Some(Command::ListAccounts(args)) => {
            crate::cli::commands::list_accounts(&config, args.include_mailboxes)?;
        }
        Some(Command::ListMailboxes(args)) => {
            crate::cli::commands::list_mailboxes(&config, args.account)?;
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

fn build_config(cli: &Cli) -> Result<MailConfig, MailMcpError> {
    MailConfig::from_overrides(MailConfigOverrides {
        mail_directory: cli.mail_directory.clone(),
        mail_version: cli.mail_version.clone(),
        account: cli.account.clone(),
    })
}
