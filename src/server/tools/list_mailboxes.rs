//! list_mailboxes tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::Serialize;

use crate::config::MailConfig;
use crate::db::{
    count_messages_in_mailbox, list_mailboxes as db_list_mailboxes, mailbox_account_id,
    open_readonly,
};
use crate::error::MailMcpError;

/// Response for list_mailboxes tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListMailboxesResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mailboxes: Vec<MailboxResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Mailbox result item.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailboxResult {
    /// Mailbox name (human-readable)
    pub name: String,
    /// Full mailbox URL
    pub url: String,
    /// Number of messages in the mailbox
    pub message_count: i64,
    /// Account identifier derived from mailbox URL prefix
    pub account_id: Option<String>,
}

/// Execute `list_mailboxes` against an already-open SQLite connection.
pub fn list_mailboxes_with_conn(
    config: &MailConfig,
    conn: &Connection,
) -> Result<ListMailboxesResponse, MailMcpError> {
    let mailboxes = db_list_mailboxes(conn)?
        .into_iter()
        .filter(|(_, url)| config.is_mailbox_allowed(url))
        .collect::<Vec<_>>();

    if mailboxes.is_empty() {
        return Ok(ListMailboxesResponse {
            status: "not_found".to_string(),
            mailboxes: vec![],
            total_count: Some(0),
            guidance: Some("No mailboxes found. Apple Mail may not be configured.".to_string()),
        });
    }

    let results = mailboxes
        .iter()
        .map(|(id, url)| MailboxResult {
            name: url
                .rsplit('/')
                .next()
                .unwrap_or(url)
                .trim_end_matches(".mbox")
                .to_string(),
            url: url.clone(),
            message_count: count_messages_in_mailbox(conn, *id).unwrap_or(0),
            account_id: mailbox_account_id(url),
        })
        .collect::<Vec<_>>();

    Ok(ListMailboxesResponse {
        status: "success".to_string(),
        total_count: Some(results.len() as u32),
        guidance: None,
        mailboxes: results,
    })
}

/// Execute the list_mailboxes tool.
pub fn list_mailboxes(config: &MailConfig) -> Result<ListMailboxesResponse, MailMcpError> {
    let db_path = config.envelope_db_path();
    let conn = open_readonly(&db_path)?;
    list_mailboxes_with_conn(config, &conn)
}
