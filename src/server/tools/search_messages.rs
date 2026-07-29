//! `search_messages` tool implementation.

use std::sync::Arc;
use std::time::Instant;

use crate::db::{MailRepository, MessageRow, SearchParams, tokenize};
use crate::error::MailMcpError;
use crate::mail::EmlxLocator;
use crate::mail::parse_emlx_without_attachment_content;
use crate::server::tools::ResponseOutcome;
use crate::{MailConfig, MessageMeta};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the `search_messages` tool.
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
    /// Account identifier returned by `list_accounts` (for example, `ews://account-id`)
    pub account: Option<String>,
    /// Mailbox name or fragment
    pub mailbox: Option<String>,
    /// Maximum number of results (default 20, max 100)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Offset for pagination (use `next_offset` from previous response)
    #[serde(default)]
    pub offset: u32,
    /// Include ~200 character body preview
    #[serde(default)]
    pub include_body_preview: bool,
}

fn default_limit() -> u32 {
    20
}

impl From<SearchMessagesParams> for SearchParams {
    fn from(p: SearchMessagesParams) -> Self {
        Self {
            subject_query: p.subject_query,
            date_from: None,
            date_to: None,
            sender: p.sender,
            participant: p.participant,
            account: p.account,
            allowed_accounts: None,
            mailbox: p.mailbox,
            limit: p.limit,
            offset: p.offset,
        }
    }
}

/// Response message item for `search_messages` results.
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

const fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Response for `search_messages` tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchMessagesResponse {
    pub outcome: ResponseOutcome,
    pub messages: Vec<SearchMessageResult>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
}

impl SearchMessagesResponse {
    /// Create a not-found response with a guidance message.
    pub fn not_found(guidance: impl Into<String>) -> Self {
        Self {
            outcome: ResponseOutcome::NotFound,
            guidance: Some(guidance.into()),
            messages: Vec::new(),
            has_more: false,
            next_offset: None,
        }
    }

    /// Create a partial response with a result and guidance.
    pub fn partial(messages: Vec<SearchMessageResult>, guidance: impl Into<String>) -> Self {
        let has_more = messages.len() >= 100;
        Self {
            outcome: ResponseOutcome::Partial,
            has_more,
            next_offset: None,
            guidance: Some(guidance.into()),
            messages,
        }
    }

    /// Create a success response with messages.
    pub fn success(
        messages: Vec<SearchMessageResult>,
        has_more: bool,
        next_offset: Option<u32>,
    ) -> Self {
        Self {
            outcome: ResponseOutcome::Success,
            has_more,
            next_offset,
            guidance: None,
            messages,
        }
    }
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

