//! `get_attachment_content` tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::domain::{AttachmentMeta, ContentFormat};
use crate::error::MailMcpError;
use crate::mail::{extract_text, parse_emlx};
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
    pub mime_type: String,
    pub size_bytes: u64,
    pub is_inline: bool,
    pub content_format: ContentFormat,
    pub content: Option<String>,
    pub extraction_method: Option<String>,
}

/// Execute `get_attachment_content` against an already-open `SQLite` connection.
///
/// # Errors
///
/// Returns an error if the database cannot be accessed or the message file cannot be parsed.
#[allow(clippy::too_many_lines)]
#[allow(clippy::ptr_arg, clippy::needless_pass_by_value)]
pub fn get_attachment_content_with_conn(
    config: &MailConfig,
    conn: &Connection,
    params: GetAttachmentParams,
) -> Result<GetAttachmentResponse, MailMcpError> {
    let message_id: i64 = match params.message_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(GetAttachmentResponse::error(
                "Invalid message_id format. Expected a numeric ID from search results.",
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
                    return Ok(GetAttachmentResponse::error(
                        "Invalid attachment_id format. Expected \"{message_id}:{attachment_index}\".",
                    ));
                }
            }
        }
        None => {
            return Ok(GetAttachmentResponse::error(
                "Invalid attachment_id format. Expected \"{message_id}:{attachment_index}\".",
            ));
        }
    };

    if attachment_rowid != message_id {
        return Ok(GetAttachmentResponse::error(
            "attachment_id does not belong to the provided message_id.",
        ));
    }

    let row = match load_accessible_message(config, conn, message_id)? {
        AccessibleMessage::Found(row) => row,
        AccessibleMessage::NotFound => {
            return Ok(GetAttachmentResponse::not_found(
                "Message not found in the index.",
            ));
        }
        AccessibleMessage::BlockedAccount => {
            return Ok(GetAttachmentResponse::error(
                "This attachment belongs to an account excluded by APPLE_MAIL_ACCOUNT.",
            ));
        }
    };

    let Some(emlx_path) = locate_message_file(config, &row) else {
        return Ok(GetAttachmentResponse::not_found(
            "Message body file not found on disk.",
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
            return Ok(GetAttachmentResponse::error(
                "Failed to parse message body file.",
            ));
        }
    };

    let Some(raw_attachment) = parsed.attachments.get(attachment_index) else {
        return Ok(GetAttachmentResponse::not_found(format!(
            "Attachment index {attachment_index} out of range. Message has {} attachment(s).",
            parsed.attachments.len()
        )));
    };

    let meta = AttachmentMeta {
        id: params.attachment_id.clone(),
        filename: raw_attachment
            .filename
            .clone()
            .unwrap_or_else(|| "unnamed".to_string()),
        mime_type: raw_attachment.mime_type.clone(),
        size_bytes: raw_attachment.size_bytes,
        is_inline: raw_attachment.is_inline,
    };

    let Some(content) = raw_attachment.content.as_deref() else {
        return Ok(GetAttachmentResponse::error(
            "Attachment content is unavailable in the parsed message.",
        ));
    };

    let base_result = GetAttachmentResult {
        id: meta.id.clone(),
        filename: meta.filename.clone(),
        mime_type: meta.mime_type.clone(),
        size_bytes: meta.size_bytes,
        is_inline: meta.is_inline,
        content_format: ContentFormat::NotAvailable,
        content: None,
        extraction_method: None,
    };

    match extract_text(content, &raw_attachment.mime_type) {
        crate::mail::ExtractionResult::Text { content, method } => {
            let result = GetAttachmentResult {
                content_format: ContentFormat::ExtractedText,
                content: Some(content),
                extraction_method: Some(method.to_string()),
                ..base_result
            };
            Ok(GetAttachmentResponse::success(result))
        }
        crate::mail::ExtractionResult::NotSupported { reason } => {
            let result = GetAttachmentResult {
                extraction_method: Some(reason.to_string()),
                ..base_result
            };
            Ok(GetAttachmentResponse::partial(result, reason.to_string()))
        }
    }
}

