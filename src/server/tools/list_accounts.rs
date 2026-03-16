//! list_accounts tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::Serialize;

use crate::config::MailConfig;
use crate::db::{list_accounts as db_list_accounts, open_readonly};
use crate::error::MailMcpError;

/// Response for list_accounts tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListAccountsResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<AccountResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Account result item.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountResult {
    pub account_id: String,
    pub account_type: String,
    pub mailbox_count: i64,
    pub message_count: i64,
}

/// Execute `list_accounts` against an already-open SQLite connection.
pub fn list_accounts_with_conn(conn: &Connection) -> Result<ListAccountsResponse, MailMcpError> {
    let accounts = db_list_accounts(conn)?;

    if accounts.is_empty() {
        return Ok(ListAccountsResponse {
            status: "not_found".to_string(),
            accounts: vec![],
            total_count: Some(0),
            guidance: Some(
                "No mail accounts were derived from mailbox URLs. Apple Mail may not be configured."
                    .to_string(),
            ),
        });
    }

    Ok(ListAccountsResponse {
        status: "success".to_string(),
        total_count: Some(accounts.len() as u32),
        guidance: Some(
            "Use account_id as the `account` filter in search_messages to restrict results to a single account."
                .to_string(),
        ),
        accounts: accounts
            .into_iter()
            .map(|account| AccountResult {
                account_id: account.account_id,
                account_type: account.account_type,
                mailbox_count: account.mailbox_count,
                message_count: account.message_count,
            })
            .collect(),
    })
}

/// Execute the list_accounts tool.
pub fn list_accounts(config: &MailConfig) -> Result<ListAccountsResponse, MailMcpError> {
    let db_path = config.envelope_db_path();
    let conn = open_readonly(&db_path)?;
    list_accounts_with_conn(&conn)
}
