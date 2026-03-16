//! search_messages tool implementation.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use crate::config::MailConfig;
use crate::db::{
    address_exists, detect_epoch_offset_seconds, open_readonly, search_messages as db_search,
};
use crate::domain::MessageMeta;
use crate::error::MailMcpError;
use crate::mail::{locate_emlx_quick_with_hints, parse_emlx_without_attachment_content};

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

fn describe_search_filters(params: &SearchMessagesParams) -> String {
    let mut parts = Vec::new();

    if let Some(subject_query) = params.subject_query.as_deref() {
        parts.push(format!("subject_query={subject_query:?}"));
    }
    if let Some(date_from) = params.date_from.as_deref() {
        parts.push(format!("date_from={date_from}"));
    }
    if let Some(date_to) = params.date_to.as_deref() {
        parts.push(format!("date_to={date_to}"));
    }
    if let Some(sender) = params.sender.as_deref() {
        parts.push(format!("sender={sender}"));
    }
    if let Some(participant) = params.participant.as_deref() {
        parts.push(format!("participant={participant}"));
    }
    if let Some(account) = params.account.as_deref() {
        parts.push(format!("account={account}"));
    }
    if let Some(mailbox) = params.mailbox.as_deref() {
        parts.push(format!("mailbox={mailbox:?}"));
    }

    parts.push(format!(
        "include_body_preview={}",
        params.include_body_preview
    ));
    parts.push(format!("limit={}", params.limit));

    parts.join(", ")
}