/// Execute the `get_attachment_content` tool.
///
/// # Errors
///
/// Returns an error if the database cannot be opened or accessed.
pub fn get_attachment_content(
    config: &MailConfig,
    params: GetAttachmentParams,
) -> Result<GetAttachmentResponse, MailMcpError> {
    let db_path = config.envelope_db_path();
    let conn = crate::db::open_readonly(&db_path)?;
    get_attachment_content_with_conn(config, &conn, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ContentFormat;
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
        conn
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
        MailConfig::from_parts_with_accounts(
            mail_directory,
            mail_version,
            allowed_account_ids,
            HashMap::new(),
        )
        .expect("valid config")
    }

    #[test]
    fn get_attachment_content_with_conn_invalid_attachment_id_format() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "invalid".to_string(),
            message_id: "1".to_string(),
        };

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::Error));
        assert!(response.guidance.is_some());
        assert!(
            response
                .guidance
                .unwrap()
                .contains("Invalid attachment_id format")
        );
    }

    #[test]
    fn get_attachment_content_with_conn_message_not_found() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "999:0".to_string(),
            message_id: "999".to_string(),
        };

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::NotFound));
        assert!(response.guidance.is_some());
        assert!(response.guidance.unwrap().contains("Message not found"));
    }

    #[test]
    fn get_attachment_content_with_conn_blocked_account() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, Some(vec!["ews://other-account".to_string()]));
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

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
    fn get_attachment_content_with_conn_attachment_not_found() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        // Create a fake .emlx file without attachments
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

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, Some(ResponseStatus::NotFound));
        assert!(response.guidance.is_some());
        assert!(response.guidance.unwrap().contains("out of range"));
    }

    #[test]
    fn get_attachment_content_with_conn_success_text_attachment() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        // Create a fake .emlx file with a text attachment
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

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, None);
        assert!(response.attachment.is_some());
        let attachment = response.attachment.unwrap();
        assert_eq!(attachment.filename, "notes.txt");
        assert_eq!(attachment.mime_type, "text/plain");
        assert_eq!(attachment.content_format, ContentFormat::ExtractedText);
        assert_eq!(attachment.content.as_deref(), Some("Attachment content"));
        assert_eq!(attachment.extraction_method.as_deref(), Some("direct_utf8"));
    }

    #[test]
    fn get_attachment_content_with_conn_success_binary_attachment() {
        let conn = make_test_db();
        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        // Create a fake .emlx file with a binary attachment (image)
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

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

        // Should return partial status with guidance about OCR
        assert_eq!(response.status, Some(ResponseStatus::Partial));
        assert!(response.attachment.is_some());
        let attachment = response.attachment.unwrap();
        assert_eq!(attachment.filename, "image.png");
        assert_eq!(attachment.mime_type, "image/png");
        assert_eq!(attachment.content_format, ContentFormat::NotAvailable);
        assert!(attachment.content.is_none());
        assert!(attachment.extraction_method.is_some());
        assert!(response.guidance.is_some());
    }

    #[test]
    fn get_attachment_content_with_conn_falls_back_to_external_apple_mail_attachment() {
        let conn = make_test_db();
        conn.execute(
            "INSERT INTO attachments (ROWID, message, attachment_id, name) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![1_i64, 1_i64, "2", "Test Document.docx"],
        )
        .unwrap();

        let temp_dir = TempDir::new().unwrap();
        let config = make_test_config(&temp_dir, None);
        let params = GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
        };

        let docx_bytes = create_minimal_docx();

        let mail_dir = temp_dir
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

        let response = get_attachment_content_with_conn(&config, &conn, params).unwrap();

        assert_eq!(response.status, None);
        let attachment = response.attachment.expect("attachment result");
        assert_eq!(
            attachment.mime_type,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(attachment.content_format, ContentFormat::ExtractedText);
        assert_eq!(
            attachment.extraction_method.as_deref(),
            Some("docx_to_markdown")
        );
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
