//! get_message tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::db::{
    detect_epoch_offset_seconds, get_message_by_id, get_recipients, mailbox_account_id,
    open_readonly,
};
use crate::domain::AttachmentMeta;
use crate::error::MailMcpError;
use crate::mail::{locate_emlx_with_hints, parse_emlx, raw_attachments_to_meta};

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
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<GetMessageResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Message result in get_message response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetMessageResult {
    pub id: String,
    pub message_id_header: Option<String>,
    pub subject: String,
    pub from: String,
    pub to: Vec<String>,
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
    let message_id: i64 = match params.message_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(GetMessageResponse {
                status: "error".to_string(),
                message: None,
                guidance: Some(
                    "Invalid message_id format. Expected a numeric ID from search results."
                        .to_string(),
                ),
            });
        }
    };

    let epoch_offset_s = detect_epoch_offset_seconds(conn)?;
    let row = match get_message_by_id(conn, message_id)? {
        Some(row) => row,
        None => {
            return Ok(GetMessageResponse {
                status: "not_found".to_string(),
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
            status: "error".to_string(),
            message: None,
            guidance: Some(
                "This message belongs to an account excluded by APPLE_MAIL_ACCOUNT."
                    .to_string(),
            ),
        });
    }

    let recipients = get_recipients(conn, message_id)?;
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
        to,
        cc,
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

        if let Some(path) = emlx_path {
            match parse_emlx(&path) {
                Ok(parsed) => {
                    if params.include_body {
                        result.body = match params.body_format {
                            BodyFormat::Text => parsed.body_text.clone(),
                            BodyFormat::Html => parsed.body_html.clone(),
                            BodyFormat::Both => {
                                parsed.body_text.clone().or(parsed.body_html.clone())
                            }
                        };

                        if matches!(params.body_format, BodyFormat::Both) {
                            result.body_html = parsed.body_html.clone();
                        }
                    }

                    if params.include_attachments_summary {
                        result.attachments =
                            raw_attachments_to_meta(row.rowid, &parsed.attachments);
                    }
                }
                Err(MailMcpError::BodyFileNotFound { .. }) => {
                    return Ok(GetMessageResponse {
                        status: "partial".to_string(),
                        message: Some(result),
                        guidance: Some(
                            "Message body file not found on disk (emlx missing). The message index entry exists but the local file may have been deleted or not yet downloaded. Try another message or check Mail sync status.".to_string(),
                        ),
                    });
                }
                Err(error) => {
                    tracing::warn!(?error, "failed to parse emlx");
                    return Ok(GetMessageResponse {
                        status: "partial".to_string(),
                        message: Some(result),
                        guidance: Some(
                            "Message metadata was loaded, but the body could not be parsed from the local message file.".to_string(),
                        ),
                    });
                }
            }
        } else {
            return Ok(GetMessageResponse {
                status: "partial".to_string(),
                message: Some(result),
                guidance: Some(
                    "No local message file matched this message inside the mailbox subtree. The message may not be downloaded, may only exist as a partial cache entry, or the local Mail storage layout may differ from the indexed metadata.".to_string(),
                ),
            });
        }
    }

    Ok(GetMessageResponse {
        status: "success".to_string(),
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
