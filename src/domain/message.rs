//! Domain types for email messages.

use chrono::{DateTime, TimeZone, Utc};
use schemars::JsonSchema;
use serde::Serialize;

use crate::db::MessageRow;
use crate::domain::attachment::AttachmentMeta;

/// Convert database integer timestamp to ISO 8601 string.
///
/// `epoch_offset_s` should be `0` for Unix timestamps or `978_307_200`
/// for `CoreData` timestamps.
///
/// # Arguments
///
/// * `ts` - Timestamp from the database
/// * `epoch_offset_s` - Seconds to add before formatting
///
/// # Returns
///
/// Compact ISO 8601 string without seconds (e.g. `2024-09-15T00:00Z`)
#[must_use]
pub fn timestamp_to_iso(ts: i64, epoch_offset_s: i64) -> String {
    let unix_ts = ts + epoch_offset_s;
    Utc.timestamp_opt(unix_ts, 0).single().map_or_else(
        || format!("invalid_ts:{ts}"),
        |dt: DateTime<Utc>| dt.format("%Y-%m-%dT%H:%MZ").to_string(),
    )
}

/// Extract display name from a mailbox URL with percent-decoding.
///
/// Extracts the last path component and decodes percent-encoded characters:
/// - `imap://account-id/INBOX` → `INBOX`
/// - `ews://account-id/Sent%20Items` → `Sent Items`
/// - `imap://account-id/folder.mbox` → `folder`
#[must_use]
pub fn extract_mailbox_name(url: &str) -> String {
    let raw = url
        .rsplit('/')
        .next()
        .unwrap_or(url)
        .trim_end_matches(".mbox");
    percent_decode(raw)
}

/// Minimal percent-decoder for URL path segments.
///
/// Decodes `%XX` sequences (case-insensitive hex) and `+` to space.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_digit(bytes[i + 1]);
                let lo = hex_digit(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    result.push(h * 16 + l);
                    i += 3;
                    continue;
                }
                result.push(bytes[i]);
            }
            b'+' => {
                result.push(b' ');
            }
            byte => {
                result.push(byte);
            }
        }
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
    /// Number of attachments
    pub attachment_count: u32,
    /// Preview of the body text (~200 characters), if requested
    pub body_preview: Option<String>,
}

impl MessageMeta {
    /// Convert a database row to `MessageMeta`.
    #[must_use]
    pub fn from_row(row: &MessageRow, epoch_offset_s: i64) -> Self {
        let mailbox = row
            .mailbox_url
            .as_deref()
            .map(extract_mailbox_name)
            .unwrap_or_else(|| "Unknown".to_string());

        Self {
            id: row.rowid.to_string(),
            subject: row.subject.clone().unwrap_or_default(),
            from: row.sender.clone().unwrap_or_default(),
            date_sent: row.date_sent.map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            date_received: row
                .date_received
                .map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            mailbox,
            attachment_count: 0, // Will be populated from emlx parsing
            body_preview: None,
        }
    }

    /// Set body preview text.
    #[must_use]
    pub fn with_body_preview(mut self, preview: impl Into<String>) -> Self {
        self.body_preview = Some(preview.into());
        self
    }

