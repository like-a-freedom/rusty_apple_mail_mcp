//! get_attachment_content tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::domain::{AttachmentMeta, ContentFormat};
use crate::error::MailMcpError;
use crate::mail::{extract_text, locate_emlx, parse_emlx};

/// Parameters for the get_attachment_content tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetAttachmentParams {
    /// Attachment identifier (format: "{message_id}:{attachment_index}")
    pub attachment_id: String,
    /// Parent message identifier (needed to locate the attachment file)
    pub message_id: String,
}

/// Response for get_attachment_content tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetAttachmentResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<GetAttachmentResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
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

/// Execute `get_attachment_content` against an already-open SQLite connection.
pub fn get_attachment_content_with_conn(
    config: &MailConfig,
    conn: &Connection,
    params: GetAttachmentParams,
) -> Result<GetAttachmentResponse, MailMcpError> {
    let message_id: i64 = match params.message_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(GetAttachmentResponse {
                status: "error".to_string(),
                attachment: None,
                guidance: Some(
                    "Invalid message_id format. Expected a numeric ID from search results."
                        .to_string(),
                ),
            });
        }
    };

    let (attachment_rowid, attachment_index) = match params.attachment_id.split_once(':') {
        Some((rowid, index)) => {
            let rowid = rowid.parse::<i64>().ok();
            let index = index.parse::<usize>().ok();
            match (rowid, index) {
                (Some(rowid), Some(index)) => (rowid, index),
                _ => {
                    return Ok(GetAttachmentResponse {
                        status: "error".to_string(),
                        attachment: None,
                        guidance: Some(
                            "Invalid attachment_id format. Expected \"{message_id}:{attachment_index}\"."
                                .to_string(),
                        ),
                    });
                }
            }
        }
        None => {
            return Ok(GetAttachmentResponse {
                status: "error".to_string(),
                attachment: None,
                guidance: Some(
                    "Invalid attachment_id format. Expected \"{message_id}:{attachment_index}\"."
                        .to_string(),
                ),
            });
        }
    };

    if attachment_rowid != message_id {
        return Ok(GetAttachmentResponse {
            status: "error".to_string(),
            attachment: None,
            guidance: Some("attachment_id does not belong to the provided message_id.".to_string()),
        });
    }

    let row = match crate::db::get_message_by_id(conn, message_id)? {
        Some(row) => row,
        None => {
            return Ok(GetAttachmentResponse {
                status: "not_found".to_string(),
                attachment: None,
                guidance: Some("Message not found in the index.".to_string()),
            });
        }
    };

    if let Some(mailbox_url) = row.mailbox_url.as_deref()
        && !config.is_mailbox_allowed(mailbox_url)
    {
        return Ok(GetAttachmentResponse {
            status: "error".to_string(),
            attachment: None,
            guidance: Some(
                "This attachment belongs to an account excluded by APPLE_MAIL_ACCOUNT."
                    .to_string(),
            ),
        });
    }

    let emlx_path = match locate_emlx(
        &config.mail_directory,
        &config.mail_version,
        row.mailbox_url.as_deref().unwrap_or(""),
        row.rowid,
    ) {
        Some(path) => path,
        None => {
            return Ok(GetAttachmentResponse {
                status: "not_found".to_string(),
                attachment: None,
                guidance: Some("Message body file not found on disk.".to_string()),
            });
        }
    };

    let parsed = match parse_emlx(&emlx_path) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(?error, "failed to parse message for attachment extraction");
            return Ok(GetAttachmentResponse {
                status: "error".to_string(),
                attachment: None,
                guidance: Some("Failed to parse message body file.".to_string()),
            });
        }
    };

    let raw_attachment = match parsed.attachments.get(attachment_index) {
        Some(attachment) => attachment,
        None => {
            return Ok(GetAttachmentResponse {
                status: "not_found".to_string(),
                attachment: None,
                guidance: Some(format!(
                    "Attachment index {attachment_index} out of range. Message has {} attachment(s).",
                    parsed.attachments.len()
                )),
            });
        }
    };

    let meta = AttachmentMeta {
        id: params.attachment_id.clone(),
        filename: raw_attachment
            .filename
            .clone()
            .unwrap_or_else(|| "unnamed".to_string()),
        mime_type: raw_attachment.mime_type.clone(),
        size_bytes: raw_attachment.content.len() as u64,
        is_inline: raw_attachment.is_inline,
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

    match extract_text(&raw_attachment.content, &raw_attachment.mime_type) {
        crate::mail::ExtractionResult::Text { content, method } => Ok(GetAttachmentResponse {
            status: "success".to_string(),
            attachment: Some(GetAttachmentResult {
                content_format: ContentFormat::ExtractedText,
                content: Some(content),
                extraction_method: Some(method.to_string()),
                ..base_result
            }),
            guidance: None,
        }),
        crate::mail::ExtractionResult::NotSupported { reason } => Ok(GetAttachmentResponse {
            status: "partial".to_string(),
            attachment: Some(GetAttachmentResult {
                extraction_method: Some(reason.to_string()),
                ..base_result
            }),
            guidance: Some(reason.to_string()),
        }),
    }
}

/// Execute the get_attachment_content tool.
pub fn get_attachment_content(
    config: &MailConfig,
    params: GetAttachmentParams,
) -> Result<GetAttachmentResponse, MailMcpError> {
    let db_path = config.envelope_db_path();
    let conn = crate::db::open_readonly(&db_path)?;
    get_attachment_content_with_conn(config, &conn, params)
}
