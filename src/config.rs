use std::collections::HashMap;
use std::path::PathBuf;

use crate::accounts::{
    AccountMetadata, default_accounts_db_path, load_account_metadata, resolve_account_selectors,
};
use crate::db::mailbox_account_id;
use crate::error::MailMcpError;

const DEFAULT_MAIL_VERSION: &str = "V10";

/// Server configuration. Loaded strictly from environment variables.
#[derive(Debug, Clone)]
pub struct MailConfig {
    pub mail_directory: PathBuf,
    pub mail_version: String,
    pub allowed_account_ids: Option<Vec<String>>,
    pub account_metadata: HashMap<String, AccountMetadata>,
}

impl MailConfig {
    /// Resolve configuration from environment variables, falling back to defaults.
    /// `APPLE_MAIL_DIR`, `APPLE_MAIL_VERSION`, `APPLE_MAIL_ACCOUNT`.
    pub fn from_env() -> Result<Self, MailMcpError> {
        let mail_directory = std::env::var("APPLE_MAIL_DIR")
            .map(|raw| expand_mail_directory(&raw))
            .unwrap_or_else(|_| default_mail_directory());
        let mail_version = std::env::var("APPLE_MAIL_VERSION")
            .unwrap_or_else(|_| DEFAULT_MAIL_VERSION.to_string());
        let account_selectors =
            parse_account_selectors(std::env::var("APPLE_MAIL_ACCOUNT").ok().as_deref())?;

        let accounts_db_path = default_accounts_db_path();
        let account_metadata = if let Some(path) = accounts_db_path.as_deref() {
            if path.exists() {
                match load_account_metadata(path) {
                    Ok(metadata) => metadata,
                    Err(_) if account_selectors.is_empty() => HashMap::new(),
                    Err(e) => return Err(e),
                }
            } else if account_selectors.is_empty() {
                HashMap::new()
            } else {
                return Err(MailMcpError::Config(format!(
                    "APPLE_MAIL_ACCOUNT is set, but Accounts database was not found at {}",
                    path.display()
                )));
            }
        } else if account_selectors.is_empty() {
            HashMap::new()
        } else {
            return Err(MailMcpError::Config(
                "APPLE_MAIL_ACCOUNT is set, but the home directory could not be resolved"
                    .to_string(),
            ));
        };

        let allowed_account_ids = if account_selectors.is_empty() {
            None
        } else {
            Some(resolve_account_selectors(
                &account_selectors,
                &account_metadata,
            )?)
        };

        Self::from_parts_with_accounts(
            mail_directory,
            mail_version,
            allowed_account_ids,
            account_metadata,
        )
    }

    /// Build a configuration from already-resolved values and validate it.
    pub fn from_parts(mail_directory: PathBuf, mail_version: String) -> Result<Self, MailMcpError> {
        Self::from_parts_with_accounts(mail_directory, mail_version, None, HashMap::new())
    }

    /// Build a configuration with pre-resolved account metadata and optional allowlist.
    pub fn from_parts_with_accounts(
        mail_directory: PathBuf,
        mail_version: String,
        allowed_account_ids: Option<Vec<String>>,
        account_metadata: HashMap<String, AccountMetadata>,
    ) -> Result<Self, MailMcpError> {
        let config = Self {
            mail_directory,
            mail_version,
            allowed_account_ids,
            account_metadata,
        };
        config.validate()?;
        Ok(config)
    }

    /// Absolute path to the Envelope Index SQLite database.
    pub fn envelope_db_path(&self) -> PathBuf {
        self.mail_directory
            .join(&self.mail_version)
            .join("MailData")
            .join("Envelope Index")
    }

    /// Returns the configured allowlist of account IDs, if any.
    pub fn allowed_account_ids(&self) -> Option<&[String]> {
        self.allowed_account_ids.as_deref()
    }

    /// Returns `true` if the given account is permitted by the current configuration.
    pub fn is_account_allowed(&self, account_id: &str) -> bool {
        self.allowed_account_ids
            .as_ref()
            .is_none_or(|allowed| allowed.iter().any(|candidate| candidate == account_id))
    }

    /// Returns `true` if the mailbox URL belongs to an allowed account.
    pub fn is_mailbox_allowed(&self, mailbox_url: &str) -> bool {
        mailbox_account_id(mailbox_url)
            .as_deref()
            .is_none_or(|account_id| self.is_account_allowed(account_id))
    }

    /// Returns friendly metadata for the given canonical account ID.
    pub fn account_metadata(&self, account_id: &str) -> Option<&AccountMetadata> {
        self.account_metadata.get(account_id)
    }

    /// Validate the configuration for env-only stdio startup.
    pub fn validate(&self) -> Result<(), MailMcpError> {
        if self.mail_version.trim().is_empty() {
            return Err(MailMcpError::Config(
                "APPLE_MAIL_VERSION must not be empty".to_string(),
            ));
        }

        let db_path = self.envelope_db_path();
        if !db_path.exists() {
            return Err(MailMcpError::DatabaseNotFound { path: db_path });
        }

        Ok(())
    }
}

