//! Parse .emlx files to extract email content and attachments.
//!
//! The .emlx format consists of:
//! 1. A byte count on the first line
//! 2. The RFC 2822 email bytes
//! 3. Optional XML metadata (plist)
//!
//! We use the `mail-parser` crate to parse the RFC 2822 content.

use std::path::Path;

use mail_parser::{MessageParser, MimeHeaders};

use crate::domain::AttachmentMeta;
use crate::error::MailMcpError;

/// A parsed email with body and attachment data.
#[derive(Debug, Clone)]
pub struct ParsedEmail {
    /// Plain text body (if available)
    pub body_text: Option<String>,
    /// HTML body (if available)
    pub body_html: Option<String>,
    /// Attachments found in the email
    pub attachments: Vec<RawAttachment>,
}

/// Raw attachment data extracted from an email.
#[derive(Debug, Clone)]
pub struct RawAttachment {
    /// Filename of the attachment (if available)
    pub filename: Option<String>,
    /// MIME type of the attachment
    pub mime_type: String,
    /// Size of the attachment content in bytes
    pub size_bytes: u64,
    /// Raw bytes of the attachment
    pub content: Option<Vec<u8>>,
    /// Whether the attachment is inline (embedded in the message body)
    pub is_inline: bool,
}

/// Parse an .emlx file and extract its content.
///
/// # Arguments
///
/// * `path` - Path to the .emlx file
///
/// # Returns
///
/// Parsed email content or an error.
pub fn parse_emlx(path: &Path) -> Result<ParsedEmail, MailMcpError> {
    parse_emlx_internal(path, true)
}

/// Parse an `.emlx` file while skipping attachment byte copies.
pub fn parse_emlx_without_attachment_content(path: &Path) -> Result<ParsedEmail, MailMcpError> {
    parse_emlx_internal(path, false)
}

fn parse_emlx_internal(
    path: &Path,
    include_attachment_content: bool,
) -> Result<ParsedEmail, MailMcpError> {
    let file_bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MailMcpError::BodyFileNotFound {
                path: path.to_path_buf(),
            }
        } else {
            MailMcpError::Io(e)
        }
    })?;

    let header_end = file_bytes.iter().position(|b| *b == b'\n').ok_or_else(|| {
        MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
        }
    })?;

    let byte_count: usize = std::str::from_utf8(&file_bytes[..header_end])
        .map_err(|_| MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
        })?
        .trim()
        .parse()
        .map_err(|_| MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
        })?;

    let email_start = header_end + 1;
    let email_end = email_start.saturating_add(byte_count);
    if file_bytes.len() < email_end {
        return Err(MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
        });
    }

    let email_bytes = &file_bytes[email_start..email_end];

    // Parse the email content
    let message = MessageParser::default().parse(email_bytes).ok_or_else(|| {
        MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
        }
    })?;

    // Extract body text (prefer plain text, fallback to HTML)
    let body_text = message.body_text(0).map(|s| s.to_string());

    let body_html = message.body_html(0).map(|s| s.to_string());

    // Extract attachments
    let mut attachments = Vec::new();
    for attachment in message.attachments() {
        let filename = attachment.attachment_name().map(|s| s.to_string());

        // Get MIME type as string
        let mime_type = attachment
            .content_type()
            .map(content_type_to_mime)
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let content_bytes = attachment.contents();
        let size_bytes = content_bytes.len() as u64;
        let content = include_attachment_content.then(|| content_bytes.to_vec());

        // Check if inline based on content disposition
        let is_inline = attachment
            .content_disposition()
            .map(|d| format!("{d:?}").to_lowercase().contains("inline"))
            .unwrap_or(false);

        attachments.push(RawAttachment {
            filename,
            mime_type,
            size_bytes,
            content,
            is_inline,
        });
    }

    Ok(ParsedEmail {
        body_text,
        body_html,
        attachments,
    })
}

fn content_type_to_mime(content_type: &mail_parser::ContentType<'_>) -> String {
    match content_type.c_subtype.as_deref() {
        Some(subtype) => format!("{}/{}", content_type.c_type, subtype),
        None => content_type.c_type.to_string(),
    }
}

