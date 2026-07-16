//! `get_message` tool implementation.

use lru::LruCache;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::MailConfig;
use crate::db::MailRepository;
use crate::domain::AttachmentMeta;
use crate::error::MailMcpError;
use crate::mail::AttachmentStore;
use crate::mail::{parse_emlx_without_attachment_content, raw_attachments_to_meta};
use crate::server::tools::ResponseStatus;
use crate::server::tools::message_lookup::{
    AccessibleMessage, load_accessible_message, locate_message_file,
};

/// LRU cache for parsed .emlx bodies keyed by resolved path.
static BODY_CACHE: LazyLock<Mutex<LruCache<std::path::PathBuf, CachedMessage>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(NonZeroUsize::new(256).expect("cache size"))));

#[derive(Clone)]
struct CachedMessage {
    body_text: Option<String>,
    body_html: Option<String>,
    attachments: Vec<AttachmentMeta>,
}

/// Parameters for the `get_message` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetMessageParams {
    /// Stable message identifier (from search results)
    pub message_id: String,
    /// Include message body (default true)
    #[serde(default = "default_true")]
    pub include_body: bool,
    /// Include attachment list (default true)
    #[serde(default = "default_true")]
    pub include_attachments_summary: bool,
    /// Include To/CC recipients lists (default false).
    /// Enable when you need to check who received the message.
    #[serde(default)]
    pub include_recipients: bool,
}

fn default_true() -> bool {
    true
}

/// Response for `get_message` tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[must_use]
pub struct GetMessageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<GetMessageResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

impl GetMessageResponse {
    /// Create an error response with a guidance message.
    pub fn error(guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::Error),
            message: None,
            guidance: Some(guidance.into()),
        }
    }

    /// Create a not found response with a guidance message.
    pub fn not_found(guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::NotFound),
            message: None,
            guidance: Some(guidance.into()),
        }
    }

    /// Create a partial response with a result and guidance.
    pub fn partial(result: GetMessageResult, guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::Partial),
            message: Some(result),
            guidance: Some(guidance.into()),
        }
    }

    /// Create a success response with a result.
    pub fn success(result: GetMessageResult) -> Self {
        Self {
            status: None,
            message: Some(result),
            guidance: None,
        }
    }
}

/// Message result in `get_message` response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetMessageResult {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id_header: Option<String>,
    pub subject: String,
    pub from: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    pub date_sent: Option<String>,
    pub date_received: Option<String>,
    pub body: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
}

