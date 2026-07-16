//! `list_accounts` tool implementation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::db::MailRepository;
use crate::error::MailMcpError;
use crate::mail::AttachmentStore;
use crate::server::tools::ResponseStatus;

/// Parameters for the `list_accounts` tool.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[must_use]
pub struct ListAccountsParams {
    /// Include mailboxes grouped by account (default false)
    #[serde(default)]
    pub include_mailboxes: bool,
}

/// Response for `list_accounts` tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[must_use]
pub struct ListAccountsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<AccountResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

impl ListAccountsResponse {
    /// Create a not found response with a guidance message.
    pub fn not_found(guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::NotFound),
            accounts: Vec::new(),
            total_count: Some(0),
            guidance: Some(guidance.into()),
        }
    }

    /// Create a success response with accounts.
    pub fn success(accounts: Vec<AccountResult>, total_count: u32) -> Self {
        Self {
            status: None,
            accounts,
            total_count: Some(total_count),
            guidance: None,
        }
    }
}

/// Account result item.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountResult {
    pub account_id: String,
    pub account_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub mailbox_count: i64,
    pub message_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailboxes: Option<Vec<MailboxResult>>,
}

/// Mailbox result item (reused for account grouping).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MailboxResult {
    pub name: String,
    pub url: String,
    pub message_count: i64,
}

/// Execute `list_accounts` using the repository trait.
///
/// # Errors
///
/// Returns an error if the database cannot be accessed.
#[allow(clippy::ptr_arg, clippy::needless_pass_by_value)]
pub fn list_accounts_with_conn(
    config: &MailConfig,
    repo: &dyn MailRepository,
    params: ListAccountsParams,
) -> Result<ListAccountsResponse, MailMcpError> {
    let accounts = repo
        .list_accounts()?
        .into_iter()
        .filter(|account| config.is_account_allowed(&account.account_id))
        .collect::<Vec<_>>();

    if accounts.is_empty() {
        return Ok(ListAccountsResponse::not_found(
            "No mail accounts were derived from mailbox URLs. Apple Mail may not be configured.",
        ));
    }

    // Pre-load mailboxes if requested
    let mailboxes_by_account = if params.include_mailboxes {
        let all_mailboxes = repo
            .list_mailboxes()?
            .into_iter()
            .filter(|(_, url)| config.is_mailbox_allowed(url))
            .collect::<Vec<_>>();

        let mut grouped: std::collections::BTreeMap<String, Vec<(i64, String)>> =
            std::collections::BTreeMap::new();
        for (id, url) in all_mailboxes {
            if let Some(account_id) = crate::db::mailbox_account_id(&url) {
                grouped.entry(account_id).or_default().push((id, url));
            }
        }
        grouped
    } else {
        std::collections::BTreeMap::new()
    };

    let total_count = u32::try_from(accounts.len()).unwrap_or(u32::MAX);
    let accounts: Vec<AccountResult> = accounts
        .into_iter()
        .map(|account| {
            let mailboxes = if params.include_mailboxes {
                mailboxes_by_account.get(&account.account_id).map(|mbs| {
                    mbs.iter()
                        .map(|(id, url)| MailboxResult {
                            name: crate::domain::extract_mailbox_name(url),
                            url: url.clone(),
                            message_count: repo.count_messages_in_mailbox(*id).unwrap_or(0),
                        })
                        .collect()
                })
            } else {
                None
            };

            AccountResult {
                account_name: config
                    .account_metadata(&account.account_id)
                    .and_then(|metadata| metadata.account_name.clone()),
                email: config
                    .account_metadata(&account.account_id)
                    .and_then(|metadata| metadata.email.clone()),
                account_id: account.account_id,
                account_type: account.account_type,
                mailbox_count: account.mailbox_count,
                message_count: account.message_count,
                mailboxes,
            }
        })
        .collect();
    Ok(ListAccountsResponse::success(accounts, total_count))
}