/// Convert raw attachments to domain AttachmentMeta.
pub fn raw_attachments_to_meta(
    message_rowid: i64,
    raw_attachments: &[RawAttachment],
) -> Vec<AttachmentMeta> {
    raw_attachments
        .iter()
        .enumerate()
        .map(|(index, raw)| AttachmentMeta {
            id: format!("{message_rowid}:{index}"),
            filename: raw
                .filename
                .clone()
                .unwrap_or_else(|| "unnamed".to_string()),
            mime_type: raw.mime_type.clone(),
            size_bytes: raw.size_bytes,
            is_inline: raw.is_inline,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_simple_emlx() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("test.emlx");

        // Create a minimal .emlx file
        let email_content = b"From: sender@example.com
To: recipient@example.com
Subject: Test
Date: Mon, 1 Jan 2024 00:00:00 +0000
Content-Type: text/plain; charset=utf-8

Hello, World!
";
        let byte_count = email_content.len();
        let emlx_content = format!("{byte_count}\n{}", String::from_utf8_lossy(email_content));
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path).unwrap();
        assert_eq!(result.body_text, Some("Hello, World!\n".to_string()));
        assert!(result.attachments.is_empty());
    }

    #[test]
    fn parse_multipart_emlx_with_attachment() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("multipart.emlx");

        let email_content = concat!(
            "From: sender@example.com\n",
            "To: recipient@example.com\n",
            "Subject: =?UTF-8?Q?Quarterly_=E2=9C=85?=\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello multipart world\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"notes.txt\"\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\n",
            "\n",
            "Attachment body\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path).unwrap();
        assert_eq!(result.body_text.as_deref(), Some("Hello multipart world"));
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].filename.as_deref(), Some("notes.txt"));
        assert_eq!(result.attachments[0].mime_type, "text/plain");
        assert_eq!(
            result.attachments[0].size_bytes,
            "Attachment body".len() as u64
        );
        assert_eq!(
            result.attachments[0].content.as_deref(),
            Some(b"Attachment body".as_slice())
        );
    }

    #[test]
    fn parse_emlx_not_found_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("nonexistent.emlx");

        let result = parse_emlx(&emlx_path);
        assert!(matches!(result, Err(MailMcpError::BodyFileNotFound { .. })));
    }

    #[test]
    fn parse_emlx_without_attachment_content_keeps_attachment_sizes() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("metadata_only.emlx");

        let email_content = concat!(
            "From: sender@example.com\n",
            "To: recipient@example.com\n",
            "Subject: Metadata only\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Body text\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"notes.txt\"\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\n",
            "\n",
            "Attachment payload\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx_without_attachment_content(&emlx_path).unwrap();
        assert_eq!(result.body_text.as_deref(), Some("Body text"));
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].filename.as_deref(), Some("notes.txt"));
        assert_eq!(
            result.attachments[0].size_bytes,
            "Attachment payload".len() as u64
        );
        assert_eq!(result.attachments[0].content, None);
    }

    #[test]
    fn parse_emlx_invalid_byte_count() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("invalid.emlx");

        // Invalid: byte count is not a number
        let emlx_content = "not_a_number\nFrom: test@example.com\n\nBody";
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path);
        assert!(matches!(result, Err(MailMcpError::BodyFileNotFound { .. })));
    }

    #[test]
    fn parse_emlx_missing_newline_after_byte_count() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("missing_newline.emlx");

        // Invalid: no newline after byte count
        let emlx_content = "100From: test@example.com\n\nBody";
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path);
        assert!(matches!(result, Err(MailMcpError::BodyFileNotFound { .. })));
    }

    #[test]
    fn parse_emlx_byte_count_exceeds_file_size() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("truncated.emlx");

        // Byte count says 1000 but file is shorter
        let emlx_content = "1000\nFrom: test@example.com\n\nBody";
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path);
        assert!(matches!(result, Err(MailMcpError::BodyFileNotFound { .. })));
    }

    #[test]
    fn parse_simple_emlx_with_html_body() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("html.emlx");

        let email_content = concat!(
            "From: sender@example.com\n",
            "To: recipient@example.com\n",
            "Subject: HTML Email\n",
            "MIME-Version: 1.0\n",
            "Content-Type: text/html; charset=utf-8\n",
            "\n",
            "<html><body><p>Hello HTML!</p></body></html>\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path).unwrap();
        assert!(result.body_html.is_some());
        // HTML-only emails may have body_text extracted from HTML
        assert!(result.body_text.is_some() || result.body_html.is_some());
    }

    #[test]
    fn parse_emlx_with_inline_attachment() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("inline.emlx");

        let email_content = concat!(
            "From: sender@example.com\n",
            "To: recipient@example.com\n",
            "Subject: Inline attachment\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/related; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Body with inline image\n",
            "--boundary\n",
            "Content-Type: image/png; name=\"image.png\"\n",
            "Content-Disposition: inline; filename=\"image.png\"\n",
            "Content-Transfer-Encoding: base64\n",
            "\n",
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==\n",
            "--boundary--\n"
        );
        let emlx_content = format!("{}\n{}", email_content.len(), email_content);
        fs::write(&emlx_path, emlx_content).unwrap();

        let result = parse_emlx(&emlx_path).unwrap();
        assert_eq!(result.attachments.len(), 1);
        assert!(result.attachments[0].is_inline);
        assert_eq!(result.attachments[0].filename.as_deref(), Some("image.png"));
        assert_eq!(result.attachments[0].mime_type, "image/png");
    }

    #[test]
    fn parsed_email_debug_format() {
        let email = ParsedEmail {
            body_text: Some("test body".to_string()),
            body_html: Some("<p>test</p>".to_string()),
            attachments: vec![],
        };
        let debug_str = format!("{:?}", email);
        assert!(debug_str.contains("body_text"));
        assert!(debug_str.contains("body_html"));
    }

    #[test]
    fn raw_attachment_debug_format() {
        let attachment = RawAttachment {
            filename: Some("test.txt".to_string()),
            mime_type: "text/plain".to_string(),
            size_bytes: 100,
            content: Some(b"test".to_vec()),
            is_inline: false,
        };
        let debug_str = format!("{:?}", attachment);
        assert!(debug_str.contains("test.txt"));
        assert!(debug_str.contains("text/plain"));
    }
}
