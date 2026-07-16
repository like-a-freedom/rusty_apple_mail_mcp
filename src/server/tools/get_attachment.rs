//! `get_attachment_content` tool implementation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::db::MailRepository;
use crate::domain::{AttachmentMeta, ContentFormat};
use crate::error::MailMcpError;
use crate::mail::AttachmentStore;
use crate::mail::extract::extract_text;
use crate::mail::parse_emlx;
use crate::server::tools::ResponseStatus;
use crate::server::tools::message_lookup::{
    AccessibleMessage, load_accessible_message, locate_message_file,
};

/// Parameters for the `get_attachment_content` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAttachmentParams {
    /// Attachment identifier (format: "`{message_id}:{attachment_index}`")
    pub attachment_id: String,
    /// Parent message identifier (needed to locate the attachment file)
    pub message_id: String,
}

/// Response for `get_attachment_content` tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[must_use]
pub struct GetAttachmentResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<GetAttachmentResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

impl GetAttachmentResponse {
    /// Create an error response with a guidance message.
    pub fn error(guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::Error),
            attachment: None,
            guidance: Some(guidance.into()),
        }
    }

    /// Create a not found response with a guidance message.
    pub fn not_found(guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::NotFound),
            attachment: None,
            guidance: Some(guidance.into()),
        }
    }

    /// Create a partial response with a result and guidance.
    pub fn partial(result: GetAttachmentResult, guidance: impl Into<String>) -> Self {
        Self {
            status: Some(ResponseStatus::Partial),
            attachment: Some(result),
            guidance: Some(guidance.into()),
        }
    }

    /// Create a success response with a result.
    pub fn success(result: GetAttachmentResult) -> Self {
        Self {
            status: None,
            attachment: Some(result),
            guidance: None,
        }
    }
}

/// Attachment result in response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetAttachmentResult {
    pub id: String,
    pub filename: String,
    pub content_format: ContentFormat,
    pub content: Option<String>,
    pub extraction_method: Option<String>,
}

/// Execute `get_attachment_content` using the repository trait.
///
/// # Errors
///
/// Returns an error if the database cannot be accessed or the message file cannot be parsed.
#[allow(clippy::too_many_lines)]
#[allow(clippy::ptr_arg, clippy::needless_pass_by_value)]
pub fn get_attachment_content_with_conn(
    config: &MailConfig,
    repo: &dyn MailRepository,
    params: GetAttachmentParams,
) -> Result<GetAttachmentResponse, MailMcpError> {
    let message_id: i64 = match params.message_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Err(MailMcpError::Validation(
                "Invalid message_id format. Expected a numeric ID from search results.".to_string(),
            ));
        }
    };

    let (attachment_rowid, attachment_index) = match params.attachment_id.split_once(':') {
        Some((rowid, index)) => {
            let rowid = rowid.parse::<i64>().ok();
            let index = index.parse::<usize>().ok();
            match (rowid, index) {
                (Some(rowid), Some(index)) => (rowid, index),
                _ => {
                    return Err(MailMcpError::Validation(
                        "Invalid attachment_id format. Expected \"{message_id}:{attachment_index}\"."
                            .to_string(),
                    ));
                }
            }
        }
        None => {
            return Err(MailMcpError::Validation(
                "Invalid attachment_id format. Expected \"{message_id}:{attachment_index}\"."
                    .to_string(),
            ));
        }
    };

    if attachment_rowid != message_id {
        return Err(MailMcpError::Validation(
            "attachment_id does not belong to the provided message_id.".to_string(),
        ));
    }

    let row = match load_accessible_message(config, repo, message_id)? {
        AccessibleMessage::Found(row) => row,
        AccessibleMessage::NotFound => {
            return Err(MailMcpError::MessageNotFound {
                id: params.message_id,
            });
        }
        AccessibleMessage::BlockedAccount => {
            return Err(MailMcpError::Validation(
                "This attachment belongs to an account excluded by APPLE_MAIL_ACCOUNT.".to_string(),
            ));
        }
    };

    let Some(emlx_path) = locate_message_file(config, &row) else {
        return Err(MailMcpError::Validation(
            "Message body file not found on disk (emlx missing). The message may not be downloaded yet or the local file was deleted.".to_string(),
        ));
    };

    let parsed = match parse_emlx(&emlx_path) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                "failed to parse message for attachment extraction: message_id={} attachment_id={} path={}: {}",
                message_id,
                params.attachment_id,
                emlx_path.display(),
                error
            );
            return Err(MailMcpError::Validation(
                "Failed to parse message body file. The file may be corrupt or in an unexpected format.".to_string(),
            ));
        }
    };

    let Some(raw_attachment) = parsed.attachments.get(attachment_index) else {
        return Err(MailMcpError::AttachmentNotFound {
            id: params.attachment_id,
            message_id: params.message_id,
        });
    };

    let meta = AttachmentMeta {
        id: params.attachment_id.clone(),
        filename: raw_attachment
            .filename
            .clone()
            .unwrap_or_else(|| "unnamed".to_string()),
    };

    let Some(content) = raw_attachment.content.as_deref() else {
        return Err(MailMcpError::Validation(
            "Attachment content is unavailable in the parsed message. The attachment data may be stored externally by Apple Mail or is in a format that cannot be decoded inline.".to_string(),
        ));
    };

    let base_result = GetAttachmentResult {
        id: meta.id.clone(),
        filename: meta.filename.clone(),
        content_format: ContentFormat::NotAvailable,
        content: None,
        extraction_method: None,
    };

    match extract_text(content, &raw_attachment.mime_type) {
        Ok(text) => {
            let result = GetAttachmentResult {
                content_format: ContentFormat::ExtractedText,
                content: Some(text),
                extraction_method: Some("extracted".to_string()),
                ..base_result
            };
            Ok(GetAttachmentResponse::success(result))
        }
        Err(e) => {
            let reason = e.to_string();
            let result = GetAttachmentResult {
                extraction_method: Some(reason.clone()),
                ..base_result
            };
            Ok(GetAttachmentResponse::partial(result, reason))
        }
    }
}

