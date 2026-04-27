//! Shared message lookup helpers for read-only tool handlers.

use std::path::PathBuf;

use rusqlite::Connection;

use crate::config::MailConfig;
use crate::db::{MessageRow, get_message_by_id};
use crate::error::MailMcpError;
use crate::mail::locate_emlx_with_hints;

/// Result of resolving a message row with account visibility checks applied.
pub(crate) enum AccessibleMessage {
    Found(MessageRow),
    NotFound,
    BlockedAccount,
}

/// Load a message row and reject messages that belong to filtered accounts.
///
/// # Errors
///
/// Returns an error if the database cannot be accessed.
pub(crate) fn load_accessible_message(
    config: &MailConfig,
    conn: &Connection,
    message_id: i64,
) -> Result<AccessibleMessage, MailMcpError> {
    let Some(row) = get_message_by_id(conn, message_id)? else {
        return Ok(AccessibleMessage::NotFound);
    };

    if let Some(mailbox_url) = row.mailbox_url.as_deref()
        && !config.is_mailbox_allowed(mailbox_url)
    {
        return Ok(AccessibleMessage::BlockedAccount);
    }

    Ok(AccessibleMessage::Found(row))
}

/// Resolve the on-disk `.emlx` path for a message row using all known hints.
#[must_use]
pub(crate) fn locate_message_file(config: &MailConfig, row: &MessageRow) -> Option<PathBuf> {
    let mut numeric_hints = vec![row.rowid.to_string()];
    if let Some(global_message_id) = row.global_message_id {
        numeric_hints.push(global_message_id.to_string());
    }
    if let Some(message_id) = row.message_id.as_ref() {
        numeric_hints.push(message_id.clone());
    }
    numeric_hints.sort();
    numeric_hints.dedup();

    locate_emlx_with_hints(
        &config.mail_directory,
        &config.mail_version,
        row.mailbox_url.as_deref().unwrap_or(""),
        row.rowid,
        &numeric_hints,
        row.message_id_header
            .as_deref()
            .or(row.message_id.as_deref()),
    )
}
