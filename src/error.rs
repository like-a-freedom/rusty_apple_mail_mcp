use std::path::PathBuf;
use thiserror::Error;

/// Machine-readable error classification for the CLI JSON envelope.
///
/// Each variant maps 1:1 to a [`MailMcpError`] and tells the agent whether
/// to retry, fix the call, or give up.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// At least one search filter must be provided. Agent should fix the call.
    MissingFilter,
    /// Limit must be 1..=100. Agent should fix the call.
    BadLimit,
    /// Invalid date format. Agent should fix the call.
    BadDate,
    /// Message ID could not be parsed. Agent should fix the call.
    BadMessageId,
    /// Attachment ID format is invalid. Agent should fix the call.
    BadAttachmentId,
    /// Account filter value did not resolve or is outside Scope. Agent should fix the call.
    AccountFilterInvalid,
    /// Message not found in the index. Agent should give up or ask the user.
    MessageNotFound,
    /// Attachment not found for the given message. Agent should give up or ask the user.
    AttachmentNotFound,
    /// Envelope Index database missing. Agent should fix server config.
    DatabaseNotFound,
    /// Database locked by another process. Agent should retry after delay.
    DatabaseLocked,
    /// I/O or body-file failure. Agent should retry once.
    IoFailure,
    /// Server misconfiguration. Agent should fix server config.
    ConfigError,
    /// Unmapped internal error. Agent should report a bug.
    InternalError,
}

impl From<&MailMcpError> for ErrorKind {
    fn from(err: &MailMcpError) -> Self {
        match err {
            MailMcpError::Validation(msg) => classify_validation(msg),
            MailMcpError::MessageNotFound { .. } => Self::MessageNotFound,
            MailMcpError::AttachmentNotFound { .. } => Self::AttachmentNotFound,
            MailMcpError::DatabaseNotFound { .. } => Self::DatabaseNotFound,
            MailMcpError::DatabaseLocked(_) => Self::DatabaseLocked,
            MailMcpError::BodyFileNotFound { .. } => Self::IoFailure,
            MailMcpError::Config(_) => Self::ConfigError,
            MailMcpError::Io(_) => Self::IoFailure,
            MailMcpError::Sqlite(_) | MailMcpError::Json(_) => Self::InternalError,
        }
    }
}

fn classify_validation(msg: &str) -> ErrorKind {
    if msg.contains("At least one filter") || msg.contains("at least one filter") {
        ErrorKind::MissingFilter
    } else if msg.contains("limit must be") || msg.contains("limit must") {
        ErrorKind::BadLimit
    } else if msg.contains("Account filter") {
        ErrorKind::AccountFilterInvalid
    } else if msg.contains("date") || msg.contains("YYYY-MM-DD") {
        ErrorKind::BadDate
    } else if msg.contains("message_id") || msg.contains("message id") {
        ErrorKind::BadMessageId
    } else if msg.contains("attachment_id") || msg.contains("attachment id") {
        ErrorKind::BadAttachmentId
    } else {
        ErrorKind::InternalError
    }
}

/// JSON envelope for error responses on the CLI.
#[derive(Debug, serde::Serialize)]
pub struct CliErrorResponse {
    pub status: &'static str,
    pub error_kind: ErrorKind,
    pub message: String,
    pub guidance: String,
}

impl CliErrorResponse {
    /// Build an error envelope from a [`MailMcpError`].
    pub fn from_error(err: &MailMcpError) -> Self {
        let error_kind = ErrorKind::from(err);
        let message = err.to_string();
        let guidance = guidance_for(err, &error_kind);
        Self {
            status: "error",
            error_kind,
            message,
            guidance,
        }
    }
}

/// JSON envelope for not-found responses on the CLI.
#[derive(Debug, serde::Serialize)]
pub struct CliNotFoundResponse {
    pub status: &'static str,
    pub guidance: String,
}

