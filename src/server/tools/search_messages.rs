//! search_messages tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::MailConfig;
use crate::db::{
    address_exists, detect_epoch_offset_seconds, open_readonly, search_messages as db_search,
};
use crate::domain::MessageMeta;
use crate::error::MailMcpError;
use crate::mail::{locate_emlx_quick, parse_emlx};

/// Parameters for the search_messages tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchMessagesParams {
    /// Text to search in subject (partial match, case-insensitive)
    pub subject_query: Option<String>,
    /// Start of date range (YYYY-MM-DD, inclusive)
    pub date_from: Option<String>,
    /// End of date range (YYYY-MM-DD, inclusive)
    pub date_to: Option<String>,
    /// Sender email address (exact match)
    pub sender: Option<String>,
    /// Recipient participant email address (To/CC exact match)
    pub participant: Option<String>,
    /// Account identifier returned by list_accounts (for example, `ews://account-id`)
    pub account: Option<String>,
    /// Mailbox name or fragment
    pub mailbox: Option<String>,
    /// Maximum number of results (default 20, max 100)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Include ~200 character body preview
    #[serde(default)]
    pub include_body_preview: bool,
}

fn default_limit() -> u32 {
    20
}

/// Response message item for search_results.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchMessageResult {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub date_sent: Option<String>,
    pub date_received: Option<String>,
    pub mailbox: String,
    pub has_body: bool,
    pub attachment_count: u32,
    pub body_preview: Option<String>,
}

/// Response for search_messages tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchMessagesResponse {
    pub status: String,
    pub messages: Vec<SearchMessageResult>,
    pub total_count: u32,
    pub has_more: bool,
    pub next_offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

