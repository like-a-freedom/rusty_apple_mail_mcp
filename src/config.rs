use std::path::PathBuf;

use crate::error::MailMcpError;

const DEFAULT_MAIL_VERSION: &str = "V10";

/// Server configuration. Loaded strictly from environment variables.
#[derive(Debug, Clone)]
pub struct MailConfig {
    pub mail_directory: PathBuf,
    pub mail_version: String,
    pub primary_email: String,
}

impl MailConfig {
    /// Resolve configuration from environment variables, falling back to defaults.
    /// `APPLE_MAIL_DIR`, `APPLE_MAIL_VERSION`, `APPLE_MAIL_PRIMARY_EMAIL`.
    pub fn from_env() -> Result<Self, MailMcpError> {
        let mail_directory = std::env::var("APPLE_MAIL_DIR")
            .map(|raw| expand_mail_directory(&raw))
            .unwrap_or_else(|_| default_mail_directory());
        let mail_version = std::env::var("APPLE_MAIL_VERSION")
            .unwrap_or_else(|_| DEFAULT_MAIL_VERSION.to_string());
        let primary_email = std::env::var("APPLE_MAIL_PRIMARY_EMAIL").unwrap_or_default();

        Self::from_parts(mail_directory, mail_version, primary_email)
    }

    /// Build a configuration from already-resolved values and validate it.
    pub fn from_parts(
        mail_directory: PathBuf,
        mail_version: String,
        primary_email: String,
    ) -> Result<Self, MailMcpError> {
        let config = Self {
            mail_directory,
            mail_version,
            primary_email,
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

    /// Validate the configuration for env-only stdio startup.
    pub fn validate(&self) -> Result<(), MailMcpError> {
        if self.mail_version.trim().is_empty() {
            return Err(MailMcpError::Config(
                "APPLE_MAIL_VERSION must not be empty".to_string(),
            ));
        }

        if self.primary_email.trim().is_empty() {
            return Err(MailMcpError::Config(
                "APPLE_MAIL_PRIMARY_EMAIL is required for startup".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn make_valid_config_inputs() -> (TempDir, PathBuf, String, String) {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let primary_email = "test@example.com".to_string();
        let db_path = mail_directory.join(&mail_version).join("MailData");
        std::fs::create_dir_all(&db_path).expect("mail data dir");
        std::fs::write(db_path.join("Envelope Index"), b"sqlite placeholder")
            .expect("db placeholder");
        (temp_dir, mail_directory, mail_version, primary_email)
    }

    #[test]
    fn default_mail_version_is_v10() {
        let (_temp_dir, mail_directory, mail_version, primary_email) = make_valid_config_inputs();
        let cfg = MailConfig::from_parts(mail_directory, mail_version, primary_email).unwrap();
        let db = cfg.envelope_db_path();
        assert!(db.ends_with("Envelope Index"));
        assert!(db.to_str().unwrap().contains("V10"));
    }

    #[test]
    fn from_env_uses_env_vars() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let (_temp_dir, mail_directory, _mail_version, _primary_email) = make_valid_config_inputs();
        unsafe {
            std::env::set_var("APPLE_MAIL_DIR", &mail_directory);
            std::env::set_var("APPLE_MAIL_VERSION", "V9");
            std::env::set_var("APPLE_MAIL_PRIMARY_EMAIL", "user@corp.com");
            let db_dir = mail_directory.join("V9").join("MailData");
            std::fs::create_dir_all(&db_dir).expect("mail data dir");
            std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder")
                .expect("db placeholder");
        }
        let cfg = MailConfig::from_env().unwrap();
        assert_eq!(cfg.mail_version, "V9");
        assert_eq!(cfg.primary_email, "user@corp.com");
    }

    #[test]
    fn expand_mail_directory_expands_tilde_prefix() {
        let expected = dirs::home_dir().expect("home dir").join("Library/Mail");

        assert_eq!(expand_mail_directory("~/Library/Mail"), expected);
    }

    #[test]
    fn validate_requires_primary_email() {
        let (_temp_dir, mail_directory, mail_version, _primary_email) = make_valid_config_inputs();
        let error = MailConfig::from_parts(mail_directory, mail_version, String::new())
            .expect_err("missing email should fail");
        assert!(error.to_string().contains("APPLE_MAIL_PRIMARY_EMAIL"));
    }
}
