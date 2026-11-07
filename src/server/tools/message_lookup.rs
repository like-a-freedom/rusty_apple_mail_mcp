//! Shared message lookup helpers for read-only tool handlers.

use std::path::PathBuf;

use crate::config::MailConfig;
use crate::db::MailRepository;
use crate::db::MessageRow;
use crate::error::MailMcpError;
use crate::mail::EmlxLocator;

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
    repo: &dyn MailRepository,
    message_id: i64,
) -> Result<AccessibleMessage, MailMcpError> {
    let Some(row) = repo.get_message(message_id)? else {
        return Ok(AccessibleMessage::NotFound);
    };

    // Fail closed: with an allowlist active, a message without a mailbox URL
    // cannot be attributed to an allowed account and must stay blocked.
    let in_scope = row
        .mailbox_url
        .as_deref()
        .map(|mailbox_url| config.is_mailbox_allowed(mailbox_url))
        .unwrap_or_else(|| config.allowed_account_ids().is_none());

    if !in_scope {
        return Ok(AccessibleMessage::BlockedAccount);
    }

    Ok(AccessibleMessage::Found(row))
}

/// Resolve the on-disk `.emlx` path for a message row using all known hints.
#[must_use]
pub(crate) fn locate_message_file(
    locator: &EmlxLocator<'_>,
    config: &MailConfig,
    row: &MessageRow,
) -> Option<PathBuf> {
    let mut numeric_hints = vec![row.rowid.to_string()];
    if let Some(global_message_id) = row.global_message_id {
        numeric_hints.push(global_message_id.to_string());
    }
    if let Some(message_id) = row.message_id.as_ref() {
        numeric_hints.push(message_id.clone());
    }
    numeric_hints.sort();
    numeric_hints.dedup();

    locator.locate_emlx_with_hints(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FakeMailRepository;
    use std::{collections::HashMap, fs};
    use tempfile::TempDir;

    fn make_test_config(
        temp_dir: &TempDir,
        allowed_account_ids: Option<Vec<String>>,
    ) -> MailConfig {
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let db_dir = mail_directory.join(&mail_version).join("MailData");
        fs::create_dir_all(&db_dir).expect("mail data dir");
        fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");
        MailConfig::new(
            mail_directory,
            mail_version,
            allowed_account_ids,
            HashMap::new(),
            None,
        )
        .expect("valid config")
    }

    fn repo_with_message(mailbox_url: Option<String>) -> FakeMailRepository {
        let repo = FakeMailRepository::default();
        repo.messages.lock().expect("messages lock").insert(
            1,
            crate::db::MessageRow {
                rowid: 1,
                subject: Some("Test Subject".to_string()),
                sender: Some("sender@example.com".to_string()),
                mailbox_url,
                date_sent: Some(0),
                date_received: Some(0),
                message_id: Some("<msg1@mail>".to_string()),
                global_message_id: Some(10),
                message_id_header: Some("<msg1@mail>".to_string()),
            },
        );
        repo
    }

    #[test]
    fn load_accessible_message_blocks_null_mailbox_url_when_scope_set() {
        let temp_dir = TempDir::new().expect("temp dir");
        let config = make_test_config(&temp_dir, Some(vec!["ews://work".to_string()]));
        let repo = repo_with_message(None);

        let result = load_accessible_message(&config, &repo, 1).expect("no db error");

        assert!(
            matches!(result, AccessibleMessage::BlockedAccount),
            "messages without a mailbox must be blocked while an allowlist is active"
        );
    }

    #[test]
    fn load_accessible_message_blocks_unparseable_mailbox_url_when_scope_set() {
        let temp_dir = TempDir::new().expect("temp dir");
        let config = make_test_config(&temp_dir, Some(vec!["ews://work".to_string()]));
        let repo = repo_with_message(Some("no-scheme".to_string()));

        let result = load_accessible_message(&config, &repo, 1).expect("no db error");

        assert!(matches!(result, AccessibleMessage::BlockedAccount));
    }

    #[test]
    fn load_accessible_message_allows_null_mailbox_url_without_scope() {
        let temp_dir = TempDir::new().expect("temp dir");
        let config = make_test_config(&temp_dir, None);
        let repo = repo_with_message(None);

        let result = load_accessible_message(&config, &repo, 1).expect("no db error");

        assert!(matches!(result, AccessibleMessage::Found(_)));
    }

    #[test]
    fn load_accessible_message_allows_in_scope_mailbox_url() {
        let temp_dir = TempDir::new().expect("temp dir");
        let config = make_test_config(&temp_dir, Some(vec!["ews://work".to_string()]));
        let repo = repo_with_message(Some("ews://work/INBOX".to_string()));

        let result = load_accessible_message(&config, &repo, 1).expect("no db error");

        assert!(matches!(result, AccessibleMessage::Found(_)));
    }
}
