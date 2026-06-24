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


