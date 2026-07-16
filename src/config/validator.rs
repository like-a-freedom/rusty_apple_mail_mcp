use super::MailConfig;
use crate::error::MailMcpError;

/// Validate mail configuration.
pub fn validate_config(config: &MailConfig) -> Result<(), MailMcpError> {
    if config.mail_version.trim().is_empty() {
        return Err(MailMcpError::Config(
            "APPLE_MAIL_VERSION must not be empty".to_string(),
        ));
    }

    let db_path = config.envelope_db_path();
    if !db_path.exists() {
        return Err(MailMcpError::DatabaseNotFound { path: db_path });
    }

    Ok(())
}
