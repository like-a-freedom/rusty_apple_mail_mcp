mod accounts;
mod builder;
mod parser;
mod paths;
mod validator;

pub use accounts::load_account_metadata_for_selectors;
pub use builder::MailConfigBuilder;
pub use parser::{MailConfigOverrides, parse_account_selectors};
pub use paths::{
    default_mail_directory, envelope_db_path, expand_mail_directory, normalize_mail_directory,
};
pub use validator::validate_config;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::accounts::AccountMetadata;
use crate::db::mailbox_account_id;
use crate::error::MailMcpError;

/// Server configuration for Apple Mail access.
#[derive(Debug, Clone)]
pub struct MailConfig {
    pub mail_directory: PathBuf,
    pub mail_version: String,
    pub allowed_account_ids: Option<Vec<String>>,
    pub account_metadata: HashMap<String, AccountMetadata>,
}

impl MailConfig {
    pub fn new(
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
        validate_config(&config)?;
        Ok(config)
    }

    pub fn envelope_db_path(&self) -> PathBuf {
        envelope_db_path(&self.mail_directory, &self.mail_version)
    }

    pub fn allowed_account_ids(&self) -> Option<&[String]> {
        self.allowed_account_ids.as_deref()
    }

    pub fn is_account_allowed(&self, account_id: &str) -> bool {
        self.allowed_account_ids
            .as_ref()
            .is_none_or(|allowed| allowed.iter().any(|candidate| candidate == account_id))
    }

    pub fn is_mailbox_allowed(&self, mailbox_url: &str) -> bool {
        mailbox_account_id(mailbox_url)
            .as_deref()
            .is_none_or(|account_id| self.is_account_allowed(account_id))
    }

