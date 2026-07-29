//! Application runner for CLI and stdio server startup.

use std::sync::Once;

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::cli::execution;
use crate::cli::{Cli, Command};
use crate::config::{MailConfig, MailConfigBuilder, MailConfigOverrides};
use crate::error::MailMcpError;
use crate::server::MailMcpServer;

static TRACING_INIT: Once = Once::new();

/// Initialize tracing subscribers for the application.
fn init_tracing(log_level: Option<&str>) {
    TRACING_INIT.call_once(|| {
        let filter = std::env::var("RUST_LOG")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| log_level.map(String::from))
            .unwrap_or_else(|| "warn".to_owned());
        let _ = tracing_subscriber::fmt()
            .compact()
            .with_ansi(false)
            .with_target(false)
            .with_env_filter(
                EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::from_default_env()),
            )
            .with_writer(std::io::stderr)
            .try_init();
    });
}

/// Run the application.
///
/// This is the main entry point called from `main.rs`.
/// All startup logic is centralized here for testability.
pub async fn run() -> Result<()> {
    let used_legacy_scope_account = legacy_scope_account_used(std::env::args_os());
    let cli = Cli::parse();
    let config = match build_config(&cli) {
        Ok(config) => config,
        Err(err) => {
            init_tracing(None);
            warn_legacy_scope_account(used_legacy_scope_account);
            execution::write_error(&err);
            std::process::exit(i32::from(execution::exit_code::CONFIG));
        }
    };

    init_tracing(config.log_level.as_deref());
    warn_legacy_scope_account(used_legacy_scope_account);

    match cli.command {
        Some(Command::ListAccounts(args)) => {
            let exit = execution::execute(
                || crate::cli::commands::list_accounts(&config, args.include_mailboxes),
                cli.pretty,
            );
            std::process::exit(i32::from(exit));
        }
        Some(Command::Search(args)) => {
            let exit = execution::execute(
                || crate::cli::commands::search_messages(&config, args),
                cli.pretty,
            );
            std::process::exit(i32::from(exit));
        }
        Some(Command::GetMessage(args)) => {
            let exit = execution::execute(
                || crate::cli::commands::get_message(&config, args),
                cli.pretty,
            );
            std::process::exit(i32::from(exit));
        }
        Some(Command::GetAttachment(args)) => {
            let exit = execution::execute(
                || crate::cli::commands::get_attachment(&config, args),
                cli.pretty,
            );
            std::process::exit(i32::from(exit));
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
    MailConfigBuilder::from_overrides(MailConfigOverrides {
        mail_directory: cli.mail_directory.clone(),
        mail_version: cli.mail_version.clone(),
        account: cli.scope_account.clone(),
    })
}

fn legacy_scope_account_used(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    const SUBCOMMANDS: [&str; 4] = ["list-accounts", "search", "get-message", "get-attachment"];

    for arg in args.into_iter().skip(1) {
        let arg = arg.to_string_lossy();
        if arg == "--" || SUBCOMMANDS.contains(&arg.as_ref()) {
            break;
        }
        if arg == "--account" || arg.starts_with("--account=") {
            return true;
        }
    }
    false
}

fn warn_legacy_scope_account(used_legacy_scope_account: bool) {
    if used_legacy_scope_account {
        eprintln!(
            "warning: top-level --account is deprecated for startup Scope; use --scope-account. search --account remains the per-call Filter."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::error::MailMcpError;
    use std::ffi::OsString;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_db_for_version(base: &Path, version: &str) {
        let db_dir = base.join(version).join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder")
            .expect("db placeholder");
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn legacy_scope_account_used_detects_top_level_alias() {
        assert!(legacy_scope_account_used(args(&[
            "test",
            "--account",
            "Work",
            "search",
            "--sender",
            "a@example.com",
        ])));
    }

    #[test]
    fn legacy_scope_account_used_ignores_search_filter() {
        assert!(!legacy_scope_account_used(args(&[
            "test",
            "search",
            "--account",
            "Work",
        ])));
    }

    #[test]
    fn legacy_scope_account_used_ignores_canonical_scope_flag() {
        assert!(!legacy_scope_account_used(args(&[
            "test",
            "--scope-account",
            "Work",
            "list-accounts",
        ])));
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