    /// Set attachment count.
    #[must_use]
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
    /// Create a `MessageFull` from a database row and recipients.
    #[must_use]
    pub fn from_row_with_recipients(
        row: &MessageRow,
        recipients: &[(String, i32)],
        epoch_offset_s: i64,
    ) -> Self {
        let mailbox = row
            .mailbox_url
            .as_deref()
            .map(extract_mailbox_name)
            .unwrap_or_else(|| "Unknown".to_string());

        // Split recipients by Apple Mail recipient code: 0=To, 1=CC.
        let mut to = Vec::new();
        let mut cc = Vec::new();
        for (addr, type_) in recipients {
            match type_ {
                0 => to.push(addr.clone()),
                1 => cc.push(addr.clone()),
                _ => {}
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
            date_received: row
                .date_received
                .map(|ts| timestamp_to_iso(ts, epoch_offset_s)),
            mailbox,
            body: None,
            attachments: Vec::new(),
        }
    }

    /// Set the message body.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set attachments.
    #[must_use]
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
        // Should still produce a valid ISO 8601 string (contains 'T')
        assert!(iso.contains('T'));
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

    #[test]
    fn message_full_from_row_with_recipients() {
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

        let recipients = vec![
            ("to1@example.com".to_string(), 0),
            ("to2@example.com".to_string(), 0),
            ("cc1@example.com".to_string(), 1),
            ("ignored@example.com".to_string(), 9),
        ];

        let full = MessageFull::from_row_with_recipients(&row, &recipients, COREDATA_EPOCH_OFFSET);
        assert_eq!(full.id, "42");
        assert_eq!(full.subject, "Test Subject");
        assert_eq!(full.from, "sender@example.com");
        assert_eq!(full.mailbox, "INBOX");
        assert_eq!(full.to.len(), 2);
        assert_eq!(full.cc.len(), 1);
        assert!(full.to.contains(&"to1@example.com".to_string()));
        assert!(full.to.contains(&"to2@example.com".to_string()));
        assert!(full.cc.contains(&"cc1@example.com".to_string()));
        assert!(!full.to.contains(&"ignored@example.com".to_string()));
    }

    #[test]
    fn message_full_with_body() {
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

        let full = MessageFull::from_row_with_recipients(&row, &[], COREDATA_EPOCH_OFFSET)
            .with_body("Test body content");
        assert_eq!(full.body, Some("Test body content".to_string()));
    }

    #[test]
    fn message_full_with_attachments() {
        use crate::domain::attachment::AttachmentMeta;

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

        let attachments = vec![AttachmentMeta {
            id: "42:0".to_string(),
            filename: "document.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 512,
        }];

        let full = MessageFull::from_row_with_recipients(&row, &[], COREDATA_EPOCH_OFFSET)
            .with_attachments(attachments.clone());
        assert_eq!(full.attachments.len(), 1);
        assert_eq!(full.attachments[0].filename, "document.pdf");
    }

    #[test]
    fn message_full_with_empty_recipients() {
        let row = MessageRow {
            rowid: 42,
            subject: Some("Test Subject".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: None,
            date_sent: None,
            date_received: None,
            message_id: None,
            global_message_id: None,
            message_id_header: None,
        };

        let full = MessageFull::from_row_with_recipients(&row, &[], COREDATA_EPOCH_OFFSET);
        assert_eq!(full.mailbox, "Unknown");
        assert_eq!(full.to.len(), 0);
        assert_eq!(full.cc.len(), 0);
        assert_eq!(full.body, None);
        assert_eq!(full.attachments.len(), 0);
    }

    #[test]
    fn message_full_with_cc_and_bcc_recipients() {
        let row = MessageRow {
            rowid: 42,
            subject: Some("Test Subject".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: Some("imap://test/INBOX".to_string()),
            date_sent: Some(748051200),
            date_received: Some(748051200),
            message_id: Some("<test@mail>".to_string()),
            global_message_id: Some(7),
            message_id_header: Some("<test@mail>".to_string()),
        };

        // Recipients: (address, type) where Apple Mail uses 0=To, 1=CC.
        let recipients = vec![
            ("to1@example.com".to_string(), 0),
            ("to2@example.com".to_string(), 0),
            ("cc1@example.com".to_string(), 1),
            ("cc2@example.com".to_string(), 1),
            ("ignored@example.com".to_string(), 9),
        ];

        let full = MessageFull::from_row_with_recipients(&row, &recipients, COREDATA_EPOCH_OFFSET);

        assert_eq!(full.to.len(), 2);
        assert!(full.to.contains(&"to1@example.com".to_string()));
        assert!(full.to.contains(&"to2@example.com".to_string()));

        assert_eq!(full.cc.len(), 2);
        assert!(full.cc.contains(&"cc1@example.com".to_string()));
        assert!(full.cc.contains(&"cc2@example.com".to_string()));
    }

    #[test]
    fn message_full_maps_apple_mail_recipient_types_zero_and_one() {
        let row = MessageRow {
            rowid: 42,
            subject: Some("Test Subject".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: Some("imap://test/INBOX".to_string()),
            date_sent: Some(748051200),
            date_received: Some(748051200),
            message_id: Some("<test@mail>".to_string()),
            global_message_id: Some(7),
            message_id_header: Some("<test@mail>".to_string()),
        };

        let recipients = vec![
            ("to@example.com".to_string(), 0),
            ("cc@example.com".to_string(), 1),
        ];

        let full = MessageFull::from_row_with_recipients(&row, &recipients, COREDATA_EPOCH_OFFSET);

        assert_eq!(full.to, vec!["to@example.com".to_string()]);
        assert_eq!(full.cc, vec!["cc@example.com".to_string()]);
    }

    #[test]
    fn message_meta_with_body_preview_and_attachment_count_chain() {
        let row = MessageRow {
            rowid: 42,
            subject: Some("Test".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: Some("imap://test/INBOX".to_string()),
            date_sent: None,
            date_received: None,
            message_id: None,
            global_message_id: None,
            message_id_header: None,
        };

        let meta = MessageMeta::from_row(&row, 0)
            .with_body_preview("Preview text")
            .with_attachment_count(3);

        assert_eq!(meta.body_preview, Some("Preview text".to_string()));
        assert_eq!(meta.attachment_count, 3);
    }

    #[test]
    fn message_full_with_body_and_attachments_chain() {
        let row = MessageRow {
            rowid: 42,
            subject: Some("Test".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: Some("imap://test/INBOX".to_string()),
            date_sent: None,
            date_received: None,
            message_id: None,
            global_message_id: None,
            message_id_header: None,
        };

        let attachment = AttachmentMeta {
            id: "42:0".to_string(),
            filename: "test.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 1024,
        };

        let full = MessageFull::from_row_with_recipients(&row, &[], 0)
            .with_body("Test body content")
            .with_attachments(vec![attachment.clone()]);

        assert_eq!(full.body, Some("Test body content".to_string()));
        assert_eq!(full.attachments.len(), 1);
        assert_eq!(full.attachments[0].filename, "test.pdf");
    }

    #[test]
    fn extract_mailbox_name_plain_inbox() {
        assert_eq!(extract_mailbox_name("imap://account-id/INBOX"), "INBOX");
    }

    #[test]
    fn extract_mailbox_name_percent_encoded_spaces() {
        assert_eq!(
            extract_mailbox_name("ews://account-id/Sent%20Items"),
            "Sent Items"
        );
    }

    #[test]
    fn extract_mailbox_name_plus_to_space() {
        assert_eq!(
            extract_mailbox_name("imap://account-id/Drafts+Folder"),
            "Drafts Folder"
        );
    }

    #[test]
    fn extract_mailbox_name_mbox_suffix_stripped() {
        assert_eq!(
            extract_mailbox_name("imap://account-id/folder.mbox"),
            "folder"
        );
    }

    #[test]
    fn extract_mailbox_name_nested_subfolder() {
        assert_eq!(
            extract_mailbox_name("ews://uuid/Parent/Sub%20Folder"),
            "Sub Folder"
        );
    }

    #[test]
    fn extract_mailbox_name_no_path() {
        assert_eq!(extract_mailbox_name("imap://account-id"), "account-id");
    }

    #[test]
    fn percent_decode_leaves_malformed_percent_as_literal() {
        // %2g — 'g' is not a valid hex digit, so %2g is left as literal bytes
        assert_eq!(percent_decode("foo%2gbar"), "foo%2gbar");
    }

    #[test]
    fn percent_decode_leaves_invalid_hex_as_literal() {
        assert_eq!(percent_decode("foo%ZZbar"), "foo%ZZbar");
    }

    #[test]
    fn percent_decode_handles_double_percent() {
        assert_eq!(percent_decode("foo%%20bar"), "foo% bar");
    }

    #[test]
    fn percent_decode_empty_string() {
        assert_eq!(percent_decode(""), "");
    }
}
