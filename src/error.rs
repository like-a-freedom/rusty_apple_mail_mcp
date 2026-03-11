use std::path::PathBuf;
use thiserror::Error;

/// All recoverable errors produced by the Apple Mail MCP server.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum MailMcpError {
    #[error(
        "Envelope Index database not found at: {path}. Check mail_directory and mail_version config."
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_not_found_displays_useful_message() {
        let err = MailMcpError::DatabaseNotFound {
            path: "/tmp/no/such/path".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("no/such/path"),
            "should include the path: {msg}"
        );
    }
}