    if params.limit > 100 {
        return Err(format!(
            "limit must be between 1 and 100, got {}; use --offset to paginate beyond 100 results",
            params.limit
        ));
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
    locator: &EmlxLocator<'_>,
    row: &MessageRow,
    epoch_offset_s: i64,
    include_body_preview: bool,
    metadata: Option<&SearchMetadata>,
    mail_root: &std::path::Path,
    mail_version: &str,
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
        && let Some(path) = locator.locate_emlx_quick_with_hints(
            mail_root,
            mail_version,
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
    repo: &dyn MailRepository,
    message_ids: &[i64],
) -> Result<std::collections::HashMap<i64, SearchMetadata>, MailMcpError> {
    let metadata_map = repo.get_message_metadata(message_ids)?;
    Ok(metadata_map
        .into_iter()
        .map(|(id, meta)| {
            (
                id,
                SearchMetadata {
                    summary: meta.summary,
                    attachment_count: meta.attachment_count,
                },
            )
        })
        .collect())
}

/// Resolve a human-friendly account filter value to a canonical account ID,
/// intersecting with the Scope (allowed accounts) when present.
///
/// Returns `Ok(None)` when no filter is provided (no restriction needed).
/// Returns `Ok(Some(canonical_id))` when the filter resolves and passes the Scope.
/// Returns `Err(MailMcpError::Validation)` with guidance when the filter cannot
/// be resolved or falls outside the Scope.
fn resolve_account_filter(
    filter_value: Option<&str>,
    account_metadata: &std::collections::HashMap<String, crate::accounts::AccountMetadata>,
    allowed_account_ids: Option<&[String]>,
) -> Result<Option<String>, MailMcpError> {
    let Some(value) = filter_value else {
        return Ok(None);
    };

    if account_metadata.is_empty() {
        return Err(MailMcpError::Validation(format!(
            "Account filter \"{value}\" cannot be resolved without account metadata. \
             Set APPLE_MAIL_ACCOUNT to restrict access, or use a canonical account ID \
             (e.g. ews://UUID). Use list_accounts to see available account names, emails, or IDs."
        )));
    }

    let resolved =
        crate::accounts::resolve_account_selectors(&[value.to_string()], account_metadata)
            .map_err(|_| {
                MailMcpError::Validation(format!(
                    "Account filter \"{value}\" did not match any known account. \
                     Use list_accounts to see available account names, emails, or IDs."
                ))
            })?;

    let canonical = resolved.into_iter().next().ok_or_else(|| {
        MailMcpError::Validation(format!(
            "Account filter \"{value}\" did not match any known account. \
             Use list_accounts to see available account names, emails, or IDs."
        ))
    })?;

    if let Some(allowed) = allowed_account_ids
        && !allowed.iter().any(|a| a == &canonical)
    {
        return Err(MailMcpError::Validation(format!(
            "Account filter \"{value}\" resolved to {canonical}, which is excluded by APPLE_MAIL_ACCOUNT. \
             Use list_accounts to see available account names, emails, or IDs."
        )));
    }

    Ok(Some(canonical))
}

fn build_not_found_guidance(
    repo: &dyn MailRepository,
    params: &SearchMessagesParams,
) -> Result<String, MailMcpError> {
    if let Some(sender) = params.sender.as_deref() {
        if !repo.address_exists(sender)? {
            return Ok(format!(
                "Sender address \"{sender}\" was not found in Apple Mail. Check the spelling with list_accounts or use a different account filter."
            ));
        }
        return Ok("No messages match the provided filters. Try broadening the date range or shortening subject_query to one or two keywords.".to_string());
    }

    if let Some(participant) = params.participant.as_deref() {
        if !repo.address_exists(participant)? {
            return Ok(format!(
                "Participant address \"{participant}\" was not found in Apple Mail. Verify the address with list_accounts or try a different account filter."
            ));
        }
        return Ok("No messages match the provided filters. Try broadening the date range or changing the mailbox filter.".to_string());
    }

    Ok("No messages match the provided filters. Try broadening the date range, shortening subject_query to one or two keywords, or verifying the sender address with list_accounts.".to_string())
}

fn search_rows_with_subject_fallback(
    repo: &dyn MailRepository,
    params: &SearchMessagesParams,
    date_from_ts: Option<i64>,
    date_to_ts: Option<i64>,
    allowed_accounts: Option<&[String]>,
) -> Result<Vec<MessageRow>, MailMcpError> {
    let fetch_limit = params.limit.saturating_add(1);
    let mut search_params = SearchParams {
        subject_query: params.subject_query.clone(),
        date_from: date_from_ts,
        date_to: date_to_ts,
        sender: params.sender.clone(),
        participant: params.participant.clone(),
        account: params.account.clone(),
        allowed_accounts: allowed_accounts.map(|v| v.to_vec()),
        mailbox: params.mailbox.clone(),
        limit: fetch_limit,
        offset: params.offset,
    };

    let mut rows = repo.search_messages(search_params.clone())?;

    if rows.is_empty()
        && let Some(subject_query) = params.subject_query.as_deref()
    {
        let tokens = tokenize(subject_query);
        if !tokens.is_empty() {
            tracing::debug!("Token search returned no results, trying fallback full-string search");

            search_params.subject_query = Some(subject_query.to_string());
            rows = repo.search_messages(search_params)?;
        }
    }

    Ok(rows)
}

/// Internal implementation that uses the repository trait.
/// This is the new architecture-aware implementation.
pub async fn search_messages_with_repo(
    config: &MailConfig,
    repo: Arc<dyn MailRepository>,
    _attachment_store: Arc<dyn crate::mail::AttachmentStore>,
    locator: &EmlxLocator<'_>,
    params: SearchMessagesParams,
) -> Result<SearchMessagesResponse, MailMcpError> {
    let total_started = Instant::now();
    let filters_description = describe_search_filters(&params);
    validate_params(&params).map_err(MailMcpError::Validation)?;

    let (date_from_ts, date_to_ts) = parse_date_range(&params).map_err(MailMcpError::Validation)?;

    let epoch_offset_s = repo.detect_epoch_offset()?;

    let resolved_account = resolve_account_filter(
        params.account.as_deref(),
        &config.account_metadata,
        config.allowed_account_ids(),
    )?;
    let mut params = params;
    params.account = resolved_account;

    let sql_started = Instant::now();

    let mut rows = search_rows_with_subject_fallback(
        &*repo,
        &params,
        date_from_ts,
        date_to_ts,
        config.allowed_account_ids(),
    )?;
    let sql_elapsed = sql_started.elapsed();

    let has_more = rows.len() > params.limit as usize;
    if has_more {
        rows.truncate(params.limit as usize);
    }

    let metadata_started = Instant::now();
    let message_ids = rows.iter().map(|row| row.rowid).collect::<Vec<_>>();
    let search_metadata = load_search_metadata(repo.as_ref(), &message_ids)?;
    let metadata_elapsed = metadata_started.elapsed();

    if rows.is_empty() {
        tracing::debug!(
            "search_messages completed: 0 result(s), sql={} ms, metadata={} ms, hydration=0 ms, total={} ms; filters: {}",
            sql_elapsed.as_millis(),
            metadata_elapsed.as_millis(),
            total_started.elapsed().as_millis(),
            filters_description,
        );
        return Ok(SearchMessagesResponse::not_found(build_not_found_guidance(
            repo.as_ref(),
            &params,
        )?));
    }

    let hydration_started = Instant::now();
    let messages = rows
        .iter()
        .map(|row| {
            hydrate_search_result(
                locator,
                row,
                epoch_offset_s,
                params.include_body_preview,
                search_metadata.get(&row.rowid),
                &config.mail_directory,
                &config.mail_version,
            )
        })
        .collect::<Vec<_>>();
    let hydration_elapsed = hydration_started.elapsed();

    tracing::debug!(
        "search_messages completed: {} result(s), sql={} ms, metadata={} ms, hydration={} ms, total={} ms; filters: {}",
        messages.len(),
        sql_elapsed.as_millis(),
        metadata_elapsed.as_millis(),
        hydration_elapsed.as_millis(),
        total_started.elapsed().as_millis(),
        filters_description,
    );
    let next_offset = has_more.then_some(params.offset + params.limit);
    Ok(SearchMessagesResponse::success(
        messages,
        has_more,
        next_offset,
    ))
}

/// Public async tool function for the MCP handler.
/// Uses the provided repository and attachment store from the server context.
pub async fn search_messages_async(
    repo: Arc<dyn MailRepository>,
    store: Arc<dyn crate::mail::AttachmentStore>,
    config: &MailConfig,
    locator: &EmlxLocator<'_>,
    params: SearchMessagesParams,
) -> Result<SearchMessagesResponse, MailMcpError> {
    search_messages_with_repo(config, repo, store, locator, params).await
}

/// Public sync tool function for CLI usage.
/// Creates repository and attachment store internally, then delegates to the async implementation.
pub fn search_messages(
    config: &MailConfig,
    params: SearchMessagesParams,
) -> Result<SearchMessagesResponse, MailMcpError> {
    let db_path = config.envelope_db_path();
    let repo = Arc::new(crate::db::SqliteMailRepository::new(&db_path)?);
    let store = Arc::new(crate::mail::FilesystemAttachmentStore::new(
        &config.mail_directory,
    ));
    let registry = crate::mail::CacheRegistry::new();
    let locator = EmlxLocator::new(&registry);
    let future = search_messages_with_repo(config, repo, store, &locator, params);
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => tokio::runtime::Runtime::new().unwrap().block_on(future),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MailConfig;
    use crate::db::MessageRow;
    use crate::mail::CacheRegistry;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    type FakeMailRepository = crate::db::FakeMailRepository;

    fn test_locator() -> EmlxLocator<'static> {
        use std::sync::LazyLock;
        static REGISTRY: LazyLock<CacheRegistry> = LazyLock::new(CacheRegistry::new);
        EmlxLocator::new(&REGISTRY)
    }

    fn make_test_config() -> (TempDir, MailConfig) {
        let temp_dir = TempDir::new().unwrap();
        let mail_directory = temp_dir.path().to_path_buf();
        let db_dir = mail_directory.join("V10").join("MailData");
        std::fs::create_dir_all(&db_dir).unwrap();
        std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").unwrap();
        let config = MailConfig::new(
            mail_directory,
            "V10".to_string(),
            None,
            HashMap::new(),
            None,
        )
        .unwrap();
        (temp_dir, config)
    }

    fn make_fake_repo() -> (FakeMailRepository, Arc<FakeMailRepository>) {
        let repo = FakeMailRepository::default();
        let arc = Arc::new(repo.clone());
        (repo, arc)
    }

    #[tokio::test]
    async fn search_with_no_filters_returns_error() {
        let (_temp, config) = make_test_config();
        let (_repo, repo_arc) = make_fake_repo();
        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        let params = SearchMessagesParams {
            subject_query: None,
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            offset: 0,
            include_body_preview: false,
        };
        let err = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("At least one filter"));
    }

