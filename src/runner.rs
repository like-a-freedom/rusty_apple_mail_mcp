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
        Some(Command::ListMailboxes(_args)) => {
            crate::cli::commands::list_mailboxes(&config)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::error::MailMcpError;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_db_for_version(base: &Path, version: &str) {
        let db_dir = base.join(version).join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder")
            .expect("db placeholder");
    }

    #[test]
    fn build_config_uses_cli_mail_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        create_db_for_version(&mail_directory, "V9");
        let cli = Cli::parse_from([
            "test",
            "--mail-directory",
            mail_directory.to_str().unwrap(),
            "--mail-version",
            "V9",
        ]);

        let config = build_config(&cli).expect("config should build");
        assert_eq!(config.mail_directory, mail_directory);
        assert_eq!(config.mail_version, "V9");
    }

    #[test]
    fn build_config_uses_cli_mail_version() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        create_db_for_version(&mail_directory, "V8");
        let cli = Cli::parse_from([
            "test",
            "--mail-directory",
            mail_directory.to_str().unwrap(),
            "--mail-version",
            "V8",
        ]);
        let config = build_config(&cli).expect("config should build");
        assert_eq!(config.mail_version, "V8");
    }

    #[test]
    fn build_config_works_with_directory_only() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        create_db_for_version(&mail_directory, "V10");
        let cli = Cli::parse_from([
            "test",
            "--mail-directory",
            mail_directory.to_str().unwrap(),
            "--mail-version",
            "V10",
        ]);
        let config = build_config(&cli).expect("config should build");
        assert_eq!(config.mail_directory, mail_directory);
        assert_eq!(config.allowed_account_ids(), None);
    }

    #[test]
    fn build_config_fails_on_invalid_mail_directory() {
        let cli = Cli::parse_from(["test", "--mail-directory", "/nonexistent"]);
        let err = build_config(&cli).expect_err("should fail on missing directory");
        assert!(matches!(err, MailMcpError::DatabaseNotFound { .. }));
    }

    #[test]
    fn build_config_fails_on_empty_mail_version() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        create_db_for_version(&mail_directory, "V10");
        let cli = Cli::parse_from([
            "test",
            "--mail-directory",
            mail_directory.to_str().unwrap(),
            "--mail-version",
            "",
        ]);
        let err = build_config(&cli).expect_err("empty mail version should fail");
        assert!(matches!(err, MailMcpError::Config(_)));
    }
}
