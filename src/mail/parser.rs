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
    /// Raw bytes of the attachment
    pub content: Vec<u8>,
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
    let file_bytes = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MailMcpError::BodyFileNotFound {
                path: path.to_path_buf(),
            }
        } else {
            MailMcpError::Io(e)
        }
    })?;

    let header_end = file_bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
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
    let message = MessageParser::default()
        .parse(email_bytes)
        .ok_or_else(|| MailMcpError::BodyFileNotFound {
            path: path.to_path_buf(),
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

        let content = attachment.contents().to_vec();

        // Check if inline based on content disposition
        let is_inline = attachment
            .content_disposition()
            .map(|d| format!("{d:?}").to_lowercase().contains("inline"))
            .unwrap_or(false);

        attachments.push(RawAttachment {
            filename,
            mime_type,
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
            size_bytes: raw.content.len() as u64,
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
    }

    #[test]
    fn parse_emlx_not_found_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let emlx_path = temp_dir.path().join("nonexistent.emlx");

        let result = parse_emlx(&emlx_path);
        assert!(matches!(result, Err(MailMcpError::BodyFileNotFound { .. })));
    }
}