#[derive(Debug, Clone, Default)]
struct SearchMetadata {
    summary: Option<String>,
    attachment_count: u32,
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
    metadata: Option<&SearchMetadata>,
) -> SearchMessageResult {
    let mut meta = MessageMeta::from_row(row, epoch_offset_s);
    meta.has_body = true;
    if let Some(metadata) = metadata {
        meta = meta.with_attachment_count(metadata.attachment_count);
        if include_body_preview && let Some(summary) = metadata.summary.as_deref() {
            let preview = preview_text(summary);
            if !preview.is_empty() {
                meta = meta.with_body_preview(preview);
            }
        }
    }

    if !include_body_preview || meta.body_preview.is_some() {
        return SearchMessageResult {
            id: meta.id,
            subject: meta.subject,
            from: meta.from,
            date_sent: meta.date_sent,
            date_received: meta.date_received,
            mailbox: meta.mailbox,
            has_body: meta.has_body,
            attachment_count: meta.attachment_count,
            body_preview: meta.body_preview,
        };
    }

    let mut numeric_hints = vec![row.rowid.to_string()];
    if let Some(global_message_id) = row.global_message_id {
        numeric_hints.push(global_message_id.to_string());
    }
    if let Some(message_id) = row.message_id.as_ref() {
        numeric_hints.push(message_id.clone());
    }
    numeric_hints.sort();
    numeric_hints.dedup();

    if let Some(mailbox_url) = row.mailbox_url.as_deref()
        && let Some(path) = locate_emlx_quick_with_hints(
            &config.mail_directory,
            &config.mail_version,
            mailbox_url,
            row.rowid,
            &numeric_hints,
            row.message_id_header
                .as_deref()
                .or(row.message_id.as_deref()),
        )
        && let Ok(parsed) = parse_emlx_without_attachment_content(&path)
        && let Some(text) = parsed.body_text.or(parsed.body_html)
    {
        let preview = preview_text(&text);
        if !preview.is_empty() {
            meta = meta.with_body_preview(preview);
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

fn load_search_metadata(
    conn: &Connection,
    message_ids: &[i64],
) -> Result<HashMap<i64, SearchMetadata>, MailMcpError> {
    if message_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = std::iter::repeat_n("?", message_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT
            m.ROWID,
            sm.summary,
            COUNT(att.ROWID)
        FROM messages m
        LEFT JOIN summaries sm ON sm.ROWID = m.summary
        LEFT JOIN attachments att ON att.message = m.ROWID
        WHERE m.ROWID IN ({placeholders})
        GROUP BY m.ROWID, sm.summary
        "#
    );

    let params: Vec<&dyn rusqlite::ToSql> = message_ids
        .iter()
        .map(|message_id| message_id as &dyn rusqlite::ToSql)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |row| {
        let attachment_count = row.get::<_, i64>(2)?;
        Ok((
            row.get::<_, i64>(0)?,
            SearchMetadata {
                summary: row.get(1)?,
                attachment_count: attachment_count.max(0) as u32,
            },
        ))
    })?;

    let mut metadata = HashMap::with_capacity(message_ids.len());
    for row in rows {
        let (message_id, entry) = row?;
        metadata.insert(message_id, entry);
    }

    Ok(metadata)
}

/// Execute `search_messages` against an already-open SQLite connection.
pub fn search_messages_with_conn(
    config: &MailConfig,
    conn: &Connection,
    params: SearchMessagesParams,
) -> Result<SearchMessagesResponse, MailMcpError> {
    let total_started = Instant::now();
    let filters_description = describe_search_filters(&params);
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
    if let Some(account) = params.account.as_deref()
        && !config.is_account_allowed(account)
    {
        return Ok(SearchMessagesResponse {
            status: "error".to_string(),
            guidance: Some(format!(
                "The requested account filter {account} is excluded by APPLE_MAIL_ACCOUNT."
            )),
            messages: Vec::new(),
            total_count: 0,
            has_more: false,
            next_offset: None,
        });
    }

    let sql_started = Instant::now();
    let rows = db_search(
        conn,
        params.subject_query.as_deref(),
        date_from_ts,
        date_to_ts,
        params.sender.as_deref(),
        params.participant.as_deref(),
        params.account.as_deref(),
        config.allowed_account_ids(),
        params.mailbox.as_deref(),
        params.limit,
        0,
    )?;
    let sql_elapsed = sql_started.elapsed();

    let metadata_started = Instant::now();
    let message_ids = rows.iter().map(|row| row.rowid).collect::<Vec<_>>();
    let search_metadata = load_search_metadata(conn, &message_ids)?;
    let metadata_elapsed = metadata_started.elapsed();

    if rows.is_empty() {
        tracing::debug!(
            "search_messages completed: 0 result(s), sql={} ms, metadata={} ms, hydration=0 ms, total={} ms; filters: {}",
            sql_elapsed.as_millis(),
            metadata_elapsed.as_millis(),
            total_started.elapsed().as_millis(),
            filters_description,
        );
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

    let hydration_started = Instant::now();
    let messages = rows
        .iter()
        .map(|row| {
            hydrate_search_result(
                config,
                row,
                epoch_offset_s,
                params.include_body_preview,
                search_metadata.get(&row.rowid),
            )
        })
        .collect::<Vec<_>>();
    let hydration_elapsed = hydration_started.elapsed();

    let has_more = rows.len() as u32 >= params.limit;
    tracing::debug!(
        "search_messages completed: {} result(s), sql={} ms, metadata={} ms, hydration={} ms, total={} ms; filters: {}",
        messages.len(),
        sql_elapsed.as_millis(),
        metadata_elapsed.as_millis(),
        hydration_elapsed.as_millis(),
        total_started.elapsed().as_millis(),
        filters_description,
    );
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
            allowed_account_ids: None,
            account_metadata: Default::default(),
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

    #[test]
    fn describe_search_filters_formats_only_present_values() {
        let params = SearchMessagesParams {
            subject_query: Some("invoice".to_string()),
            date_from: Some("2026-03-16".to_string()),
            date_to: None,
            sender: None,
            participant: Some("user@example.com".to_string()),
            account: Some("ews://account-b".to_string()),
            mailbox: Some("Inbox".to_string()),
            limit: 50,
            include_body_preview: true,
        };

        let description = describe_search_filters(&params);
        assert!(description.contains("subject_query=\"invoice\""));
        assert!(description.contains("date_from=2026-03-16"));
        assert!(description.contains("participant=user@example.com"));
        assert!(description.contains("account=ews://account-b"));
        assert!(description.contains("mailbox=\"Inbox\""));
        assert!(description.contains("include_body_preview=true"));
        assert!(description.contains("limit=50"));
        assert!(!description.contains("date_to="));
        assert!(!description.contains("sender="));
    }

    #[test]
    fn validate_params_rejects_no_filters() {
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

        let result = validate_params(&params);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("At least one filter must be provided")
        );
    }

    #[test]
    fn validate_params_rejects_high_limit() {
        let params = SearchMessagesParams {
            subject_query: Some("test".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 101,
            include_body_preview: false,
        };

        let result = validate_params(&params);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("limit must be between 1 and 100")
        );
    }

    #[test]
    fn parse_date_range_handles_invalid_format() {
        let params = SearchMessagesParams {
            subject_query: None,
            date_from: Some("invalid-date".to_string()),
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        };

        let result = parse_date_range(&params);
        assert!(result.is_err());
    }

    #[test]
    fn parse_date_range_with_both_dates() {
        let params = SearchMessagesParams {
            subject_query: Some("test".to_string()),
            date_from: Some("2026-03-15".to_string()),
            date_to: Some("2026-03-16".to_string()),
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            include_body_preview: false,
        };

        let result = parse_date_range(&params);
        assert!(result.is_ok());
        let (from_ts, to_ts) = result.unwrap();
        assert!(from_ts.is_some());
        assert!(to_ts.is_some());
    }
}
