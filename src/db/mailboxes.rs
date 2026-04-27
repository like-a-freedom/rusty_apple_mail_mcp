//! Mailbox listing and counting queries.

use rusqlite::{Connection, params};

use crate::error::MailMcpError;

/// List all mailboxes with their row ID and URL.
///
/// # Errors
///
/// Returns [`MailMcpError::Sqlite`] if the query fails.
pub fn list_mailboxes(conn: &Connection) -> Result<Vec<(i64, String)>, MailMcpError> {
    let mut stmt = conn.prepare("SELECT ROWID, url FROM mailboxes ORDER BY url")?;

    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// Count messages in a mailbox.
///
/// # Errors
///
/// Returns [`MailMcpError::Sqlite`] if the query fails.
pub fn count_messages_in_mailbox(conn: &Connection, mailbox_id: i64) -> Result<i64, MailMcpError> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM messages WHERE mailbox = ?")?;
    let count: i64 = stmt.query_row(params![mailbox_id], |row| row.get(0))?;
    Ok(count)
}
