//! get_message tool implementation.

use lru::LruCache;
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::MailConfig;
use crate::db::{
    detect_epoch_offset_seconds, get_message_by_id, get_recipients, mailbox_account_id,
    open_readonly,
};
use crate::domain::AttachmentMeta;
use crate::error::MailMcpError;
use crate::mail::{
    locate_emlx_with_hints, parse_emlx_without_attachment_content, raw_attachments_to_meta,
};
use crate::server::tools::ResponseStatus;

/// LRU cache for parsed .emlx bodies keyed by resolved path.
static BODY_CACHE: once_cell::sync::Lazy<Mutex<LruCache<std::path::PathBuf, CachedMessage>>> =
    once_cell::sync::Lazy::new(|| {
        Mutex::new(LruCache::new(NonZeroUsize::new(256).expect("cache size")))
    });

#[derive(Clone)]
struct CachedMessage {
    body_text: Option<String>,
    body_html: Option<String>,
    attachments: Vec<AttachmentMeta>,
}

/// Parameters for the get_message tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessageParams {
    /// Stable message identifier (from search results)
    pub message_id: String,
    /// Include message body (default true)
    #[serde(default = "default_true")]
    pub include_body: bool,
    /// Include attachment list (default true)
    #[serde(default = "default_true")]
    pub include_attachments_summary: bool,
    /// Body format: "text", "html", or "both"
    #[serde(default)]
    pub body_format: BodyFormat,
    /// Include To/CC recipients lists (default false).
    /// Enable when you need to check who received the message.
    #[serde(default)]
    pub include_recipients: bool,
}

fn default_true() -> bool {
    true
}

/// Body format option.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BodyFormat {
    #[default]
    Text,
    Html,
    Both,
}

/// Response for get_message tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetMessageResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<GetMessageResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Message result in get_message response.
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
    pub mailbox: String,
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_html: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
}

/// Execute `get_message` against an already-open SQLite connection.
pub fn get_message_with_conn(
    config: &MailConfig,
    conn: &Connection,
    params: GetMessageParams,
) -> Result<GetMessageResponse, MailMcpError> {
    let total_started = Instant::now();
    let message_id: i64 = match params.message_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(GetMessageResponse {
                status: Some(ResponseStatus::Error),
                message: None,
                guidance: Some(
                    "Invalid message_id format. Expected a numeric ID from search results."
                        .to_string(),
                ),
            });
        }
    };

    let db_started = Instant::now();
    let epoch_offset_s = detect_epoch_offset_seconds(conn)?;
    let row = match get_message_by_id(conn, message_id)? {
        Some(row) => row,
        None => {
            return Ok(GetMessageResponse {
                status: Some(ResponseStatus::NotFound),
                message: None,
                guidance: Some(
                    "Message not found in the index. The message_id may be incorrect or the message was deleted."
                        .to_string(),
                ),
            });
        }
    };

    if let Some(mailbox_url) = row.mailbox_url.as_deref()
        && !config.is_mailbox_allowed(mailbox_url)
    {
        return Ok(GetMessageResponse {
            status: Some(ResponseStatus::Error),
            message: None,
            guidance: Some(
                "This message belongs to an account excluded by APPLE_MAIL_ACCOUNT.".to_string(),
            ),
        });
    }

    let recipients = get_recipients(conn, message_id)?;
    let db_elapsed = db_started.elapsed();
    let mailbox = row
        .mailbox_url
        .as_ref()
        .map(|url| url.rsplit('/').next().unwrap_or(url).to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let _account_id = row.mailbox_url.as_deref().and_then(mailbox_account_id);

    let mut to = Vec::new();
    let mut cc = Vec::new();
    for (addr, type_) in &recipients {
        match type_ {
            1 => to.push(addr.clone()),
            2 => cc.push(addr.clone()),
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
        mailbox,
        body: None,
        body_html: None,
        attachments: Vec::new(),
    };

    if params.include_body || params.include_attachments_summary {
        let locator_started = Instant::now();
        let mut numeric_hints = vec![row.rowid.to_string()];
        if let Some(global_message_id) = row.global_message_id {
            numeric_hints.push(global_message_id.to_string());
        }
        if let Some(message_id) = row.message_id.as_ref() {
            numeric_hints.push(message_id.clone());
        }
        numeric_hints.sort();
        numeric_hints.dedup();

        let emlx_path = locate_emlx_with_hints(
            &config.mail_directory,
            &config.mail_version,
            row.mailbox_url.as_deref().unwrap_or(""),
            row.rowid,
            &numeric_hints,
            row.message_id_header
                .as_deref()
                .or(row.message_id.as_deref()),
        );
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
                        return Ok(GetMessageResponse {
                            status: Some(ResponseStatus::Partial),
                            message: Some(result),
                            guidance: Some(
                                "Message body file not found on disk (emlx missing). The message index entry exists but the local file may have been deleted or not yet downloaded. Try another message or check Mail sync status.".to_string(),
                            ),
                        });
                    }
                    Err(error) => {
                        tracing::warn!(
                            "failed to parse emlx for message_id={} mailbox={}: {}",
                            row.rowid,
                            row.mailbox_url.as_deref().unwrap_or("unknown"),
                            error
                        );
                        return Ok(GetMessageResponse {
                            status: Some(ResponseStatus::Partial),
                            message: Some(result),
                            guidance: Some(
                                "Message metadata was loaded, but the body could not be parsed from the local message file.".to_string(),
                            ),
                        });
                    }
                }
            };

            if params.include_body {
                result.body = match params.body_format {
                    BodyFormat::Text => body_text
                        .or_else(|| body_html.as_deref().map(crate::mail::html_to_plain_text)),
                    BodyFormat::Html => body_html.clone(),
                    BodyFormat::Both => {
                        let text = body_text
                            .or_else(|| body_html.as_deref().map(crate::mail::html_to_plain_text));
                        if matches!(params.body_format, BodyFormat::Both) {
                            result.body_html = body_html;
                        }
                        text
                    }
                };
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
            return Ok(GetMessageResponse {
                status: Some(ResponseStatus::Partial),
                message: Some(result),
                guidance: Some(
                    "No local message file matched this message inside the mailbox subtree. The message may not be downloaded, may only exist as a partial cache entry, or the local Mail storage layout may differ from the indexed metadata.".to_string(),
                ),
            });
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

    Ok(GetMessageResponse {
        status: None,
        message: Some(result),
        guidance: None,
    })
}

