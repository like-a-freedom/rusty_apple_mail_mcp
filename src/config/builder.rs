use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::MailMcpError;

use super::MailConfig;
use super::accounts::load_account_metadata_for_selectors;
use super::parser::{MailConfigOverrides, parse_account_selectors};
use super::paths::{default_mail_directory, normalize_mail_directory};
use super::yaml_config::YamlConfig;

/// Builder for MailConfig with automatic env var and default resolution.
#[derive(Debug, Default)]
pub struct MailConfigBuilder {
    mail_directory: Option<PathBuf>,
    mail_version: Option<String>,
    allowed_accounts: Option<Vec<String>>,
    load_metadata: bool,
}

impl MailConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mail_directory(mut self, path: PathBuf) -> Self {
        self.mail_directory = Some(path);
        self
    }

    pub fn mail_version(mut self, version: String) -> Self {
        self.mail_version = Some(version);
        self
    }

    pub fn allowed_accounts(mut self, accounts: Vec<String>) -> Self {
        self.allowed_accounts = Some(accounts);
        self
    }

    pub fn load_account_metadata(mut self) -> Self {
        self.load_metadata = true;
        self
    }

    pub fn build(self) -> Result<MailConfig, MailMcpError> {
        let yaml = YamlConfig::load().unwrap_or_default();

        let mail_directory = match self.mail_directory {
            Some(path) => path,
            None => match std::env::var("APPLE_MAIL_DIR").ok() {
                Some(dir) => PathBuf::from(dir),
                None => match yaml.apple_mail_dir {
                    Some(dir) => super::yaml_config::expand_tilde(&dir),
                    None => default_mail_directory(),
                },
            },
        };
        let mail_directory = normalize_mail_directory(mail_directory);

        let mail_version = match self.mail_version {
            Some(v) => v,
            None => std::env::var("APPLE_MAIL_VERSION")
                .ok()
                .or(yaml.apple_mail_version)
                .unwrap_or_else(|| "V10".to_string()),
        };

        let account_selectors: Vec<String> = match self.allowed_accounts {
            Some(accounts) => accounts,
            None => {
                let from_env = std::env::var("APPLE_MAIL_ACCOUNT").ok();
                let from_yaml = yaml.apple_mail_account;
                let raw = from_env.as_deref().or(from_yaml.as_deref());
                parse_account_selectors(raw)?
            }
        };

        let log_level = std::env::var("APPLE_MAIL_LOG_LEVEL")
            .ok()
            .or(yaml.log_level);

        let account_metadata = if self.load_metadata && !account_selectors.is_empty() {
            load_account_metadata_for_selectors(&account_selectors)?
        } else {
            HashMap::new()
        };

        let allowed_account_ids = if account_selectors.is_empty() {
            None
        } else {
            Some(
                crate::accounts::resolve_account_selectors(&account_selectors, &account_metadata)?
                    .into_iter()
                    .collect(),
            )
        };

        MailConfig::new(
            mail_directory,
            mail_version,
            allowed_account_ids,
            account_metadata,
            log_level,
        )
    }

    /// Build from CLI overrides.
    pub fn from_overrides(overrides: MailConfigOverrides) -> Result<MailConfig, MailMcpError> {
        let mut builder = MailConfigBuilder::new();
        if let Some(dir) = overrides.mail_directory {
            builder = builder.mail_directory(dir);
        }
        if let Some(ver) = overrides.mail_version {
            builder = builder.mail_version(ver);
        }
        if let Some(accounts) = overrides.account {
            let selectors = parse_account_selectors(Some(&accounts))?;
            builder = builder.allowed_accounts(selectors);
        }
        builder.load_account_metadata().build()
    }
}
