//! Application runner for CLI and stdio server startup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Once;

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::accounts::{
    AccountMetadata, default_accounts_db_path, load_account_metadata, resolve_account_selectors,
};
use crate::cli::{Cli, Command};
use crate::config::MailConfig;
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
    let mail_directory = cli
        .mail_directory
        .clone()
        .map(normalize_mail_directory)
        .or_else(|| {
            std::env::var("APPLE_MAIL_DIR")
                .ok()
                .map(|raw| expand_mail_directory(&raw))
        })
        .unwrap_or_else(default_mail_directory);

    let mail_version = cli
        .mail_version
        .clone()
        .or_else(|| std::env::var("APPLE_MAIL_VERSION").ok())
        .unwrap_or_else(|| "V10".to_string());

    let raw_account_selectors = cli
        .account
        .clone()
        .or_else(|| std::env::var("APPLE_MAIL_ACCOUNT").ok());
    let account_selectors = parse_account_selectors(raw_account_selectors.as_deref())?;

    let account_metadata = load_metadata_for_selectors(&account_selectors)?;
    let allowed_account_ids = if account_selectors.is_empty() {
        None
    } else {
        Some(resolve_account_selectors(
            &account_selectors,
            &account_metadata,
        )?)
    };

    MailConfig::from_parts_with_accounts(
        mail_directory,
        mail_version,
        allowed_account_ids,
        account_metadata,
    )
}

fn load_metadata_for_selectors(
    account_selectors: &[String],
) -> Result<HashMap<String, AccountMetadata>, MailMcpError> {
    let accounts_db_path = default_accounts_db_path();
    let Some(path) = accounts_db_path.as_deref() else {
        return if account_selectors.is_empty() {
            Ok(HashMap::new())
        } else {
            Err(MailMcpError::Config(
                "APPLE_MAIL_ACCOUNT is set, but the home directory could not be resolved"
                    .to_string(),
            ))
        };
    };

    if !path.exists() {
        return if account_selectors.is_empty() {
            Ok(HashMap::new())
        } else {
            Err(MailMcpError::Config(format!(
                "APPLE_MAIL_ACCOUNT is set, but Accounts database was not found at {}",
                path.display()
            )))
        };
    }

    match load_account_metadata(path) {
        Ok(metadata) => Ok(metadata),
        Err(_) if account_selectors.is_empty() => Ok(HashMap::new()),
        Err(error) => Err(error),
    }
}

fn default_mail_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/Mail")
}

fn normalize_mail_directory(path: PathBuf) -> PathBuf {
    expand_mail_directory(&path.to_string_lossy())
}

fn expand_mail_directory(raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }

    if let Some(stripped) = raw.strip_prefix("~/")
        && let Some(home_dir) = dirs::home_dir()
    {
        return home_dir.join(stripped);
    }

    PathBuf::from(raw)
}

fn parse_account_selectors(raw: Option<&str>) -> Result<Vec<String>, MailMcpError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let selectors = raw
        .split(',')
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if selectors.is_empty() {
        return Err(MailMcpError::Config(
            "APPLE_MAIL_ACCOUNT was provided, but no account selectors were found".to_string(),
        ));
    }

    Ok(selectors)
}