/// Parse a date string (YYYY-MM-DD) to Unix timestamp (start of day UTC).
fn parse_date(date_str: &str) -> Option<i64> {
    use chrono::NaiveDate;
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

fn preview_text(body: &str) -> String {
    body.trim().chars().take(200).collect()
}

fn validate_params(params: &SearchMessagesParams) -> Result<(), String> {
    let has_any_filter = params.subject_query.is_some()
        || params.date_from.is_some()
        || params.date_to.is_some()
        || params.sender.is_some()
        || params.participant.is_some()
        || params.account.is_some()
        || params.mailbox.is_some();

    if !has_any_filter {
        return Err(
            "At least one filter must be provided: subject_query, date_from, date_to, sender, participant, account, or mailbox.".to_string(),
        );
    }

    if let Some(limit) = (params.limit > 100).then_some(params.limit) {
        return Err(format!("limit must be between 1 and 100, got {limit}"));
    }

    Ok(())
}

fn parse_date_range(params: &SearchMessagesParams) -> Result<(Option<i64>, Option<i64>), String> {
    let date_from_ts = match params.date_from.as_deref() {
        Some(date) => Some(
            parse_date(date)
                .ok_or_else(|| format!("Invalid date_from format: {date}. Expected YYYY-MM-DD"))?,
        ),
        None => None,
    };

    let date_to_ts = match params.date_to.as_deref() {
        Some(date_str) => {
            use chrono::NaiveDate;
            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map_err(|_| format!("Invalid date_to format: {date_str}. Expected YYYY-MM-DD"))?;
            Some(
                date.and_hms_opt(23, 59, 59)
                    .ok_or_else(|| format!("Could not compute end-of-day for {date_str}"))?
                    .and_utc()
                    .timestamp(),
            )
        }
        None => None,
    };

    Ok((date_from_ts, date_to_ts))
}

fn hydrate_search_result(
    config: &MailConfig,
    row: &crate::db::MessageRow,
    epoch_offset_s: i64,
    include_body_preview: bool,
) -> SearchMessageResult {
    let mut meta = MessageMeta::from_row(row, epoch_offset_s);
    meta.has_body = true;

    if include_body_preview
        && let Some(mailbox_url) = row.mailbox_url.as_deref()
        && let Some(path) = locate_emlx_quick(
            &config.mail_directory,
            &config.mail_version,
            mailbox_url,
            row.rowid,
        )
        && let Ok(parsed) = parse_emlx(&path)
    {
        meta = meta.with_attachment_count(parsed.attachments.len() as u32);
        if let Some(text) = parsed.body_text.or(parsed.body_html) {
            let preview = preview_text(&text);
            if !preview.is_empty() {
                meta = meta.with_body_preview(preview);
            }
        }
    }

    SearchMessageResult {
        id: meta.id,
        subject: meta.subject,
        from: meta.from,
        date_sent: meta.date_sent,
        date_received: meta.date_received,
        mailbox: meta.mailbox,
        has_body: meta.has_body,
        attachment_count: meta.attachment_count,
        body_preview: meta.body_preview,
    }
}

/// Execute `search_messages` against an already-open SQLite connection.
pub fn search_messages_with_conn(
    config: &MailConfig,
    conn: &Connection,
    params: SearchMessagesParams,
) -> Result<SearchMessagesResponse, MailMcpError> {
    if let Err(message) = validate_params(&params) {
        return Ok(SearchMessagesResponse {
            status: "error".to_string(),
            guidance: Some(message),
            messages: Vec::new(),
            total_count: 0,
            has_more: false,
            next_offset: None,
        });
    }

    let (date_from_ts, date_to_ts) = match parse_date_range(&params) {
        Ok(range) => range,
        Err(message) => {
            return Ok(SearchMessagesResponse {
                status: "error".to_string(),
                guidance: Some(message),
                messages: Vec::new(),
                total_count: 0,
                has_more: false,
                next_offset: None,
            });
        }
    };

    let epoch_offset_s = detect_epoch_offset_seconds(conn)?;
    let rows = db_search(
        conn,
        params.subject_query.as_deref(),
        date_from_ts,
        date_to_ts,
        params.sender.as_deref(),
        params.participant.as_deref(),
        params.account.as_deref(),
        params.mailbox.as_deref(),
        params.limit,
        0,
    )?;

    if rows.is_empty() {
        let guidance = if let Some(sender) = params.sender.as_deref() {
            if !address_exists(conn, sender)? {
                format!("Sender address {sender} is not present in Apple Mail's address index.")
            } else {
                "No messages match the provided filters. Try broadening the date range or shortening subject_query to one or two keywords.".to_string()
            }
        } else if let Some(participant) = params.participant.as_deref() {
            if !address_exists(conn, participant)? {
                format!(
                    "Participant address {participant} is not present in Apple Mail's address index."
                )
            } else {
                "No messages match the provided filters. Try broadening the date range or changing the mailbox filter.".to_string()
            }
        } else {
            "No messages match the provided filters. Try broadening the date range, shortening subject_query to one or two keywords, or verifying the sender address with list_mailboxes.".to_string()
        };

        return Ok(SearchMessagesResponse {
            status: "not_found".to_string(),
            guidance: Some(guidance),
            messages: Vec::new(),
            total_count: 0,
            has_more: false,
            next_offset: None,
        });
    }

    let messages = rows
        .iter()
        .map(|row| hydrate_search_result(config, row, epoch_offset_s, params.include_body_preview))
        .collect::<Vec<_>>();

    let has_more = rows.len() as u32 >= params.limit;
    Ok(SearchMessagesResponse {
        status: "success".to_string(),
        total_count: messages.len() as u32,
        has_more,
        next_offset: has_more.then_some(params.limit),
        guidance: None,
        messages,
    })
}

/// Execute the search_messages tool.
pub fn search_messages(
    config: &MailConfig,
    params: SearchMessagesParams,
) -> Result<SearchMessagesResponse, MailMcpError> {
    if let Err(message) = validate_params(&params) {
        return Ok(SearchMessagesResponse {
            status: "error".to_string(),
            guidance: Some(message),
            messages: Vec::new(),
            total_count: 0,
            has_more: false,
            next_offset: None,
        });
    }

    if let Err(message) = parse_date_range(&params) {
        return Ok(SearchMessagesResponse {
            status: "error".to_string(),
            guidance: Some(message),
            messages: Vec::new(),
            total_count: 0,
            has_more: false,
            next_offset: None,
        });
    }

    let db_path = config.envelope_db_path();
    let conn = open_readonly(&db_path)?;
    search_messages_with_conn(config, &conn, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn search_with_no_filters_returns_error() {
        let config = MailConfig {
            mail_directory: PathBuf::from("/tmp"),
            mail_version: "V10".to_string(),
            primary_email: "test@example.com".to_string(),
        };

        let params = SearchMessagesParams {
            subject_query: None,
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        };

        let result = search_messages(&config, params).unwrap();
        assert_eq!(result.status, "error");
        assert!(result.guidance.is_some());
    }
}
