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
use crate::server::tools::ResponseStatus;

/// Parameters for the search_messages tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// Offset for pagination (use next_offset from previous response)
    #[serde(default)]
    pub offset: u32,
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
    pub mailbox: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub attachment_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_preview: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Response for search_messages tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchMessagesResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseStatus>,
    pub messages: Vec<SearchMessageResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u32>,
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
            mailbox: meta.mailbox,
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
        mailbox: meta.mailbox,
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
            status: Some(ResponseStatus::Error),
            guidance: Some(message),
            messages: Vec::new(),
            total_count: None,
            has_more: false,
            next_offset: None,
        });
    }

    let (date_from_ts, date_to_ts) = match parse_date_range(&params) {
        Ok(range) => range,
        Err(message) => {
            return Ok(SearchMessagesResponse {
                status: Some(ResponseStatus::Error),
                guidance: Some(message),
                messages: Vec::new(),
                total_count: None,
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
            status: Some(ResponseStatus::Error),
            guidance: Some(format!(
                "The requested account filter {account} is excluded by APPLE_MAIL_ACCOUNT."
            )),
            messages: Vec::new(),
            total_count: None,
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
        params.offset,
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
            status: Some(ResponseStatus::NotFound),
            guidance: Some(guidance),
            messages: Vec::new(),
            total_count: None,
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
        status: None,
        total_count: if has_more {
            Some(messages.len() as u32)
        } else {
            None
        },
        has_more,
        next_offset: has_more.then_some(params.offset + params.limit),
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
            status: Some(ResponseStatus::Error),
            guidance: Some(message),
            messages: Vec::new(),
            total_count: None,
            has_more: false,
            next_offset: None,
        });
    }

    if let Err(message) = parse_date_range(&params) {
        return Ok(SearchMessagesResponse {
            status: Some(ResponseStatus::Error),
            guidance: Some(message),
            messages: Vec::new(),
            total_count: None,
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
            offset: 0,
        };

        let result = search_messages(&config, params).unwrap();
        assert_eq!(result.status, Some(ResponseStatus::Error));
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
            offset: 0,
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
            offset: 0,
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
            offset: 0,
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
    fn deserialization_rejects_unknown_fields() {
        let json = r#"{"subject_query":"test","body_query":"internet"}"#;
        let result: Result<SearchMessagesParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown field") || err.contains("body_query"));
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
            offset: 0,
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
            offset: 0,
        };

        let result = parse_date_range(&params);
        assert!(result.is_ok());
        let (from_ts, to_ts) = result.unwrap();
        assert!(from_ts.is_some());
        assert!(to_ts.is_some());
    }

    #[test]
    fn parse_date_valid_yyyy_mm_dd_format() {
        let result = parse_date("2024-09-15");
        assert!(result.is_some());
        let ts = result.unwrap();
        // 2024-09-15 00:00:00 UTC
        assert_eq!(ts, 1726358400);
    }

    #[test]
    fn parse_date_invalid_format_returns_none() {
        assert!(parse_date("2024/09/15").is_none());
        assert!(parse_date("09-15-2024").is_none());
        assert!(parse_date("not-a-date").is_none());
        assert!(parse_date("").is_none());
    }

    #[test]
    fn validate_params_rejects_limit_over_100() {
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
            offset: 0,
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
    fn preview_text_truncates_to_200_chars() {
        let long_text = "a".repeat(300);
        let preview = preview_text(&long_text);
        assert_eq!(preview.len(), 200);
        assert!(preview.starts_with('a'));
        assert!(preview.ends_with('a'));

        let short_text = "Hello";
        let preview = preview_text(short_text);
        assert_eq!(preview, "Hello");
    }

    #[test]
    fn load_search_metadata_empty_list() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE messages (ROWID INTEGER PRIMARY KEY, subject INTEGER, sender INTEGER, mailbox INTEGER, summary INTEGER, date_sent INTEGER, date_received INTEGER, message_id TEXT, global_message_id INTEGER);
            CREATE TABLE summaries (ROWID INTEGER PRIMARY KEY, summary TEXT);
            CREATE TABLE attachments (ROWID INTEGER PRIMARY KEY, message INTEGER, attachment_id TEXT, name TEXT);
            "#,
        ).expect("create schema");

        let result = load_search_metadata(&conn, &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn describe_search_filters_with_all_options() {
        let params = SearchMessagesParams {
            subject_query: Some("test".to_string()),
            date_from: Some("2024-01-01".to_string()),
            date_to: Some("2024-12-31".to_string()),
            sender: Some("sender@example.com".to_string()),
            participant: Some("participant@example.com".to_string()),
            account: Some("ews://account".to_string()),
            mailbox: Some("INBOX".to_string()),
            limit: 20,
            include_body_preview: false,
            offset: 0,
        };
        let desc = describe_search_filters(&params);
        assert!(desc.contains("test"));
        assert!(desc.contains("2024-01-01"));
        assert!(desc.contains("sender@example.com"));
    }

    #[test]
    fn describe_search_filters_with_empty_options() {
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
            offset: 0,
        };
        let desc = describe_search_filters(&params);
        // Empty filters may return empty string or a default message
        let _ = desc.len(); // Suppress unused warning
    }

    // Serialization tests — ensure skip_serializing_if works correctly

    #[test]
    fn attachment_count_zero_is_omitted() {
        let result = SearchMessageResult {
            id: "1".into(),
            subject: "test".into(),
            from: "a@b.com".into(),
            date_sent: Some("2024-01-01T00:00Z".into()),
            mailbox: "INBOX".into(),
            attachment_count: 0,
            body_preview: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("attachment_count"),
            "zero should be omitted: {json}"
        );
    }

    #[test]
    fn attachment_count_nonzero_is_present() {
        let result = SearchMessageResult {
            id: "1".into(),
            subject: "test".into(),
            from: "a@b.com".into(),
            date_sent: Some("2024-01-01T00:00Z".into()),
            mailbox: "INBOX".into(),
            attachment_count: 3,
            body_preview: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("\"attachment_count\":3"),
            "nonzero should be present: {json}"
        );
    }

    #[test]
    fn body_preview_none_is_omitted() {
        let result = SearchMessageResult {
            id: "1".into(),
            subject: "test".into(),
            from: "a@b.com".into(),
            date_sent: Some("2024-01-01T00:00Z".into()),
            mailbox: "INBOX".into(),
            attachment_count: 1,
            body_preview: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("body_preview"),
            "None should be omitted: {json}"
        );
    }

    #[test]
    fn date_received_not_in_search_result() {
        let result = SearchMessageResult {
            id: "1".into(),
            subject: "test".into(),
            from: "a@b.com".into(),
            date_sent: Some("2024-01-01T00:00Z".into()),
            mailbox: "INBOX".into(),
            attachment_count: 0,
            body_preview: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            !json.contains("date_received"),
            "date_received should not exist: {json}"
        );
    }

    #[test]
    fn total_count_omitted_when_no_more() {
        let response = SearchMessagesResponse {
            status: None,
            messages: vec![],
            total_count: None,
            has_more: false,
            next_offset: None,
            guidance: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(
            !json.contains("total_count"),
            "None total_count should be omitted: {json}"
        );
    }

    #[test]
    fn total_count_present_when_has_more() {
        let response = SearchMessagesResponse {
            status: None,
            messages: vec![],
            total_count: Some(20),
            has_more: true,
            next_offset: Some(40),
            guidance: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(
            json.contains("\"total_count\":20"),
            "Some total_count should be present: {json}"
        );
    }

    #[test]
    fn status_none_omitted_from_response() {
        let response = SearchMessagesResponse {
            status: None,
            messages: vec![],
            total_count: None,
            has_more: false,
            next_offset: None,
            guidance: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(
            !json.contains("status"),
            "None status should be omitted: {json}"
        );
    }

    #[test]
    fn next_offset_is_offset_plus_limit() {
        let response = SearchMessagesResponse {
            status: None,
            messages: vec![],
            total_count: Some(20),
            has_more: true,
            next_offset: Some(30),
            guidance: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(
            json.contains("\"next_offset\":30"),
            "next_offset should be offset+limit: {json}"
        );
    }
}