impl CliNotFoundResponse {
    pub fn new(guidance: impl Into<String>) -> Self {
        Self {
            status: "not_found",
            guidance: guidance.into(),
        }
    }
}

fn guidance_for(err: &MailMcpError, kind: &ErrorKind) -> String {
    match kind {
        ErrorKind::MissingFilter => {
            "Provide at least one filter: --subject-query, --date-from, --date-to, \
             --sender, --participant, --account, or --mailbox."
                .to_string()
        }
        ErrorKind::BadLimit => "Use --limit between 1 and 100; use --offset to paginate.".to_string(),
        ErrorKind::BadDate => {
            "Use YYYY-MM-DD format for --date-from and --date-to.".to_string()
        }
        ErrorKind::BadMessageId => {
            "Use the id field from list_accounts or search_messages output.".to_string()
        }
        ErrorKind::BadAttachmentId => {
            "Use the attachment_id field from get_message output.".to_string()
        }
        ErrorKind::AccountFilterInvalid => {
            "The account filter did not match any known account or is outside the configured scope. \
             Use list_accounts to see available account names, emails, or IDs."
                .to_string()
        }
        ErrorKind::MessageNotFound => {
            format!("{err}\nUse search_messages or list_accounts to find valid message IDs.")
        }
        ErrorKind::AttachmentNotFound => {
            format!("{err}\nUse get_message to list valid attachment IDs for a message.")
        }
        ErrorKind::DatabaseNotFound => {
            "Ensure Apple Mail is configured with at least one email account, \
             or set APPLE_MAIL_DIR and APPLE_MAIL_VERSION to the correct path."
                .to_string()
        }
        ErrorKind::DatabaseLocked => {
            "Apple Mail may be running. Close Apple Mail or retry after a short delay.".to_string()
        }
        ErrorKind::IoFailure => "Retry once. If the problem persists, check file permissions.".to_string(),
        ErrorKind::ConfigError => {
            "Check APPLE_MAIL_DIR, APPLE_MAIL_VERSION, and APPLE_MAIL_ACCOUNT environment variables."
                .to_string()
        }
        ErrorKind::InternalError => "Report this as a bug — internal error.".to_string(),
    }
}

/// All recoverable errors produced by the Apple Mail MCP server.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum MailMcpError {
    #[error(
        "Envelope Index database not found at: {path}. \
         Ensure Apple Mail is configured with at least one email account, \
         or set APPLE_MAIL_DIR and APPLE_MAIL_VERSION to the correct path."
    )]
    DatabaseNotFound { path: PathBuf },

    #[error("Database is locked by another process (Apple Mail may be running): {0}")]
    DatabaseLocked(String),

    #[error("SQLite query failed: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Message {id} not found in the index")]
    MessageNotFound { id: String },

    #[error("Attachment {id} not found for message {message_id}")]
    AttachmentNotFound { id: String, message_id: String },

    #[error("Email body file not found on disk: {path}")]
    BodyFileNotFound { path: PathBuf },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Validation(String),
}