/// Execute the `get_attachment_content` tool.
///
/// # Errors
///
/// Returns an error if the database cannot be opened or accessed.
pub fn get_attachment_content(
    repo: &dyn MailRepository,
    _store: &dyn AttachmentStore,
    config: &MailConfig,
    params: GetAttachmentParams,
) -> Result<GetAttachmentResponse, MailMcpError> {
    get_attachment_content_with_conn(config, repo, params)
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
            CREATE TABLE attachments (
                ROWID INTEGER PRIMARY KEY,
                message INTEGER,
                attachment_id TEXT,
                name TEXT
            );
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

            -- Seed data
            INSERT INTO subjects VALUES (1, 'Test Subject');
            INSERT INTO addresses VALUES (1, 'sender@example.com');
            INSERT INTO sender_addresses VALUES (1, 1);
            INSERT INTO mailboxes VALUES (1, 'imap://account-a/INBOX');
            INSERT INTO message_global_data VALUES (10, 111, '<msg1@mail>');
            INSERT INTO messages VALUES (1, 1, 1, 1, 0, 0, '<msg1@mail>', 10);
            "#,
        )
        .expect("seed test schema");
        drop(conn);
        let repo = SqliteMailRepository::new(db_path).unwrap();
        (temp_dir, repo)
    }

    fn create_minimal_docx() -> Vec<u8> {
        use std::io::{Cursor, Write};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(
                                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
    <Default Extension="xml" ContentType="application/xml"/>
    <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
    <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
                        )
                        .unwrap();

            zip.start_file("_rels/.rels", options).unwrap();
            zip.write_all(
                                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
    <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
                        )
                        .unwrap();

            zip.start_file("word/document.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
    <w:body>
        <w:p>
            <w:pPr>
                <w:pStyle w:val="Heading1"/>
            </w:pPr>
            <w:r>
                <w:t>External DOCX</w:t>
            </w:r>
        </w:p>
        <w:p>
            <w:r>
                <w:t>Attachment payload</w:t>
            </w:r>
        </w:p>
    </w:body>
</w:document>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        buf.into_inner()
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
        MailConfig::new(
            mail_directory,
            mail_version,
            allowed_account_ids,
            HashMap::new(),
        )
        .expect("valid config")
    }

    #[test]
    fn get_attachment_content_with_conn_invalid_attachment_id_format() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "invalid".to_string(),
            message_id: "1".to_string(),
        };

        let err = get_attachment_content_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::Validation(_)));
        assert!(err.to_string().contains("Invalid attachment_id format"));
    }

    #[test]
    fn get_attachment_content_with_conn_message_not_found() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "999:0".to_string(),
            message_id: "999".to_string(),
        };

        let err = get_attachment_content_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::MessageNotFound { .. }));
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn get_attachment_content_with_conn_blocked_account() {
        let (temp_dir, repo) = make_test_repo();
        let config = make_test_config(&temp_dir, Some(vec!["ews://other-account".to_string()]));
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        let err = get_attachment_content_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::Validation(_)));
        assert!(err.to_string().contains("excluded by APPLE_MAIL_ACCOUNT"));
    }

    #[test]
    fn get_attachment_content_with_conn_attachment_not_found() {
        let (_temp_dir, repo) = make_test_repo();
        let temp_dir2 = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir2, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        // Create a fake .emlx file without attachments
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

        let err = get_attachment_content_with_conn(&config, &repo, params).unwrap_err();

        assert!(matches!(err, MailMcpError::AttachmentNotFound { .. }));
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn get_attachment_content_with_conn_success_text_attachment() {
        let (_temp_dir, repo) = make_test_repo();
        let temp_dir2 = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir2, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        // Create a fake .emlx file with a text attachment
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
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from body\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"notes.txt\"\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\n",
            "\n",
            "Attachment content\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let response = get_attachment_content_with_conn(&config, &repo, params).unwrap();

        assert_eq!(response.status, None);
        assert!(response.attachment.is_some());
        let attachment = response.attachment.unwrap();
        assert_eq!(attachment.filename, "notes.txt");
        assert_eq!(attachment.content_format, ContentFormat::ExtractedText);
        assert_eq!(attachment.content.as_deref(), Some("Attachment content"));
        assert_eq!(attachment.extraction_method.as_deref(), Some("extracted"));
    }

    #[test]
    fn get_attachment_content_with_conn_success_binary_attachment() {
        let (_temp_dir, repo) = make_test_repo();
        let temp_dir2 = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir2, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        // Create a fake .emlx file with a binary attachment (image)
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
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from body\n",
            "--boundary\n",
            "Content-Type: image/png; name=\"image.png\"\n",
            "Content-Disposition: attachment; filename=\"image.png\"\n",
            "\n",
            "fake image data\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let response = get_attachment_content_with_conn(&config, &repo, params).unwrap();

        // Should return partial status with guidance about OCR
        assert_eq!(response.status, Some(ResponseStatus::Partial));
        assert!(response.attachment.is_some());
        let attachment = response.attachment.unwrap();
        assert_eq!(attachment.filename, "image.png");
        assert_eq!(attachment.content_format, ContentFormat::NotAvailable);
        assert!(attachment.content.is_none());
        assert!(attachment.extraction_method.is_some());
        assert!(response.guidance.is_some());
    }

    #[test]
    fn get_attachment_content_with_conn_falls_back_to_external_apple_mail_attachment() {
        let (temp_dir, _repo) = make_test_repo();
        // Add attachment to the database via direct connection
        let db_path = temp_dir.path().join("test.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "INSERT INTO attachments (ROWID, message, attachment_id, name) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![1_i64, 1_i64, "2", "Test Document.docx"],
            )
            .unwrap();
            drop(conn);
        }
        // Re-create repo to pick up new data
        let repo = SqliteMailRepository::new(&db_path).unwrap();

        let temp_dir2 = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir2, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        let docx_bytes = create_minimal_docx();

        let mail_dir = temp_dir2
            .path()
            .join("V10")
            .join("account-a")
            .join("INBOX.mbox");
        let messages_dir = mail_dir.join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        let emlx_path = messages_dir.join("1.partial.emlx");
        let email_content = concat!(
            "From: sender@example.com\n",
            "To: recipient@example.com\n",
            "Subject: Test Subject\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from body\n",
            "--boundary\n",
            "Content-Transfer-Encoding: base64\n",
            "Content-Disposition: attachment; filename=\"Test Document.docx\"\n",
            "Content-Type: application/vnd.openxmlformats-officedocument.wordprocessingml.document; name=\"Test Document.docx\"\n",
            "X-Apple-Content-Length: 2048\n",
            "\n",
            "\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let attachment_path = mail_dir
            .join("Attachments")
            .join("1")
            .join("2")
            .join("Test Document.docx");
        fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
        fs::write(&attachment_path, docx_bytes).unwrap();

        let response = get_attachment_content_with_conn(&config, &repo, params).unwrap();

        assert_eq!(response.status, None);
        let attachment = response.attachment.expect("attachment result");
        assert_eq!(attachment.content_format, ContentFormat::ExtractedText);
        assert_eq!(attachment.extraction_method.as_deref(), Some("extracted"));
        let content = attachment.content.expect("extracted content");
        assert!(
            content.contains("External DOCX"),
            "unexpected content: {content}"
        );
        assert!(
            content.contains("Attachment payload"),
            "unexpected content: {content}"
        );
    }
}
