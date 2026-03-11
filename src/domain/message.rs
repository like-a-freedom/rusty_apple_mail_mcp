//! Domain types for email messages.

use chrono::{DateTime, TimeZone, Utc};
use schemars::JsonSchema;
use serde::Serialize;

use crate::db::MessageRow;
use crate::domain::attachment::AttachmentMeta;

/// Convert database integer timestamp to ISO 8601 string.
///
/// `epoch_offset_s` should be `0` for Unix timestamps or `978_307_200`
/// for CoreData timestamps.
///
/// # Arguments
///
/// * `ts` - Timestamp from the database
/// * `epoch_offset_s` - Seconds to add before formatting
///
/// # Returns
///
/// ISO 8601 formatted string (RFC 3339)
pub fn timestamp_to_iso(ts: i64, epoch_offset_s: i64) -> String {
    let unix_ts = ts + epoch_offset_s;
    Utc.timestamp_opt(unix_ts, 0)
        .single()
        .map(|dt: DateTime<Utc>| dt.to_rfc3339())
        .unwrap_or_else(|| format!("invalid_ts:{ts}"))
}

/// Compact message representation for search result lists.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MessageMeta {
    /// Stable message identifier (database ROWID as string)
    pub id: String,
    /// Email subject line
    pub subject: String,
    /// Sender email address
    pub from: String,
    /// Date/time when the message was sent (ISO 8601)
    pub date_sent: Option<String>,
    /// Date/time when the message was received (ISO 8601)
    pub date_received: Option<String>,
    /// Mailbox name (extracted from mailbox URL)
    pub mailbox: String,
    /// Whether the message has a body available
    pub has_body: bool,
    /// Number of attachments
    pub attachment_count: u32,
    /// Preview of the body text (~200 characters), if requested
    pub body_preview: Option<String>,
}

impl MessageMeta {
    /// Convert a database row to MessageMeta.
    pub fn from_row(row: &MessageRow, epoch_offset_s: i64) -> Self {
        let mailbox = row
            .mailbox_url
            .as_ref()
            .map(|url| {
                // Extract the last segment of the URL as the mailbox name
                url.rsplit('/').next().unwrap_or(url).to_string()
            })
            .unwrap_or_else(|| "Unknown".to_string());

        Self {
            id: row.rowid.to_string(),
            subject: row.subject.clone().unwrap_or_default(),
            from: row.sender.clone().unwrap_or_default(),
            date_sent: row.date_sent.map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            date_received: row.date_received.map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            mailbox,
            has_body: true,      // Assume true; actual check happens when reading emlx
            attachment_count: 0, // Will be populated from emlx parsing
            body_preview: None,
        }
    }

    /// Set body preview text.
    pub fn with_body_preview(mut self, preview: impl Into<String>) -> Self {
        self.body_preview = Some(preview.into());
        self
    }

    /// Set attachment count.
    pub fn with_attachment_count(mut self, count: u32) -> Self {
        self.attachment_count = count;
        self
    }
}

/// Full message representation for detailed retrieval.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MessageFull {
    /// Stable message identifier (database ROWID as string)
    pub id: String,
    /// Message-ID header value (for email threading)
    pub message_id_header: Option<String>,
    /// Email subject line
    pub subject: String,
    /// Sender email address
    pub from: String,
    /// Recipient email addresses (To)
    pub to: Vec<String>,
    /// CC recipient email addresses
    pub cc: Vec<String>,
    /// Date/time when the message was sent (ISO 8601)
    pub date_sent: Option<String>,
    /// Date/time when the message was received (ISO 8601)
    pub date_received: Option<String>,
    /// Mailbox name (extracted from mailbox URL)
    pub mailbox: String,
    /// Message body text (format depends on request)
    pub body: Option<String>,
    /// Attachment metadata
    pub attachments: Vec<AttachmentMeta>,
}

impl MessageFull {
    /// Create a MessageFull from a database row and recipients.
    pub fn from_row_with_recipients(
        row: &MessageRow,
        recipients: &[(String, i32)],
        epoch_offset_s: i64,
    ) -> Self {
        let mailbox = row
            .mailbox_url
            .as_ref()
            .map(|url| url.rsplit('/').next().unwrap_or(url).to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        // Split recipients by type: 1=To, 2=CC, 3=BCC
        let mut to = Vec::new();
        let mut cc = Vec::new();
        for (addr, type_) in recipients {
            match type_ {
                1 => to.push(addr.clone()),
                2 => cc.push(addr.clone()),
                _ => {} // Ignore BCC
            }
        }

        Self {
            id: row.rowid.to_string(),
            message_id_header: row.message_id_header.clone().or(row.message_id.clone()),
            subject: row.subject.clone().unwrap_or_default(),
            from: row.sender.clone().unwrap_or_default(),
            to,
            cc,
            date_sent: row.date_sent.map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            date_received: row.date_received.map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            mailbox,
            body: None,
            attachments: Vec::new(),
        }
    }

    /// Set the message body.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set attachments.
    pub fn with_attachments(mut self, attachments: Vec<AttachmentMeta>) -> Self {
        self.attachments = attachments;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::COREDATA_EPOCH_OFFSET;

    #[test]
    fn timestamp_to_iso_converts_coredata_epoch() {
        // 2024-09-15 00:00:00 UTC
        // Unix timestamp: 1726358400
        // CoreData timestamp = Unix - COREDATA_EPOCH_OFFSET = 1726358400 - 978307200 = 748051200
        let ts = 748051200;
        let iso = timestamp_to_iso(ts, COREDATA_EPOCH_OFFSET);
        println!("Converted timestamp: {}", iso);
        assert!(iso.contains("2024-09-15"));
    }

    #[test]
    fn timestamp_to_iso_handles_invalid_timestamp() {
        // Very negative timestamps will produce dates far in the past
        // but chrono is permissive, so we just check it produces a valid ISO string
        let iso = timestamp_to_iso(-999999999999, COREDATA_EPOCH_OFFSET);
        // Should still produce a valid ISO 8601 string (contains 'T' and timezone)
        assert!(iso.contains('T'));
        assert!(iso.contains('+') || iso.contains('-'));
    }

    #[test]
    fn message_meta_from_row() {
        let row = MessageRow {
            rowid: 42,
            subject: Some("Test Subject".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: Some("imap://user@mail.example.com/INBOX".to_string()),
            date_sent: Some(2704665600),
            date_received: Some(2704665600),
            message_id: Some("<test@mail>".to_string()),
            global_message_id: Some(7),
            message_id_header: Some("<test@mail>".to_string()),
        };

        let meta = MessageMeta::from_row(&row, COREDATA_EPOCH_OFFSET);
        assert_eq!(meta.id, "42");
        assert_eq!(meta.subject, "Test Subject");
        assert_eq!(meta.from, "sender@example.com");
        assert_eq!(meta.mailbox, "INBOX");
    }
}