impl From<MailMcpError> for rmcp::ErrorData {
    fn from(err: MailMcpError) -> Self {
        match err {
            MailMcpError::DatabaseNotFound { path } => {
                Self::internal_error(format!("Database not found at: {}", path.display()), None)
            }
            MailMcpError::DatabaseLocked(msg) => Self::internal_error(msg, None),
            MailMcpError::Sqlite(e) => Self::internal_error(format!("SQLite error: {e}"), None),
            MailMcpError::MessageNotFound { id } => {
                Self::invalid_params(format!("Message {id} not found in the index"), None)
            }
            MailMcpError::AttachmentNotFound { id, message_id } => Self::invalid_params(
                format!("Attachment {id} not found for message {message_id}"),
                None,
            ),
            MailMcpError::BodyFileNotFound { path } => {
                Self::internal_error(format!("Body file not found at: {}", path.display()), None)
            }
            MailMcpError::Config(msg) => Self::internal_error(msg, None),
            MailMcpError::Io(e) => Self::internal_error(format!("I/O error: {e}"), None),
            MailMcpError::Json(e) => Self::internal_error(format!("JSON error: {e}"), None),
            MailMcpError::Validation(msg) => Self::invalid_params(msg, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::{ErrorData, model::ErrorCode};
    use std::io;

    fn assert_invalid_params(err: MailMcpError, expected_msg: &str) {
        let error_data: ErrorData = err.into();
        assert_eq!(error_data.code, ErrorCode::INVALID_PARAMS);
        assert!(error_data.message.contains(expected_msg));
    }

    fn assert_internal_error(err: MailMcpError, expected_msg: &str) {
        let error_data: ErrorData = err.into();
        assert_eq!(error_data.code, ErrorCode::INTERNAL_ERROR);
        assert!(error_data.message.contains(expected_msg));
    }

    #[test]
    fn error_database_not_found_maps_to_internal_error() {
        let err = MailMcpError::DatabaseNotFound {
            path: PathBuf::from("/tmp/test.db"),
        };
        assert_internal_error(err, "Database not found at: /tmp/test.db");
    }

    #[test]
    fn error_database_locked_maps_to_internal_error() {
        let err = MailMcpError::DatabaseLocked("locked by process 123".to_string());
        assert_internal_error(err, "locked by process 123");
    }

    #[test]
    fn error_sqlite_maps_to_internal_error() {
        let err = MailMcpError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("constraint failed".to_string()),
        ));
        assert_internal_error(err, "SQLite error:");
    }

    #[test]
    fn error_message_not_found_maps_to_invalid_params() {
        let err = MailMcpError::MessageNotFound {
            id: "42".to_string(),
        };
        assert_invalid_params(err, "Message 42 not found in the index");
    }

    #[test]
    fn error_attachment_not_found_maps_to_invalid_params() {
        let err = MailMcpError::AttachmentNotFound {
            id: "42:0".to_string(),
            message_id: "42".to_string(),
        };
        assert_invalid_params(err, "Attachment 42:0 not found for message 42");
    }

    #[test]
    fn error_body_file_not_found_maps_to_internal_error() {
        let err = MailMcpError::BodyFileNotFound {
            path: PathBuf::from("/tmp/body.emlx"),
        };
        assert_internal_error(err, "Body file not found at: /tmp/body.emlx");
    }

    #[test]
    fn error_config_maps_to_internal_error() {
        let err = MailMcpError::Config("invalid mail version".to_string());
        assert_internal_error(err, "invalid mail version");
    }

    #[test]
    fn error_io_maps_to_internal_error() {
        let err = MailMcpError::Io(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        assert_internal_error(err, "I/O error: file not found");
    }

    #[test]
    fn error_json_maps_to_internal_error() {
        let err = MailMcpError::Json(serde_json::Error::io(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid json",
        )));
        assert_internal_error(err, "JSON error:");
    }

    #[test]
    fn error_validation_maps_to_invalid_params() {
        let err = MailMcpError::Validation("limit must be between 1 and 100".to_string());
        assert_invalid_params(err, "limit must be between 1 and 100");
    }

    // --- ErrorKind classification tests ---

    #[test]
    fn error_kind_missing_filter() {
        let err = MailMcpError::Validation(
            "At least one filter must be provided: --subject-query, --date-from, --date-to, \
             --sender, --participant, --account, or --mailbox."
                .to_string(),
        );
        assert_eq!(ErrorKind::from(&err), ErrorKind::MissingFilter);
    }

    #[test]
    fn error_kind_bad_limit() {
        let err = MailMcpError::Validation("limit must be between 1 and 100, got 200".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::BadLimit);
    }

    #[test]
    fn error_kind_bad_date() {
        let err = MailMcpError::Validation("invalid date_from: not-a-date".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::BadDate);
    }

    #[test]
    fn error_kind_bad_message_id() {
        let err = MailMcpError::Validation("could not parse message_id: abc".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::BadMessageId);
    }

    #[test]
    fn error_kind_bad_attachment_id() {
        let err = MailMcpError::Validation("attachment_id format invalid: xyz".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::BadAttachmentId);
    }

    #[test]
    fn error_kind_message_not_found() {
        let err = MailMcpError::MessageNotFound {
            id: "99".to_string(),
        };
        assert_eq!(ErrorKind::from(&err), ErrorKind::MessageNotFound);
    }

    #[test]
    fn error_kind_attachment_not_found() {
        let err = MailMcpError::AttachmentNotFound {
            id: "99:0".to_string(),
            message_id: "99".to_string(),
        };
        assert_eq!(ErrorKind::from(&err), ErrorKind::AttachmentNotFound);
    }

    #[test]
    fn error_kind_database_not_found() {
        let err = MailMcpError::DatabaseNotFound {
            path: PathBuf::from("/tmp/db"),
        };
        assert_eq!(ErrorKind::from(&err), ErrorKind::DatabaseNotFound);
    }

    #[test]
    fn error_kind_database_locked() {
        let err = MailMcpError::DatabaseLocked("locked".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::DatabaseLocked);
    }

    #[test]
    fn error_kind_body_file_not_found_is_io_failure() {
        let err = MailMcpError::BodyFileNotFound {
            path: PathBuf::from("/tmp/body"),
        };
        assert_eq!(ErrorKind::from(&err), ErrorKind::IoFailure);
    }

    #[test]
    fn error_kind_io_is_io_failure() {
        let err = MailMcpError::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        assert_eq!(ErrorKind::from(&err), ErrorKind::IoFailure);
    }

    #[test]
    fn error_kind_config_is_config_error() {
        let err = MailMcpError::Config("bad config".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::ConfigError);
    }

    #[test]
    fn error_kind_sqlite_is_internal() {
        let err = MailMcpError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("boom".to_string()),
        ));
        assert_eq!(ErrorKind::from(&err), ErrorKind::InternalError);
    }

    #[test]
    fn error_kind_json_is_internal() {
        let err = MailMcpError::Json(serde_json::Error::io(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad",
        )));
        assert_eq!(ErrorKind::from(&err), ErrorKind::InternalError);
    }

    #[test]
    fn error_kind_unclassified_validation_is_internal() {
        let err = MailMcpError::Validation("something unexpected".to_string());
        assert_eq!(ErrorKind::from(&err), ErrorKind::InternalError);
    }

    #[test]
    fn error_kind_account_filter_invalid() {
        let err = MailMcpError::Validation(
            "Account filter \"NoSuchAccount\" did not match any known account. \
             Use list_accounts to see available account names, emails, or IDs."
                .to_string(),
        );
        assert_eq!(ErrorKind::from(&err), ErrorKind::AccountFilterInvalid);
    }

    #[test]
    fn error_kind_account_filter_excluded() {
        let err = MailMcpError::Validation(
            "Account filter \"Gmail\" resolved to imap://personal, which is excluded by APPLE_MAIL_ACCOUNT."
                .to_string(),
        );
        assert_eq!(ErrorKind::from(&err), ErrorKind::AccountFilterInvalid);
    }

    // --- CLI envelope tests ---

    #[test]
    fn cli_error_response_serializes_correctly() {
        let err = MailMcpError::Validation("limit must be between 1 and 100".to_string());
        let envelope = CliErrorResponse::from_error(&err);
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["status"], "error");
        assert_eq!(json["error_kind"], "bad_limit");
        assert!(json["message"].as_str().unwrap().contains("limit"));
        assert!(json["guidance"].as_str().unwrap().contains("--limit"));
    }

    #[test]
    fn cli_not_found_response_serializes_correctly() {
        let envelope = CliNotFoundResponse::new("No messages match".to_string());
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["status"], "not_found");
        assert!(
            json["guidance"]
                .as_str()
                .unwrap()
                .contains("No messages match")
        );
        assert!(json.get("error_kind").is_none());
    }
}