/// Execute the get_message tool.
pub fn get_message(
    config: &MailConfig,
    params: GetMessageParams,
) -> Result<GetMessageResponse, MailMcpError> {
    let db_path = config.envelope_db_path();
    let conn = open_readonly(&db_path)?;
    get_message_with_conn(config, &conn, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    /// Create an in-memory test database with a minimal schema and seed data.
    fn make_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
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
        conn
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
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "invalid".to_string(),
            include_body: false,
            include_attachments_summary: false,
            body_format: BodyFormat::Text,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::Error));
        assert!(response.guidance.is_some());
        assert!(
            response
                .guidance
                .unwrap()
                .contains("Invalid message_id format")
        );
    }

    #[test]
    fn get_message_with_conn_message_not_found() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "999".to_string(),
            include_body: false,
            include_attachments_summary: false,
            body_format: BodyFormat::Text,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::NotFound));
        assert!(response.guidance.is_some());
        assert!(response.guidance.unwrap().contains("Message not found"));
    }

    #[test]
    fn get_message_with_conn_blocked_account() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, Some(vec!["ews://other-account".to_string()]));
        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: false,
            include_attachments_summary: false,
            body_format: BodyFormat::Text,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::Error));
        assert!(response.guidance.is_some());
        assert!(
            response
                .guidance
                .unwrap()
                .contains("excluded by APPLE_MAIL_ACCOUNT")
        );
    }

    #[test]
    fn get_message_with_conn_success_no_body() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: false,
            include_attachments_summary: false,
            body_format: BodyFormat::Text,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

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
    fn get_message_with_conn_success_with_emlx() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);

        // Create a fake .emlx file
        let mail_dir = temp_dir
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
            body_format: BodyFormat::Text,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, None);
        assert!(response.message.is_some());
        let msg = response.message.unwrap();
        assert!(msg.body.is_some());
        assert!(msg.body.unwrap().contains("Hello, World!"));
    }

    #[test]
    fn get_message_with_conn_body_format_html() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);

        // Create a fake .emlx file with HTML body
        let mail_dir = temp_dir
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
            "Content-Type: text/html; charset=utf-8\n",
            "\n",
            "<html><body><p>Hello HTML!</p></body></html>\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: true,
            include_attachments_summary: false,
            body_format: BodyFormat::Html,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, None);
        let msg = response.message.unwrap();
        assert!(msg.body.is_some());
        assert!(msg.body.unwrap().contains("<html>"));
    }

    #[test]
    fn get_message_with_conn_body_format_both() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);

        // Create a fake .emlx file with both text and HTML
        let mail_dir = temp_dir
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
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/alternative; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Plain text body\n",
            "--boundary\n",
            "Content-Type: text/html; charset=utf-8\n",
            "\n",
            "<html><body>HTML body</body></html>\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let params = GetMessageParams {
            message_id: "1".to_string(),
            include_body: true,
            include_attachments_summary: false,
            body_format: BodyFormat::Both,
            include_recipients: false,
        };

        let response = get_message_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, None);
        let msg = response.message.unwrap();
        // With BodyFormat::Both, body should contain text, and body_html should have HTML
        assert!(msg.body.is_some());
        assert!(msg.body_html.is_some());
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
}