    #[tokio::test]
    async fn search_rejects_limit_over_100() {
        let (_temp, config) = make_test_config();
        let (_repo, repo_arc) = make_fake_repo();
        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        let params = SearchMessagesParams {
            subject_query: Some("test".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 101,
            offset: 0,
            include_body_preview: false,
        };
        let err = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("limit must be between 1 and 100"));
    }

    #[tokio::test]
    async fn search_finds_matching_messages() {
        let (_temp, config) = make_test_config();
        let (repo, repo_arc) = make_fake_repo();

        let msg = MessageRow {
            rowid: 42,
            subject: Some("Test Subject".to_string()),
            sender: Some("sender@example.com".to_string()),
            mailbox_url: Some("imap://test@example.com/INBOX".to_string()),
            date_sent: Some(1000000000),
            date_received: Some(1000000000),
            message_id: Some("<test@msg>".to_string()),
            global_message_id: Some(1),
            message_id_header: Some("<test@msg>".to_string()),
        };
        repo.messages.lock().unwrap().insert(42, msg);

        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        let params = SearchMessagesParams {
            subject_query: Some("Test".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            offset: 0,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap();
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].subject, "Test Subject");
    }

    #[tokio::test]
    async fn search_by_sender_works() {
        let (_temp, config) = make_test_config();
        let (repo, repo_arc) = make_fake_repo();

        let msg = MessageRow {
            rowid: 1,
            subject: Some("Hello".to_string()),
            sender: Some("alice@example.com".to_string()),
            mailbox_url: Some("imap://test/INBOX".to_string()),
            date_sent: Some(1000),
            date_received: Some(1000),
            message_id: None,
            global_message_id: None,
            message_id_header: None,
        };
        repo.messages.lock().unwrap().insert(1, msg);

        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        let params = SearchMessagesParams {
            subject_query: None,
            date_from: None,
            date_to: None,
            sender: Some("alice@example.com".to_string()),
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            offset: 0,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(
            &config,
            repo_arc.clone(),
            store.clone(),
            &test_locator(),
            params,
        )
        .await
        .unwrap();
        assert_eq!(result.messages.len(), 1);

        // Wrong sender should return empty
        let params = SearchMessagesParams {
            subject_query: None,
            date_from: None,
            date_to: None,
            sender: Some("bob@example.com".to_string()),
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            offset: 0,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap();
        assert!(result.messages.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sync_search_messages_works_inside_tokio_runtime() {
        let (_temp, config) = make_test_config();
        // Remove placeholder and create a real SQLite database
        let db_path = config.envelope_db_path();
        std::fs::remove_file(&db_path).expect("remove placeholder");
        let conn = rusqlite::Connection::open(&db_path).expect("create db");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (
                ROWID INTEGER PRIMARY KEY,
                subject INTEGER,
                sender INTEGER,
                mailbox INTEGER,
                summary INTEGER,
                date_sent INTEGER,
                date_received INTEGER,
                message_id TEXT,
                global_message_id INTEGER
            );
            CREATE TABLE subjects (ROWID INTEGER PRIMARY KEY, subject TEXT);
            CREATE TABLE addresses (ROWID INTEGER PRIMARY KEY, address TEXT);
            CREATE TABLE sender_addresses (sender INTEGER PRIMARY KEY, address INTEGER REFERENCES addresses);
            CREATE TABLE summaries (ROWID INTEGER PRIMARY KEY, summary TEXT);
            CREATE TABLE attachments (ROWID INTEGER PRIMARY KEY, message INTEGER REFERENCES messages, attachment_id TEXT, name TEXT);
            CREATE TABLE message_global_data (ROWID INTEGER PRIMARY KEY, message_id INTEGER, message_id_header TEXT);
            CREATE TABLE recipients (message INTEGER REFERENCES messages, address INTEGER REFERENCES addresses, type INTEGER);
            INSERT INTO subjects VALUES (1, 'Test Subject');
            INSERT INTO addresses VALUES (1, 'test@example.com');
            INSERT INTO sender_addresses VALUES (1, 1);
            INSERT INTO mailboxes VALUES (1, 'imap://test/INBOX');
            INSERT INTO message_global_data VALUES (10, 111, '<msg1@mail>');
            INSERT INTO messages VALUES (1, 1, 1, 1, NULL, 748051200, 748051200, '<msg1@mail>', 10);
            "#,
        )
        .expect("seed db");
        drop(conn);

        let params = SearchMessagesParams {
            subject_query: Some("Test Subject".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            offset: 0,
            include_body_preview: false,
        };

        // Call the SYNC function from inside a tokio runtime — exercises block_in_place
        let result = search_messages(&config, params).expect("sync search inside tokio runtime");
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].subject, "Test Subject");
    }

    #[tokio::test]
    async fn has_more_false_when_results_fit_exactly_in_limit() {
        let (_temp, config) = make_test_config();
        let (repo, repo_arc) = make_fake_repo();
        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        for i in 1..=3 {
            let msg = MessageRow {
                rowid: i,
                subject: Some(format!("Subject {i}")),
                sender: Some("sender@example.com".to_string()),
                mailbox_url: Some("imap://test/INBOX".to_string()),
                date_sent: Some(1000 + i),
                date_received: Some(1000 + i),
                message_id: None,
                global_message_id: None,
                message_id_header: None,
            };
            repo.messages.lock().unwrap().insert(i, msg);
        }

        let params = SearchMessagesParams {
            subject_query: Some("Subject".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 3,
            offset: 0,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap();
        assert_eq!(result.messages.len(), 3);
        assert!(
            !result.has_more,
            "has_more should be false when all results fit in limit"
        );
        assert!(result.next_offset.is_none());
    }

    #[tokio::test]
    async fn has_more_true_when_more_results_exist() {
        let (_temp, config) = make_test_config();
        let (repo, repo_arc) = make_fake_repo();
        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        for i in 1..=5 {
            let msg = MessageRow {
                rowid: i,
                subject: Some(format!("Subject {i}")),
                sender: Some("sender@example.com".to_string()),
                mailbox_url: Some("imap://test/INBOX".to_string()),
                date_sent: Some(1000 + i),
                date_received: Some(1000 + i),
                message_id: None,
                global_message_id: None,
                message_id_header: None,
            };
            repo.messages.lock().unwrap().insert(i, msg);
        }

        let params = SearchMessagesParams {
            subject_query: Some("Subject".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 3,
            offset: 0,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap();
        assert_eq!(result.messages.len(), 3);
        assert!(
            result.has_more,
            "has_more should be true when more results exist"
        );
        assert_eq!(result.next_offset, Some(3));
    }

    #[tokio::test]
    async fn last_page_has_more_false() {
        let (_temp, config) = make_test_config();
        let (repo, repo_arc) = make_fake_repo();
        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        for i in 1..=5 {
            let msg = MessageRow {
                rowid: i,
                subject: Some(format!("Subject {i}")),
                sender: Some("sender@example.com".to_string()),
                mailbox_url: Some("imap://test/INBOX".to_string()),
                date_sent: Some(1000 + i),
                date_received: Some(1000 + i),
                message_id: None,
                global_message_id: None,
                message_id_header: None,
            };
            repo.messages.lock().unwrap().insert(i, msg);
        }

        // Page 2 (offset=3, limit=3) should return 2 results with has_more=false
        let params = SearchMessagesParams {
            subject_query: Some("Subject".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 3,
            offset: 3,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap();
        assert_eq!(result.messages.len(), 2);
        assert!(!result.has_more, "last page should have has_more=false");
        assert!(result.next_offset.is_none());
    }

    #[tokio::test]
    async fn offset_past_end_returns_not_found() {
        let (_temp, config) = make_test_config();
        let (repo, repo_arc) = make_fake_repo();
        let store = Arc::new(crate::mail::FakeAttachmentStore::default());

        for i in 1..=3 {
            let msg = MessageRow {
                rowid: i,
                subject: Some(format!("Subject {i}")),
                sender: Some("sender@example.com".to_string()),
                mailbox_url: Some("imap://test/INBOX".to_string()),
                date_sent: Some(1000 + i),
                date_received: Some(1000 + i),
                message_id: None,
                global_message_id: None,
                message_id_header: None,
            };
            repo.messages.lock().unwrap().insert(i, msg);
        }

        let params = SearchMessagesParams {
            subject_query: Some("Subject".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 3,
            offset: 100,
            include_body_preview: false,
        };
        let result = search_messages_with_repo(&config, repo_arc, store, &test_locator(), params)
            .await
            .unwrap();
        assert!(result.messages.is_empty());
        assert!(!result.has_more);
        assert!(result.next_offset.is_none());
    }

    // --- resolve_account_filter unit tests ---

    fn make_metadata(
        accounts: impl IntoIterator<Item = (&'static str, &'static str, &'static str)>,
    ) -> HashMap<String, crate::accounts::AccountMetadata> {
        accounts
            .into_iter()
            .map(|(id, name, email)| {
                (
                    id.to_string(),
                    crate::accounts::AccountMetadata {
                        account_id: id.to_string(),
                        account_name: Some(name.to_string()),
                        email: Some(email.to_string()),
                        username: None,
                        source_identifier: id.trim_start_matches("ews://").to_string(),
                        account_type: "ews".to_string(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn resolve_account_filter_none_returns_none() {
        let metadata = make_metadata([]);
        let result = resolve_account_filter(None, &metadata, None).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_account_filter_resolves_by_name() {
        let metadata = make_metadata([("ews://work", "Work Email", "user@work.example.com")]);
        let result = resolve_account_filter(Some("Work Email"), &metadata, None).unwrap();
        assert_eq!(result.as_deref(), Some("ews://work"));
    }

    #[test]
    fn resolve_account_filter_resolves_by_email() {
        let metadata = make_metadata([("ews://work", "Work Email", "user@work.example.com")]);
        let result =
            resolve_account_filter(Some("user@work.example.com"), &metadata, None).unwrap();
        assert_eq!(result.as_deref(), Some("ews://work"));
    }

    #[test]
    fn resolve_account_filter_resolves_by_canonical_id() {
        let metadata = make_metadata([("ews://work", "Work Email", "user@work.example.com")]);
        let result = resolve_account_filter(Some("ews://work"), &metadata, None).unwrap();
        assert_eq!(result.as_deref(), Some("ews://work"));
    }

    #[test]
    fn resolve_account_filter_unresolvable_returns_validation_error() {
        let metadata = make_metadata([("ews://work", "Work Email", "user@work.example.com")]);
        let err = resolve_account_filter(Some("NoSuchAccount"), &metadata, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("did not match"), "unexpected error: {msg}");
        assert!(msg.contains("list_accounts"), "should guide user: {msg}");
    }

    #[test]
    fn resolve_account_filter_outside_scope_returns_validation_error() {
        let metadata = make_metadata([
            ("ews://work", "Work Email", "user@work.example.com"),
            ("imap://personal", "Gmail", "me@gmail.com"),
        ]);
        let allowed = Some(vec!["ews://work".to_string()]);
        let err = resolve_account_filter(Some("Gmail"), &metadata, allowed.as_deref()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("excluded"), "should mention exclusion: {msg}");
        assert!(msg.contains("list_accounts"), "should guide user: {msg}");
    }

    #[test]
    fn resolve_account_filter_in_scope_passes() {
        let metadata = make_metadata([
            ("ews://work", "Work Email", "user@work.example.com"),
            ("imap://personal", "Gmail", "me@gmail.com"),
        ]);
        let allowed = Some(vec!["ews://work".to_string()]);
        let result =
            resolve_account_filter(Some("Work Email"), &metadata, allowed.as_deref()).unwrap();
        assert_eq!(result.as_deref(), Some("ews://work"));
    }

    #[test]
    fn resolve_account_filter_empty_metadata_errors_with_guidance() {
        let metadata = make_metadata([]);
        let err = resolve_account_filter(Some("Exchange"), &metadata, None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("cannot be resolved without account metadata"),
            "should explain the problem: {msg}"
        );
        assert!(
            msg.contains("APPLE_MAIL_ACCOUNT"),
            "should suggest env var: {msg}"
        );
        assert!(
            msg.contains("list_accounts"),
            "should suggest list_accounts: {msg}"
        );
    }

    #[test]
    fn resolve_account_filter_empty_metadata_with_scope_also_errors() {
        let metadata = make_metadata([]);
        let allowed = Some(vec!["ews://allowed".to_string()]);
        let err =
            resolve_account_filter(Some("Exchange"), &metadata, allowed.as_deref()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("cannot be resolved without account metadata"),
            "should explain the problem: {msg}"
        );
    }
}