/// Execute the `list_accounts` tool.
///
/// # Errors
///
/// Returns an error if the database cannot be opened or accessed.
pub fn list_accounts(
    repo: &dyn MailRepository,
    _store: &dyn AttachmentStore,
    config: &MailConfig,
    params: ListAccountsParams,
) -> Result<ListAccountsResponse, MailMcpError> {
    list_accounts_with_conn(config, repo, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::AccountMetadata;
    use crate::db::SqliteMailRepository;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Create an in-memory test database with mailboxes and messages.
    fn make_test_repo() -> (TempDir, SqliteMailRepository) {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
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
        drop(conn);
        let repo = SqliteMailRepository::new(db_path).unwrap();
        (temp_dir, repo)
    }

    fn make_test_config() -> (TempDir, MailConfig) {
        let temp_dir = TempDir::new().expect("temp dir");
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let db_dir = mail_directory.join(&mail_version).join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");

        let config =
            MailConfig::new(mail_directory, mail_version, None, HashMap::new()).expect("config");
        (temp_dir, config)
    }

    fn default_params() -> ListAccountsParams {
        ListAccountsParams {
            include_mailboxes: false,
        }
    }

    #[test]
    fn list_accounts_with_conn_returns_accounts() {
        let (_temp_dir, repo) = make_test_repo();
        let (_temp_dir2, config) = make_test_config();
        let response = list_accounts_with_conn(&config, &repo, default_params()).unwrap();

        assert_eq!(response.status, None);
        assert_eq!(response.total_count, Some(2));
        assert_eq!(response.accounts.len(), 2);
        // Verify account IDs
        let account_ids: Vec<_> = response.accounts.iter().map(|a| &a.account_id).collect();
        assert!(account_ids.contains(&&"imap://account-a".to_string()));
        assert!(account_ids.contains(&&"ews://account-b".to_string()));
    }

    #[test]
    fn list_accounts_with_conn_filters_by_allowed_accounts() {
        let (_temp_dir, repo) = make_test_repo();
        let (temp_dir, _config) = make_test_config();
        let config = MailConfig::new(
            temp_dir.path().to_path_buf(),
            "V10".to_string(),
            Some(vec!["ews://account-b".to_string()]),
            HashMap::new(),
        )
        .expect("valid config");
        let response = list_accounts_with_conn(&config, &repo, default_params()).unwrap();

        assert_eq!(response.status, None);
        assert_eq!(response.total_count, Some(1));
        assert_eq!(response.accounts.len(), 1);
        assert_eq!(response.accounts[0].account_id, "ews://account-b");
    }

    #[test]
    fn list_accounts_with_conn_returns_not_found_when_empty() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
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
        drop(conn);
        let repo = SqliteMailRepository::new(db_path).unwrap();
        let (_temp_dir2, config) = make_test_config();
        let response = list_accounts_with_conn(&config, &repo, default_params()).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::NotFound));
        assert_eq!(response.total_count, Some(0));
        assert!(response.guidance.is_some());
    }

    #[test]
    fn list_accounts_with_conn_includes_metadata() {
        let (_temp_dir, repo) = make_test_repo();
        let mut metadata = HashMap::new();
        metadata.insert(
            "imap://account-a".to_string(),
            AccountMetadata {
                account_id: "imap://account-a".to_string(),
                account_name: Some("Personal Gmail".to_string()),
                email: Some("user@example.com".to_string()),
                username: Some("user@example.com".to_string()),
                source_identifier: "account-a".to_string(),
                account_type: "imap".to_string(),
            },
        );
        let (temp_dir, _config) = make_test_config();
        let config = MailConfig::new(
            temp_dir.path().to_path_buf(),
            "V10".to_string(),
            None,
            metadata,
        )
        .expect("valid config");
        let response = list_accounts_with_conn(&config, &repo, default_params()).unwrap();

        assert_eq!(response.status, None);
        let account_a = response
            .accounts
            .iter()
            .find(|a| a.account_id == "imap://account-a")
            .expect("account-a exists");
        assert_eq!(account_a.account_name.as_deref(), Some("Personal Gmail"));
        assert_eq!(account_a.email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn list_accounts_with_conn_includes_mailboxes_when_requested() {
        let (_temp_dir, repo) = make_test_repo();
        let (_temp_dir2, config) = make_test_config();
        let params = ListAccountsParams {
            include_mailboxes: true,
        };
        let response = list_accounts_with_conn(&config, &repo, params).unwrap();

        assert_eq!(response.status, None);
        assert_eq!(response.accounts.len(), 2);

        // Account A should have mailboxes
        let account_a = response
            .accounts
            .iter()
            .find(|a| a.account_id == "imap://account-a")
            .expect("account-a exists");
        assert!(account_a.mailboxes.is_some());
        let mailboxes = account_a.mailboxes.as_ref().unwrap();
        assert_eq!(mailboxes.len(), 1);
        assert_eq!(mailboxes[0].name, "INBOX");
    }

    #[test]
    fn list_accounts_success_has_no_guidance() {
        let (_temp_dir, repo) = make_test_repo();
        let (_temp_dir2, config) = make_test_config();
        let response = list_accounts_with_conn(&config, &repo, default_params()).unwrap();

        assert_eq!(response.status, None);
        assert!(
            response.guidance.is_none(),
            "guidance should be None on success"
        );
    }

    #[test]
    fn list_accounts_not_found_has_guidance() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (ROWID INTEGER PRIMARY KEY, mailbox INTEGER, date_sent INTEGER, date_received INTEGER, message_id TEXT, global_message_id INTEGER);
            "#,
        ).expect("seed empty schema");
        drop(conn);
        let repo = SqliteMailRepository::new(db_path).unwrap();
        let (_temp_dir2, config) = make_test_config();
        let response = list_accounts_with_conn(&config, &repo, default_params()).unwrap();

        assert!(
            response.guidance.is_some(),
            "guidance should be present on not_found"
        );
    }
}
