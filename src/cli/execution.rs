//! CLI execution adapter: stream separation, exit codes, and presentation.
//!
//! Implements ADR-0007: agent-first CLI process contract.
//! - Success: compact JSON on stdout, exit 0
//! - Failure: empty stdout, structured error on last stderr line, exit non-zero
//! - Tracing/diagnostics on stderr only

use std::io::Write;
use std::process::ExitCode;

use crate::error::{CliErrorResponse, MailMcpError};

/// Exit codes per ADR-0007.
pub mod exit_code {
    /// Command executed successfully (including empty search/list).
    pub const SUCCESS: u8 = 0;
    /// Internal or unclassified failure.
    pub const INTERNAL: u8 = 1;
    /// Usage or invalid input (aligned with clap).
    pub const USAGE: u8 = 2;
    /// Requested message or attachment not found.
    pub const NOT_FOUND: u8 = 3;
    /// Temporarily unavailable or retryable failure.
    pub const RETRYABLE: u8 = 4;
    /// Configuration, environment, or Scope failure.
    pub const CONFIG: u8 = 5;
}

impl From<&MailMcpError> for u8 {
    fn from(err: &MailMcpError) -> Self {
        match err {
            MailMcpError::Validation(_) => exit_code::USAGE,
            MailMcpError::MessageNotFound { .. } | MailMcpError::AttachmentNotFound { .. } => {
                exit_code::NOT_FOUND
            }
            MailMcpError::DatabaseLocked(_) => exit_code::RETRYABLE,
            MailMcpError::DatabaseNotFound { .. }
            | MailMcpError::Config(_)
            | MailMcpError::BodyFileNotFound { .. } => exit_code::CONFIG,
            MailMcpError::Io(_) | MailMcpError::Sqlite(_) | MailMcpError::Json(_) => {
                exit_code::INTERNAL
            }
        }
    }
}

/// Write a successful JSON response to stdout.
pub fn write_success<T: serde::Serialize>(value: &T, pretty: bool) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    if pretty {
        serde_json::to_writer_pretty(&mut stdout, value)?;
    } else {
        serde_json::to_writer(&mut stdout, value)?;
    }
    writeln!(stdout)?;
    Ok(())
}

/// Write a structured error to stderr and return the exit code.
pub fn write_error(err: &MailMcpError) -> ExitCode {
    let envelope = CliErrorResponse::from_error(err);
    let mut stderr = std::io::stderr();
    let _ = serde_json::to_writer(&mut stderr, &envelope);
    let _ = writeln!(stderr);
    ExitCode::from(u8::from(err))
}

/// Execute a CLI command handler and handle the result according to ADR-0007.
pub fn execute<F, T>(handler: F, pretty: bool) -> u8
where
    F: FnOnce() -> Result<T, MailMcpError>,
    T: serde::Serialize,
{
    match handler() {
        Ok(value) => {
            if let Err(io_err) = write_success(&value, pretty) {
                let err = MailMcpError::Io(io_err);
                write_error(&err);
                return exit_code::INTERNAL;
            }
            exit_code::SUCCESS
        }
        Err(err) => {
            write_error(&err);
            u8::from(&err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_mapping_validation_is_usage() {
        let err = MailMcpError::Validation("bad".to_string());
        assert_eq!(u8::from(&err), exit_code::USAGE);
    }

    #[test]
    fn exit_code_mapping_not_found() {
        let err = MailMcpError::MessageNotFound {
            id: "1".to_string(),
        };
        assert_eq!(u8::from(&err), exit_code::NOT_FOUND);
    }

    #[test]
    fn exit_code_mapping_attachment_not_found() {
        let err = MailMcpError::AttachmentNotFound {
            id: "1:0".to_string(),
            message_id: "1".to_string(),
        };
        assert_eq!(u8::from(&err), exit_code::NOT_FOUND);
    }

    #[test]
    fn exit_code_mapping_locked_is_retryable() {
        let err = MailMcpError::DatabaseLocked("locked".to_string());
        assert_eq!(u8::from(&err), exit_code::RETRYABLE);
    }

    #[test]
    fn exit_code_mapping_db_not_found_is_config() {
        let err = MailMcpError::DatabaseNotFound {
            path: "/tmp/db".into(),
        };
        assert_eq!(u8::from(&err), exit_code::CONFIG);
    }

    #[test]
    fn exit_code_mapping_config_is_config() {
        let err = MailMcpError::Config("bad".to_string());
        assert_eq!(u8::from(&err), exit_code::CONFIG);
    }

    #[test]
    fn exit_code_mapping_io_is_internal() {
        let err = MailMcpError::Io(std::io::Error::other("fail"));
        assert_eq!(u8::from(&err), exit_code::INTERNAL);
    }

    #[test]
    fn cli_error_response_serialization() {
        let err = MailMcpError::Validation("limit must be between 1 and 100, got 200".to_string());
        let envelope = CliErrorResponse::from_error(&err);
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["status"], "error");
        assert_eq!(json["error_kind"], "bad_limit");
    }
}