    pub fn account_metadata(&self, account_id: &str) -> Option<&AccountMetadata> {
        self.account_metadata.get(account_id)
    }
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
        let cfg = MailConfig::new(mail_directory, mail_version, None, HashMap::new()).unwrap();
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
            std::env::set_var("HOME", temp_dir.path());
            let accounts_dir = temp_dir.path().join("Library").join("Accounts");
            std::fs::create_dir_all(&accounts_dir).expect("accounts dir");
            std::fs::write(accounts_dir.join("Accounts4.sqlite"), b"")
                .expect("accounts db placeholder");
        }
        let v9_db_dir = mail_directory.join("V9").join("MailData");
        std::fs::create_dir_all(&v9_db_dir).expect("mail data dir");
        std::fs::write(v9_db_dir.join("Envelope Index"), b"sqlite placeholder")
            .expect("db placeholder");
        let cfg = MailConfigBuilder::new().build().unwrap();
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
            std::env::set_var("HOME", temp_dir.path());
            let accounts_dir = temp_dir.path().join("Library").join("Accounts");
            std::fs::create_dir_all(&accounts_dir).expect("accounts dir");
            std::fs::write(accounts_dir.join("Accounts4.sqlite"), b"")
                .expect("accounts db placeholder");
        }

        let cfg = MailConfigBuilder::new()
            .build()
            .expect("config should load without extra email config");
        assert_eq!(cfg.mail_version, "V10");
        assert_eq!(cfg.mail_directory, mail_directory);
    }

    #[test]
    fn from_overrides_prefers_explicit_values_over_environment() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let (env_temp_dir, env_mail_directory, _mail_version) = make_valid_config_inputs();
        let (_override_temp_dir, override_mail_directory, override_mail_version) =
            make_valid_config_inputs();

        unsafe {
            std::env::set_var("APPLE_MAIL_DIR", &env_mail_directory);
            std::env::set_var("APPLE_MAIL_VERSION", "V9");
            std::env::set_var("HOME", env_temp_dir.path());

            let accounts_dir = env_temp_dir.path().join("Library").join("Accounts");
            std::fs::create_dir_all(&accounts_dir).expect("accounts dir");
            std::fs::write(accounts_dir.join("Accounts4.sqlite"), b"")
                .expect("accounts db placeholder");
        }

        let override_config = MailConfigBuilder::from_overrides(MailConfigOverrides {
            mail_directory: Some(override_mail_directory.clone()),
            mail_version: Some(override_mail_version.clone()),
            account: None,
        })
        .expect("overrides should win");

        assert_eq!(override_config.mail_directory, override_mail_directory);
        assert_eq!(override_config.mail_version, override_mail_version);
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
        let cfg = MailConfig::new(
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
    fn validate_requires_non_empty_mail_version() {
        let (_temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        let error = MailConfig::new(mail_directory, String::new(), None, HashMap::new())
            .expect_err("missing mail version should fail");
        assert!(error.to_string().contains("APPLE_MAIL_VERSION"));
    }

    #[test]
    fn validate_passes_with_valid_config() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg =
            MailConfig::new(mail_directory.clone(), mail_version, None, HashMap::new()).unwrap();
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn validate_fails_when_db_missing() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let error = MailConfig::new(mail_directory, "V10".to_string(), None, HashMap::new())
            .expect_err("missing db should fail");
        assert!(
            error.to_string().contains("not found") || error.to_string().contains("Envelope Index")
        );
    }

    #[test]
    fn validate_fails_on_whitespace_only_mail_version() {
        let (_temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        let error = MailConfig::new(mail_directory, "   ".to_string(), None, HashMap::new())
            .expect_err("whitespace version should fail");
        assert!(error.to_string().contains("APPLE_MAIL_VERSION"));
    }

    #[test]
    fn is_account_allowed_none_means_all_allowed() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(mail_directory, mail_version, None, HashMap::new()).unwrap();
        assert!(cfg.allowed_account_ids().is_none());
        assert!(cfg.is_account_allowed("any-account"));
    }

    #[test]
    fn is_account_allowed_some_restricts_to_list() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(
            mail_directory,
            mail_version,
            Some(vec!["account1".to_string(), "account2".to_string()]),
            HashMap::new(),
        )
        .unwrap();
        assert!(cfg.is_account_allowed("account1"));
        assert!(cfg.is_account_allowed("account2"));
        assert!(!cfg.is_account_allowed("account3"));
        assert!(!cfg.is_account_allowed("unknown"));
    }

    #[test]
    fn is_mailbox_allowed_none_means_all_allowed() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(mail_directory, mail_version, None, HashMap::new()).unwrap();
        assert!(cfg.is_mailbox_allowed("imap://any/INBOX"));
        assert!(cfg.is_mailbox_allowed("ews://any/Inbox"));
    }

    #[test]
    fn is_mailbox_allowed_filters_by_allowed_accounts() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(
            mail_directory,
            mail_version,
            Some(vec!["ews://work".to_string()]),
            HashMap::new(),
        )
        .unwrap();
        assert!(cfg.is_mailbox_allowed("ews://work/Inbox"));
        assert!(cfg.is_mailbox_allowed("ews://work/Sent"));
        assert!(!cfg.is_mailbox_allowed("imap://personal/INBOX"));
    }

    #[test]
    fn account_metadata_returns_none_for_unknown() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(mail_directory, mail_version, None, HashMap::new()).unwrap();
        assert!(cfg.account_metadata("unknown").is_none());
    }

    #[test]
    fn account_metadata_returns_some_for_known() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let metadata = HashMap::from([(
            "test-account".to_string(),
            AccountMetadata {
                account_id: "test-account".to_string(),
                account_name: Some("Test".to_string()),
                email: Some("test@test.com".to_string()),
                username: Some("test".to_string()),
                source_identifier: "test".to_string(),
                account_type: "test".to_string(),
            },
        )]);
        let cfg = MailConfig::new(mail_directory, mail_version, None, metadata).unwrap();

        let meta = cfg.account_metadata("test-account");
        assert!(meta.is_some());
        assert_eq!(meta.unwrap().email.as_deref(), Some("test@test.com"));
    }

    #[test]
    fn envelope_db_path_constructs_correct_path() {
        let (_temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(
            mail_directory.clone(),
            "V10".to_string(),
            None,
            HashMap::new(),
        )
        .unwrap();
        let db_path = cfg.envelope_db_path();
        assert!(db_path.to_string_lossy().contains("V10"));
        assert!(db_path.to_string_lossy().contains("MailData"));
        assert!(db_path.to_string_lossy().contains("Envelope Index"));
    }

    #[test]
    fn from_parts_fails_on_empty_version() {
        let (_temp_dir, mail_directory, _mail_version) = make_valid_config_inputs();
        let error = MailConfig::new(mail_directory, "".to_string(), None, HashMap::new())
            .expect_err("empty version fails");
        assert!(error.to_string().contains("APPLE_MAIL_VERSION"));
    }

    #[test]
    fn from_parts_creates_config_without_accounts() {
        let (_temp_dir, mail_directory, mail_version) = make_valid_config_inputs();
        let cfg = MailConfig::new(mail_directory, mail_version, None, HashMap::new()).unwrap();
        assert_eq!(cfg.allowed_account_ids(), None);
        assert!(cfg.account_metadata("any").is_none());
    }
}