/// Execute `get_message` using the repository trait.
///
/// # Errors
///
/// Returns an error if the database cannot be accessed or the message file cannot be parsed.
///
/// # Panics
///
/// May panic if the internal LRU cache lock cannot be acquired.
#[allow(clippy::too_many_lines)]
#[allow(clippy::ptr_arg, clippy::needless_pass_by_value)]
pub fn get_message_with_conn(
    config: &MailConfig,
    repo: &dyn MailRepository,
    params: GetMessageParams,
) -> Result<GetMessageResponse, MailMcpError> {
    let total_started = Instant::now();
    let message_id: i64 = match params.message_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Err(MailMcpError::Validation(
                "Invalid message_id format. Expected a numeric ID from search results.".to_string(),
            ));
        }
    };

    let db_started = Instant::now();
    let row = match load_accessible_message(config, repo, message_id)? {
        AccessibleMessage::Found(row) => row,
        AccessibleMessage::NotFound => {
            return Err(MailMcpError::MessageNotFound {
                id: params.message_id.clone(),
            });
        }
        AccessibleMessage::BlockedAccount => {
            return Err(MailMcpError::Validation(
                "This message belongs to an account excluded by APPLE_MAIL_ACCOUNT.".to_string(),
            ));
        }
    };

    let epoch_offset_s = repo.detect_epoch_offset()?;

    let recipients = repo.get_recipients(message_id)?;
    let db_elapsed = db_started.elapsed();

    let mut to = Vec::new();
    let mut cc = Vec::new();
    for (addr, type_) in &recipients {
        match type_ {
            0 => to.push(addr.clone()),
            1 => cc.push(addr.clone()),
            _ => {}
        }
    }

    let mut result = GetMessageResult {
        id: row.rowid.to_string(),
        message_id_header: row.message_id_header.clone().or(row.message_id.clone()),
        subject: row.subject.clone().unwrap_or_default(),
        from: row.sender.clone().unwrap_or_default(),
        to: if params.include_recipients {
            to
        } else {
            Vec::new()
        },
        cc: if params.include_recipients {
            cc
        } else {
            Vec::new()
        },
        date_sent: row
            .date_sent
            .map(|ts| crate::domain::timestamp_to_iso(ts, epoch_offset_s)),
        date_received: row
            .date_received
            .map(|ts| crate::domain::timestamp_to_iso(ts, epoch_offset_s)),
        body: None,
        attachments: Vec::new(),
    };

    if params.include_body || params.include_attachments_summary {
        let locator_started = Instant::now();
        let emlx_path = locate_message_file(config, &row);
        let locator_elapsed = locator_started.elapsed();

        if let Some(path) = emlx_path {
            let parse_started = Instant::now();
            let cached = {
                let mut cache = BODY_CACHE.lock().expect("body cache lock");
                cache.get(&path).cloned()
            };

            let (body_text, body_html, attachments) = if let Some(cached) = cached {
                (cached.body_text, cached.body_html, cached.attachments)
            } else {
                match parse_emlx_without_attachment_content(&path) {
                    Ok(parsed) => {
                        let attachments = raw_attachments_to_meta(row.rowid, &parsed.attachments);
                        let cached = CachedMessage {
                            body_text: parsed.body_text,
                            body_html: parsed.body_html,
                            attachments,
                        };
                        let mut cache = BODY_CACHE.lock().expect("body cache lock");
                        cache.put(path.clone(), cached.clone());
                        (cached.body_text, cached.body_html, cached.attachments)
                    }
                    Err(MailMcpError::BodyFileNotFound { .. }) => {
                        return Ok(GetMessageResponse::partial(
                            result,
                            "Message body file not found on disk (emlx missing). The message index entry exists but the local file may have been deleted or not yet downloaded. Try another message or check Mail sync status.",
                        ));
                    }
                    Err(error) => {
                        tracing::warn!(
                            "failed to parse emlx for message_id={} mailbox={}: {}",
                            row.rowid,
                            row.mailbox_url.as_deref().unwrap_or("unknown"),
                            error
                        );
                        return Ok(GetMessageResponse::partial(
                            result,
                            "Message metadata was loaded, but the body could not be parsed from the local message file.",
                        ));
                    }
                }
            };

            if params.include_body {
                result.body = body_text
                    .or_else(|| body_html.as_deref().map(crate::mail::html_to_markdown))
                    .map(|text| {
                        let stripped = crate::mail::strip_quoted_replies(&text);
                        stripped.to_string()
                    });
            }

            if params.include_attachments_summary {
                result.attachments = attachments;
            }

            tracing::debug!(
                "get_message completed: message_id={}, db={} ms, locator={} ms, parse={} ms, total={} ms, include_body={}, include_attachments_summary={}",
                row.rowid,
                db_elapsed.as_millis(),
                locator_elapsed.as_millis(),
                parse_started.elapsed().as_millis(),
                total_started.elapsed().as_millis(),
                params.include_body,
                params.include_attachments_summary,
            );
        } else {
            return Ok(GetMessageResponse::partial(
                result,
                "No local message file matched this message inside the mailbox subtree. The message may not be downloaded, may only exist as a partial cache entry, or the local Mail storage layout may differ from the indexed metadata.",
            ));
        }
    }

    tracing::debug!(
        "get_message completed: message_id={}, db={} ms, locator=0 ms, parse=0 ms, total={} ms, include_body={}, include_attachments_summary={}",
        row.rowid,
        db_elapsed.as_millis(),
        total_started.elapsed().as_millis(),
        params.include_body,
        params.include_attachments_summary,
    );

    Ok(GetMessageResponse::success(result))
}

