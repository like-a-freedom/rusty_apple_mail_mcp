//! Account aggregation queries derived from mailbox URLs.

use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::error::MailMcpError;

use super::mailboxes::{count_messages_in_mailbox, list_mailboxes};

/// Aggregated account information derived from mailbox URL prefixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Canonical account identifier derived from the mailbox URL scheme and host.
    pub account_id: String,
    /// Transport family such as `ews` or `imap`.
    pub account_type: String,
    /// Number of mailboxes attached to this account.
    pub mailbox_count: i64,
    /// Total number of indexed messages across those mailboxes.
    pub message_count: i64,
}

/// List mailbox-derived accounts aggregated by URL prefix.
///
/// # Errors
///
/// Returns [`MailMcpError::Sqlite`] if the query fails.
pub fn list_accounts(conn: &Connection) -> Result<Vec<AccountRow>, MailMcpError> {
    let mailboxes = list_mailboxes(conn)?;
    let mut grouped: BTreeMap<String, (String, i64, i64)> = BTreeMap::new();

    for (mailbox_id, url) in mailboxes {
        if let Some(account_id) = mailbox_account_id(&url) {
            let account_type = mailbox_scheme(&url).unwrap_or_else(|| "unknown".to_string());
            let message_count = count_messages_in_mailbox(conn, mailbox_id)?;
            let entry = grouped.entry(account_id).or_insert((account_type, 0, 0));
            entry.1 += 1;
            entry.2 += message_count;
        }
    }

    Ok(grouped
        .into_iter()
        .map(
            |(account_id, (account_type, mailbox_count, message_count))| AccountRow {
                account_id,
                account_type,
                mailbox_count,
                message_count,
            },
        )
        .collect())
}

/// Derive an account identifier from a mailbox URL, for example `ews://account-id`.
#[must_use]
pub fn mailbox_account_id(mailbox_url: &str) -> Option<String> {
    let scheme_end = mailbox_url.find("://")?;
    let rest = &mailbox_url[scheme_end + 3..];
    let slash = rest.find('/')?;
    Some(format!(
        "{}://{}",
        &mailbox_url[..scheme_end],
        &rest[..slash]
    ))
}

fn mailbox_scheme(mailbox_url: &str) -> Option<String> {
    mailbox_url
        .find("://")
        .map(|index| mailbox_url[..index].to_string())
}
