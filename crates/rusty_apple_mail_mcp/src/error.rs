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

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
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

    #[test]
    fn database_locked_displays_useful_message() {
        let err = MailMcpError::DatabaseLocked("database is locked".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Database is locked"));
        assert!(msg.contains("database is locked"));
    }

    #[test]
    fn message_not_found_displays_id() {
        let err = MailMcpError::MessageNotFound {
            id: "12345".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Message 12345 not found"));
    }

    #[test]
    fn attachment_not_found_displays_ids() {
        let err = MailMcpError::AttachmentNotFound {
            id: "att-1".to_string(),
            message_id: "msg-1".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Attachment att-1 not found"));
        assert!(msg.contains("msg-1"));
    }

    #[test]
    fn body_file_not_found_displays_path() {
        let err = MailMcpError::BodyFileNotFound {
            path: "/path/to/message.emlx".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Email body file not found"));
        assert!(msg.contains("/path/to/message.emlx"));
    }

    #[test]
    fn config_error_displays_message() {
        let err = MailMcpError::Config("invalid mail version".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Configuration error"));
        assert!(msg.contains("invalid mail version"));
    }

    #[test]
    fn sqlite_error_wraps() {
        let sqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some("SQL logic error".to_string()),
        );
        let err = MailMcpError::Sqlite(sqlite_err);
        let msg = err.to_string();
        assert!(msg.contains("SQLite query failed"));
    }

    #[test]
    fn io_error_wraps() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = MailMcpError::Io(io_err);
        let msg = err.to_string();
        assert!(msg.contains("file not found"));
    }
}
