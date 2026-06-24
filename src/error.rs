use std::path::PathBuf;
use thiserror::Error;

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
}


