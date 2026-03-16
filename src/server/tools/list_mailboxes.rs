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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Create an in-memory test database with mailboxes and messages.
    fn make_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (
                ROWID INTEGER PRIMARY KEY,
                mailbox INTEGER REFERENCES mailboxes,
                date_sent INTEGER,
                date_received INTEGER,
                message_id TEXT,
                global_message_id INTEGER
            );
            INSERT INTO mailboxes VALUES
                (1, 'imap://account-a/INBOX'),
                (2, 'ews://account-b/Inbox');
            INSERT INTO messages VALUES
                (1, 1, 0, 0, 'msg1', NULL),
                (2, 1, 0, 0, 'msg2', NULL),
                (3, 2, 0, 0, 'msg3', NULL);
            "#,
        )
        .expect("seed test schema");
        conn
    }

    fn make_test_config() -> (TempDir, MailConfig) {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let db_dir = mail_directory.join(&mail_version).join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");

        let config = MailConfig::from_parts_with_accounts(
            mail_directory,
            mail_version,
            None,
            HashMap::new(),
        )
        .expect("config");
        (temp_dir, config)
    }

    #[test]
    fn list_mailboxes_with_conn_returns_mailboxes() {
        let conn = make_test_db();
        let (_temp_dir, config) = make_test_config();
        let response = list_mailboxes_with_conn(&config, &conn).unwrap();

        assert_eq!(response.status, "success");
        assert_eq!(response.total_count, Some(2));
        assert_eq!(response.mailboxes.len(), 2);
        // Verify mailbox names
        let names: Vec<_> = response.mailboxes.iter().map(|m| &m.name).collect();
        assert!(names.contains(&&"INBOX".to_string()));
        assert!(names.contains(&&"Inbox".to_string()));
    }

    #[test]
    fn list_mailboxes_with_conn_filters_by_allowed_accounts() {
        let conn = make_test_db();
        let (temp_dir, _config) = make_test_config();
        let config = MailConfig::from_parts_with_accounts(
            temp_dir.path().to_path_buf(),
            "V10".to_string(),
            Some(vec!["ews://account-b".to_string()]),
            HashMap::new(),
        )
        .expect("valid config");
        let response = list_mailboxes_with_conn(&config, &conn).unwrap();

        assert_eq!(response.status, "success");
        assert_eq!(response.total_count, Some(1));
        assert_eq!(response.mailboxes.len(), 1);
        assert_eq!(response.mailboxes[0].name, "Inbox");
        assert_eq!(
            response.mailboxes[0].account_id.as_deref(),
            Some("ews://account-b")
        );
    }

    #[test]
    fn list_mailboxes_with_conn_returns_not_found_when_empty() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (
                ROWID INTEGER PRIMARY KEY,
                mailbox INTEGER REFERENCES mailboxes,
                date_sent INTEGER,
                date_received INTEGER,
                message_id TEXT,
                global_message_id INTEGER
            );
            "#,
        )
        .expect("seed empty schema");
        let (_temp_dir, config) = make_test_config();
        let response = list_mailboxes_with_conn(&config, &conn).unwrap();

        assert_eq!(response.status, "not_found");
        assert_eq!(response.total_count, Some(0));
        assert!(response.guidance.is_some());
    }

    #[test]
    fn list_mailboxes_with_conn_counts_messages() {
        let conn = make_test_db();
        let (_temp_dir, config) = make_test_config();
        let response = list_mailboxes_with_conn(&config, &conn).unwrap();

        assert_eq!(response.status, "success");
        let inbox = response
            .mailboxes
            .iter()
            .find(|m| m.name == "INBOX")
            .expect("INBOX exists");
        assert_eq!(inbox.message_count, 2);
        let ews_inbox = response
            .mailboxes
            .iter()
            .find(|m| m.name == "Inbox")
            .expect("EWS Inbox exists");
        assert_eq!(ews_inbox.message_count, 1);
    }
}