fn default_mail_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/Mail")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::AccountMetadata;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn make_valid_config_inputs() -> (TempDir, PathBuf, String) {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let db_path = mail_directory.join(&mail_version).join("MailData");
        std::fs::create_dir_all(&db_path).expect("mail data dir");
        std::fs::write(db_path.join("Envelope Index"), b"sqlite placeholder")
            .expect("db placeholder");
        (temp_dir, mail_directory, mail_version)
    }

    #[test]
    fn default_mail_version_is_v10() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::from_parts(mail_directory, mail_version).unwrap();
        let db = cfg.envelope_db_path();
        assert!(db.ends_with("Envelope Index"));
        assert!(db.to_str().unwrap().contains("V10"));
        assert!(cfg.allowed_account_ids().is_none());
    }

    #[test]
    fn from_env_uses_env_vars() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let (temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        unsafe {
            std::env::set_var("APPLE_MAIL_DIR", &mail_directory);
            std::env::set_var("APPLE_MAIL_VERSION", "V9");
            // Set HOME to temp_dir so default_accounts_db_path points inside temp_dir
            std::env::set_var("HOME", temp_dir.path());
            // Create a dummy Accounts4.sqlite (empty file) to avoid "unable to open database file"
            let accounts_dir = temp_dir.path().join("Library").join("Accounts");
            std::fs::create_dir_all(&accounts_dir).expect("accounts dir");
            std::fs::write(accounts_dir.join("Accounts4.sqlite"), b"")
                .expect("accounts db placeholder");
        }
        // Create the Envelope Index for V9
        let v9_db_dir = mail_directory.join("V9").join("MailData");
        std::fs::create_dir_all(&v9_db_dir).expect("mail data dir");
        std::fs::write(v9_db_dir.join("Envelope Index"), b"sqlite placeholder")
            .expect("db placeholder");
        let cfg = MailConfig::from_env().unwrap();
        assert_eq!(cfg.mail_version, "V9");
        assert_eq!(cfg.mail_directory, mail_directory);
    }

    #[test]
    fn from_env_loads_without_extra_email_configuration() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let (temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        unsafe {
            std::env::set_var("APPLE_MAIL_DIR", &mail_directory);
            std::env::set_var("APPLE_MAIL_VERSION", "V10");
            // Set HOME to temp_dir so default_accounts_db_path points inside temp_dir
            std::env::set_var("HOME", temp_dir.path());
            // Create a dummy Accounts4.sqlite (empty file) to avoid "unable to open database file"
            let accounts_dir = temp_dir.path().join("Library").join("Accounts");
            std::fs::create_dir_all(&accounts_dir).expect("accounts dir");
            std::fs::write(accounts_dir.join("Accounts4.sqlite"), b"")
                .expect("accounts db placeholder");
        }

        let cfg = MailConfig::from_env().expect("config should load without extra email config");
        assert_eq!(cfg.mail_version, "V10");
        assert_eq!(cfg.mail_directory, mail_directory);
    }

    #[test]
    fn from_parts_with_accounts_enforces_allowlist_helpers() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let metadata = HashMap::from([(
            "ews://work".to_string(),
            AccountMetadata {
                account_id: "ews://work".to_string(),
                account_name: Some("Work Email".to_string()),
                email: Some("user@work.example.com".to_string()),
                username: Some("user\\work".to_string()),
                source_identifier: "work".to_string(),
                account_type: "ews".to_string(),
            },
        )]);
        let cfg = MailConfig::from_parts_with_accounts(
            mail_directory,
            mail_version,
            Some(vec!["ews://work".to_string()]),
            metadata,
        )
        .expect("config with allowlist");

        assert!(cfg.is_account_allowed("ews://work"));
        assert!(!cfg.is_account_allowed("imap://personal"));
        assert!(cfg.is_mailbox_allowed("ews://work/Inbox"));
        assert!(!cfg.is_mailbox_allowed("imap://personal/INBOX"));
        assert_eq!(
            cfg.account_metadata("ews://work")
                .and_then(|account| account.email.as_deref()),
            Some("user@work.example.com")
        );
    }

    #[test]
    fn expand_mail_directory_expands_tilde_prefix() {
        let expected = dirs::home_dir().expect("home dir").join("Library/Mail");

        assert_eq!(expand_mail_directory("~/Library/Mail"), expected);
    }

    #[test]
    fn validate_requires_non_empty_mail_version() {
        let (_temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        let error = MailConfig::from_parts(mail_directory, String::new())
            .expect_err("missing mail version should fail");
        assert!(error.to_string().contains("APPLE_MAIL_VERSION"));
    }

    #[test]
    fn parse_account_selectors_requires_non_empty_values() {
        let error = parse_account_selectors(Some(" ,  , ")).expect_err("empty selectors fail");
        assert!(error.to_string().contains("APPLE_MAIL_ACCOUNT"));
    }

    #[test]
    fn parse_account_selectors_splits_and_trims_values() {
        let selectors = parse_account_selectors(Some(
            " Work Email, user@work.example.com ,imap://personal ",
        ))
        .expect("selectors should parse");

        assert_eq!(
            selectors,
            vec![
                "Work Email",
                "user@work.example.com",
                "imap://personal"
            ]
        );
    }
}