/// Execute the `get_message` tool.
///
/// # Errors
///
/// Returns an error if the database cannot be opened or accessed.
pub fn get_message(
    repo: &dyn MailRepository,
    _store: &dyn AttachmentStore,
    config: &MailConfig,
    params: GetMessageParams,
) -> Result<GetMessageResponse, MailMcpError> {
    get_message_with_conn(config, repo, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SqliteMailRepository;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    /// Create an in-memory test database with a minimal schema and seed data.
    fn make_test_repo() -> (TempDir, SqliteMailRepository) {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
        conn.execute_batch(
            r#"
            CREATE TABLE subjects (ROWID INTEGER PRIMARY KEY, subject TEXT);
            CREATE TABLE addresses (ROWID INTEGER PRIMARY KEY, address TEXT);
            CREATE TABLE sender_addresses (sender INTEGER PRIMARY KEY, address INTEGER REFERENCES addresses);
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (
                ROWID INTEGER PRIMARY KEY,
                subject INTEGER REFERENCES subjects,
                sender INTEGER REFERENCES sender_addresses,
                mailbox INTEGER REFERENCES mailboxes,
                date_sent INTEGER,
                date_received INTEGER,
                message_id TEXT,
                global_message_id INTEGER
            );
            CREATE TABLE message_global_data (
                ROWID INTEGER PRIMARY KEY,
                message_id INTEGER,
                message_id_header TEXT
            );
            CREATE TABLE recipients (
                message INTEGER REFERENCES messages,
                address INTEGER REFERENCES addresses,
                type INTEGER
            );

            -- Seed data
            INSERT INTO subjects VALUES (1, 'Test Subject');
            INSERT INTO addresses VALUES (1, 'sender@example.com'), (2, 'recipient@example.com');
            INSERT INTO sender_addresses VALUES (1, 1);
            INSERT INTO mailboxes VALUES (1, 'imap://account-a/INBOX');
            INSERT INTO message_global_data VALUES (10, 111, '<msg1@mail>');
            INSERT INTO messages VALUES (1, 1, 1, 1, 0, 0, '<msg1@mail>', 10);
            INSERT INTO recipients VALUES (1, 2, 1);
            "#,
        )
        .expect("seed test schema");
        drop(conn);
        let repo = SqliteMailRepository::new(db_path).unwrap();
        (temp_dir, repo)
    }

    fn make_test_config(
        temp_dir: &TempDir,
        allowed_account_ids: Option<Vec<String>>,
    ) -> MailConfig {
        let mail_directory = temp_dir.path().to_path_buf();
        let mail_version = "V10".to_string();
        let db_dir = mail_directory.join(&mail_version).join("MailData");
        std::fs::create_dir_all(&db_dir).expect("mail data dir");
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");
        MailConfig::from_parts_with_accounts(
            mail_directory,
            mail_version,
            allowed_account_ids,
            HashMap::new(),
        )
        .expect("valid config")
    }

    #[test]
    fn get_message_with_conn_invalid_message_id_format() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "invalid".to_string(),
            include_body: false,
            include_attachments_summary: false,

            include_recipients: false,
        };

        let err = get_message_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::Validation(_)));
        assert!(err.to_string().contains("Invalid message_id format"));
    }

    #[test]
    fn get_message_with_conn_message_not_found() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "999".to_string(),
            include_body: false,
            include_attachments_summary: false,

            include_recipients: false,
        };

        let err = get_message_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::MessageNotFound { .. }));
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn get_message_with_conn_blocked_account() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, Some(vec!["ews://other-account".to_string()]));
        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: false,
            include_attachments_summary: false,

            include_recipients: false,
        };

        let err = get_message_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::Validation(_)));
        assert!(err.to_string().contains("excluded by APPLE_MAIL_ACCOUNT"));
    }

    #[test]
    fn get_message_with_conn_success_no_body() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: false,
            include_attachments_summary: false,

            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &repo, params).unwrap();

        assert_eq!(response.status, None);
        assert!(response.message.is_some());
        let msg = response.message.unwrap();
        assert_eq!(msg.id, "1");
        assert_eq!(msg.subject, "Test Subject");
        assert_eq!(msg.from, "sender@example.com");
        assert!(msg.body.is_none());
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn get_message_with_conn_maps_apple_mail_recipient_types_zero_and_one() {
        let (temp_dir, repo) = make_test_repo();
        // Re-open connection to add more test data
        let db_path = temp_dir.path().join("test.db");
        {
            let conn = rusqlite::Connection::open(&db_path).expect("create db");
            conn.execute("DELETE FROM recipients WHERE message = 1", [])
                .expect("clear seeded recipients");
            conn.execute(
                "INSERT INTO addresses (ROWID, address) VALUES (?1, ?2)",
                rusqlite::params![3_i64, "cc@example.com"],
            )
            .expect("insert cc address");
            conn.execute_batch(
                r#"
                INSERT INTO recipients VALUES (1, 2, 0);
                INSERT INTO recipients VALUES (1, 3, 1);
                "#,
            )
            .expect("insert recipients");
            drop(conn);
        }
        // Re-create repo to pick up new data
        let repo = SqliteMailRepository::new(&db_path).unwrap();

        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: false,
            include_attachments_summary: false,

            include_recipients: true,
        };

        let response = get_message_with_conn(&config, &repo, params).unwrap();

        assert_eq!(response.status, None);
        let message = response.message.expect("message response");
        assert_eq!(message.to, vec!["recipient@example.com".to_string()]);
        assert_eq!(message.cc, vec!["cc@example.com".to_string()]);
    }

    #[test]
    fn get_message_with_conn_success_with_emlx() {
        let (temp_dir, repo) = make_test_repo();
        let temp_dir2 = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir2, None);

        // Create a fake .emlx file
        let mail_dir = temp_dir2
            .path()
            .join("V10")
            .join("account-a")
            .join("INBOX.mbox")
            .join("Messages");
        fs::create_dir_all(&mail_dir).unwrap();
        let emlx_path = mail_dir.join("1.emlx");
        let email_content = concat!(
            "From: sender@example.com\n",
            "To: recipient@example.com\n",
            "Subject: Test Subject\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello, World!\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: true,
            include_attachments_summary: false,

            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &repo, params).unwrap();

        assert_eq!(response.status, None);
        assert!(response.message.is_some());
        let msg = response.message.unwrap();
        assert!(msg.body.is_some());
        assert!(msg.body.unwrap().contains("Hello, World!"));
    }

    #[test]
    fn body_cache_stores_parsed_messages() {
        use super::BODY_CACHE;

        let test_path = std::path::PathBuf::from("/tmp/test.emlx");
        let cached = CachedMessage {
            body_text: Some("cached text".to_string()),
            body_html: Some("<html>cached</html>".to_string()),
            attachments: vec![],
        };

        // Insert into cache
        {
            let mut cache = BODY_CACHE.lock().expect("lock");
            cache.put(test_path.clone(), cached.clone());
        }

        // Retrieve from cache
        {
            let mut cache = BODY_CACHE.lock().expect("lock");
            let retrieved = cache.get(&test_path).expect("cached entry");
            assert_eq!(retrieved.body_text, Some("cached text".to_string()));
            assert_eq!(retrieved.body_html, Some("<html>cached</html>".to_string()));
        }
    }

    // Serialization tests

    #[test]
    fn message_id_header_none_is_omitted() {
        let result = GetMessageResult {
            id: "1".into(),
            message_id_header: None,
            subject: "test".into(),
            from: "a@b.com".into(),
            to: vec![],
            cc: vec![],
            date_sent: Some("2024-01-01T00:00Z".into()),
            date_received: Some("2024-01-01T00:00Z".into()),
            body: Some("text".into()),
            attachments: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("message_id_header"),
            "None should be omitted: {json}"
        );
    }

    #[test]
    fn message_id_header_some_is_present() {
        let result = GetMessageResult {
            id: "1".into(),
            message_id_header: Some("<abc@example.com>".into()),
            subject: "test".into(),
            from: "a@b.com".into(),
            to: vec![],
            cc: vec![],
            date_sent: Some("2024-01-01T00:00Z".into()),
            date_received: Some("2024-01-01T00:00Z".into()),
            body: Some("text".into()),
            attachments: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("message_id_header"),
            "Some should be present: {json}"
        );
    }

    #[test]
    fn to_cc_empty_omitted() {
        let result = GetMessageResult {
            id: "1".into(),
            message_id_header: None,
            subject: "test".into(),
            from: "a@b.com".into(),
            to: vec![],
            cc: vec![],
            date_sent: Some("2024-01-01T00:00Z".into()),
            date_received: Some("2024-01-01T00:00Z".into()),
            body: Some("text".into()),
            attachments: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("\"to\""),
            "empty to should be omitted: {json}"
        );
        assert!(
            !json.contains("\"cc\""),
            "empty cc should be omitted: {json}"
        );
    }

    #[test]
    fn to_cc_nonempty_present() {
        let result = GetMessageResult {
            id: "1".into(),
            message_id_header: None,
            subject: "test".into(),
            from: "a@b.com".into(),
            to: vec!["b@b.com".into()],
            cc: vec!["c@c.com".into()],
            date_sent: Some("2024-01-01T00:00Z".into()),
            date_received: Some("2024-01-01T00:00Z".into()),
            body: Some("text".into()),
            attachments: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("\"to\""),
            "nonempty to should be present: {json}"
        );
        assert!(
            json.contains("\"cc\""),
            "nonempty cc should be present: {json}"
        );
    }
}
