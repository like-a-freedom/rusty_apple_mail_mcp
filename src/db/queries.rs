//! Typed SQL queries for the Apple Mail Envelope Index database.
//!
//! # Note on Timestamp Epoch
//!
//! Apple Mail storage can vary by version. This module therefore detects whether
//! timestamps look like Unix epoch values or CoreData epoch values and converts
//! date filters accordingly.

use crate::error::MailMcpError;
use chrono::{Datelike, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params, types::ValueRef};
use std::collections::BTreeMap;

/// Raw database row from the messages index, before domain mapping.
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub rowid: i64,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub mailbox_url: Option<String>,
    pub date_sent: Option<i64>,
    pub date_received: Option<i64>,
    pub message_id: Option<String>,
}

/// Aggregated account information derived from mailbox URL prefixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub account_id: String,
    pub account_type: String,
    pub mailbox_count: i64,
    pub message_count: i64,
}

/// CoreData epoch offset: seconds from 1970-01-01 to 2001-01-01.
pub const COREDATA_EPOCH_OFFSET: i64 = 978_307_200;

fn read_optional_string(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        ValueRef::Text(value) => Ok(Some(String::from_utf8_lossy(value).into_owned())),
        ValueRef::Integer(value) => Ok(Some(value.to_string())),
        ValueRef::Real(value) => Ok(Some(value.to_string())),
        ValueRef::Blob(_) => Err(rusqlite::Error::InvalidColumnType(
            index,
            row.as_ref()
                .column_name(index)
                .unwrap_or("unknown")
                .to_string(),
            rusqlite::types::Type::Blob,
        )),
    }
}

/// Detect the timestamp offset used by the Apple Mail database.
///
/// Returns `0` for Unix epoch or [`COREDATA_EPOCH_OFFSET`] for CoreData epoch.
pub fn detect_epoch_offset_seconds(conn: &Connection) -> Result<i64, MailMcpError> {
    let sample: Option<i64> = conn
        .query_row(
            "SELECT MAX(COALESCE(date_received, date_sent)) FROM messages",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    Ok(sample.map_or(0, infer_epoch_offset_from_sample))
}

fn infer_epoch_offset_from_sample(sample: i64) -> i64 {
    let now = Utc::now().timestamp();
    let unix_year = Utc.timestamp_opt(sample, 0).single().map(|dt| dt.year());
    let coredata_year = Utc
        .timestamp_opt(sample + COREDATA_EPOCH_OFFSET, 0)
        .single()
        .map(|dt| dt.year());

    let unix_plausible = unix_year.is_some_and(|year| (1990..=2100).contains(&year));
    let core_plausible = coredata_year.is_some_and(|year| (1990..=2100).contains(&year));

    match (unix_plausible, core_plausible) {
        (false, true) => COREDATA_EPOCH_OFFSET,
        (true, false) => 0,
        _ => {
            let unix_distance = (sample - now).abs();
            let core_distance = (sample + COREDATA_EPOCH_OFFSET - now).abs();
            if core_distance < unix_distance {
                COREDATA_EPOCH_OFFSET
            } else {
                0
            }
        }
    }
}

/// Search messages by subject, sender, date range, and/or mailbox.
///
/// All filters are optional and combined with AND logic.
/// Results are ordered by date_received DESC and limited by `limit`/`offset`.
#[allow(clippy::too_many_arguments)]
pub fn search_messages(
    conn: &Connection,
    subject_query: Option<&str>,
    date_from: Option<i64>,
    date_to: Option<i64>,
    sender: Option<&str>,
    participant: Option<&str>,
    account: Option<&str>,
    mailbox: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<Vec<MessageRow>, MailMcpError> {
    let epoch_offset = detect_epoch_offset_seconds(conn)?;

    // Build WHERE clause dynamically
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(subject) = subject_query {
        conditions.push("s.subject LIKE ?");
        params.push(Box::new(format!("%{subject}%")));
    }

    if let Some(from) = date_from {
        conditions.push("m.date_received >= ?");
        params.push(Box::new(from - epoch_offset));
    }

    if let Some(to) = date_to {
        conditions.push("m.date_received <= ?");
        params.push(Box::new(to - epoch_offset));
    }

    if let Some(sender_addr) = sender {
        conditions.push("a.address = ?");
        params.push(Box::new(sender_addr.to_string()));
    }

    if let Some(participant_addr) = participant {
        conditions.push(
            "EXISTS (SELECT 1 FROM recipients r JOIN addresses ra ON ra.ROWID = r.address WHERE r.message = m.ROWID AND ra.address = ?)",
        );
        params.push(Box::new(participant_addr.to_string()));
    }

    if let Some(account_id) = account {
        conditions.push("mb.url LIKE ?");
        params.push(Box::new(format!("{account_id}/%")));
    }

    if let Some(mailbox_filter) = mailbox {
        conditions.push("mb.url LIKE ?");
        params.push(Box::new(format!("%{mailbox_filter}%")));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        r#"
        SELECT 
            m.ROWID,
            s.subject,
            a.address,
            mb.url,
            m.date_sent,
            m.date_received,
            m.message_id
        FROM messages m
        LEFT JOIN subjects s ON m.subject = s.ROWID
        LEFT JOIN sender_addresses sa ON m.sender = sa.ROWID
        LEFT JOIN addresses a ON sa.address = a.ROWID
        LEFT JOIN mailboxes mb ON m.mailbox = mb.ROWID
        {}
        ORDER BY m.date_received DESC LIMIT ? OFFSET ?
        "#,
        where_clause
    );

    // Add limit and offset
    params.push(Box::new(limit));
    params.push(Box::new(offset));

    // Convert to slice of references
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(MessageRow {
            rowid: row.get(0)?,
            subject: row.get(1)?,
            sender: row.get(2)?,
            mailbox_url: row.get(3)?,
            date_sent: row.get(4)?,
            date_received: row.get(5)?,
            message_id: read_optional_string(row, 6)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// Returns `true` if the given email address exists in the normalized address table.
pub fn address_exists(conn: &Connection, address: &str) -> Result<bool, MailMcpError> {
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM addresses WHERE address = ?)",
        params![address],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(exists != 0)
}

/// Get a single message by its rowid.
pub fn get_message_by_id(conn: &Connection, id: i64) -> Result<Option<MessageRow>, MailMcpError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT 
            m.ROWID,
            s.subject,
            a.address,
            mb.url,
            m.date_sent,
            m.date_received,
            m.message_id
        FROM messages m
        LEFT JOIN subjects s ON m.subject = s.ROWID
        LEFT JOIN sender_addresses sa ON m.sender = sa.ROWID
        LEFT JOIN addresses a ON sa.address = a.ROWID
        LEFT JOIN mailboxes mb ON m.mailbox = mb.ROWID
        WHERE m.ROWID = ?
        "#,
    )?;

    let mut rows = stmt.query_map(params![id], |row| {
        Ok(MessageRow {
            rowid: row.get(0)?,
            subject: row.get(1)?,
            sender: row.get(2)?,
            mailbox_url: row.get(3)?,
            date_sent: row.get(4)?,
            date_received: row.get(5)?,
            message_id: read_optional_string(row, 6)?,
        })
    })?;

    match rows.next() {
        Some(result) => result.map(Some).map_err(MailMcpError::Sqlite),
        None => Ok(None),
    }
}

/// Get recipients (To, CC, BCC) for a message.
///
/// Returns (address, type) pairs where type is:
/// - 1 = To
/// - 2 = CC
/// - 3 = BCC
pub fn get_recipients(
    conn: &Connection,
    message_id: i64,
) -> Result<Vec<(String, i32)>, MailMcpError> {
    let mut stmt = conn.prepare(
        r#"
        SELECT a.address, r.type
        FROM recipients r
        JOIN addresses a ON r.address = a.ROWID
        WHERE r.message = ?
        ORDER BY r.type, a.address
        "#,
    )?;

    let rows = stmt.query_map(params![message_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// List all mailboxes with their ROWID and URL.
pub fn list_mailboxes(conn: &Connection) -> Result<Vec<(i64, String)>, MailMcpError> {
    let mut stmt = conn.prepare("SELECT ROWID, url FROM mailboxes ORDER BY url")?;

    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// List mailbox-derived accounts aggregated by URL prefix.
pub fn list_accounts(conn: &Connection) -> Result<Vec<AccountRow>, MailMcpError> {
    let mailboxes = list_mailboxes(conn)?;
    let mut grouped: BTreeMap<String, (String, i64, i64)> = BTreeMap::new();

    for (mailbox_id, url) in mailboxes {
        if let Some(account_id) = mailbox_account_id(&url) {
            let account_type = mailbox_scheme(&url).unwrap_or_else(|| "unknown".to_string());
            let message_count = count_messages_in_mailbox(conn, mailbox_id)?;
            let entry = grouped.entry(account_id).or_insert((account_type, 0, 0));
            entry.1 += 1;
            entry.2 += message_count;
        }
    }

    Ok(grouped
        .into_iter()
        .map(
            |(account_id, (account_type, mailbox_count, message_count))| AccountRow {
                account_id,
                account_type,
                mailbox_count,
                message_count,
            },
        )
        .collect())
}

/// Derive an account identifier from a mailbox URL, e.g. `ews://account-id`.
pub fn mailbox_account_id(mailbox_url: &str) -> Option<String> {
    let scheme_end = mailbox_url.find("://")?;
    let rest = &mailbox_url[scheme_end + 3..];
    let slash = rest.find('/')?;
    Some(format!(
        "{}://{}",
        &mailbox_url[..scheme_end],
        &rest[..slash]
    ))
}

fn mailbox_scheme(mailbox_url: &str) -> Option<String> {
    mailbox_url
        .find("://")
        .map(|index| mailbox_url[..index].to_string())
}

/// Count messages in a mailbox.
pub fn count_messages_in_mailbox(conn: &Connection, mailbox_id: i64) -> Result<i64, MailMcpError> {
    let mut stmt = conn.prepare("SELECT COUNT(*) FROM messages WHERE mailbox = ?")?;

    let count: i64 = stmt.query_row(params![mailbox_id], |row| row.get(0))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an in-memory test database with a minimal schema and seed data.
    pub fn make_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE subjects (ROWID INTEGER PRIMARY KEY, subject TEXT);
            CREATE TABLE addresses (ROWID INTEGER PRIMARY KEY, address TEXT);
            CREATE TABLE sender_addresses (sender INTEGER PRIMARY KEY, address INTEGER REFERENCES addresses);
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (
                ROWID INTEGER PRIMARY KEY,
                subject INTEGER REFERENCES subjects,
                sender INTEGER REFERENCES sender_addresses,
                mailbox INTEGER REFERENCES mailboxes,
                date_sent INTEGER,
                date_received INTEGER,
                message_id TEXT
            );
            CREATE TABLE recipients (
                message INTEGER REFERENCES messages,
                address INTEGER REFERENCES addresses,
                type INTEGER
            );

            -- Seed data
            INSERT INTO subjects VALUES (1, 'Q3 Review'), (2, 'Budget Planning');
            INSERT INTO addresses VALUES (1, 'alice@example.com'), (2, 'bob@example.com');
            INSERT INTO sender_addresses VALUES (1, 1);
            INSERT INTO mailboxes VALUES (1, 'imap://alice@mail.example.com/INBOX');
            
            -- Use CoreData epoch: 2024-09-15 = 1726358400 (Unix) - 978307200 = 748051200
            INSERT INTO messages VALUES (1, 1, 1, 1, 748051200, 748051200, '<msg1@mail>');
            INSERT INTO messages VALUES (2, 2, 1, 1, 766627200, 766627200, '<msg2@mail>');
            
            INSERT INTO recipients VALUES (1, 2, 1), (2, 2, 1);
            "#,
        )
        .expect("seed test schema");
        conn
    }

    #[test]
    fn search_by_subject_returns_matching_messages() {
        let conn = make_test_db();
        let results =
            search_messages(&conn, Some("Q3"), None, None, None, None, None, None, 20, 0).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subject, Some("Q3 Review".to_string()));
    }

    #[test]
    fn search_by_sender_returns_matching_messages() {
        let conn = make_test_db();
        let results = search_messages(
            &conn,
            None,
            None,
            None,
            Some("alice@example.com"),
            None,
            None,
            None,
            20,
            0,
        )
        .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_with_no_filters_returns_all_messages() {
        let conn = make_test_db();
        let results =
            search_messages(&conn, None, None, None, None, None, None, None, 20, 0).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_by_participant_returns_matching_messages() {
        let conn = make_test_db();
        let results = search_messages(
            &conn,
            None,
            None,
            None,
            None,
            Some("bob@example.com"),
            None,
            None,
            20,
            0,
        )
        .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_by_account_returns_matching_messages() {
        let conn = make_test_db();
        let results = search_messages(
            &conn,
            None,
            None,
            None,
            None,
            None,
            Some("imap://alice@mail.example.com"),
            None,
            20,
            0,
        )
        .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn get_message_by_id_returns_message() {
        let conn = make_test_db();
        let result = get_message_by_id(&conn, 1).unwrap();
        assert!(result.is_some());
        let msg = result.unwrap();
        assert_eq!(msg.subject, Some("Q3 Review".to_string()));
    }

    #[test]
    fn get_message_by_id_not_found_returns_none() {
        let conn = make_test_db();
        let result = get_message_by_id(&conn, 999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_recipients_returns_to_recipients() {
        let conn = make_test_db();
        let recipients = get_recipients(&conn, 1).unwrap();
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0], ("bob@example.com".to_string(), 1));
    }

    #[test]
    fn list_mailboxes_returns_all_mailboxes() {
        let conn = make_test_db();
        let mailboxes = list_mailboxes(&conn).unwrap();
        assert_eq!(mailboxes.len(), 1);
        assert!(mailboxes[0].1.contains("INBOX"));
    }

    #[test]
    fn list_accounts_groups_mailboxes_by_url_prefix() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (ROWID INTEGER PRIMARY KEY, mailbox INTEGER, date_sent INTEGER, date_received INTEGER, message_id TEXT, subject INTEGER, sender INTEGER);

            INSERT INTO mailboxes VALUES (1, 'ews://account-b/Inbox');
            INSERT INTO mailboxes VALUES (2, 'ews://account-b/Sent Items');
            INSERT INTO mailboxes VALUES (3, 'imap://account-a/INBOX');

            INSERT INTO messages VALUES (1, 1, 0, 0, 'm1', NULL, NULL);
            INSERT INTO messages VALUES (2, 2, 0, 0, 'm2', NULL, NULL);
            INSERT INTO messages VALUES (3, 3, 0, 0, 'm3', NULL, NULL);
            "#,
        )
        .expect("seed sqlite");

        let accounts = list_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].account_id, "ews://account-b");
        assert_eq!(accounts[0].mailbox_count, 2);
        assert_eq!(accounts[0].message_count, 2);
        assert_eq!(accounts[1].account_id, "imap://account-a");
    }

    #[test]
    fn count_messages_in_mailbox_returns_correct_count() {
        let conn = make_test_db();
        let count = count_messages_in_mailbox(&conn, 1).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn detect_epoch_offset_for_coredata_seed() {
        let conn = make_test_db();
        assert_eq!(
            detect_epoch_offset_seconds(&conn).unwrap(),
            COREDATA_EPOCH_OFFSET
        );
    }

    #[test]
    fn address_exists_returns_expected_value() {
        let conn = make_test_db();
        assert!(address_exists(&conn, "alice@example.com").unwrap());
        assert!(!address_exists(&conn, "nobody@example.com").unwrap());
    }

    #[test]
    fn search_messages_handles_integer_message_id_column() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE subjects (ROWID INTEGER PRIMARY KEY, subject TEXT);
            CREATE TABLE addresses (ROWID INTEGER PRIMARY KEY, address TEXT);
            CREATE TABLE sender_addresses (ROWID INTEGER PRIMARY KEY, address INTEGER REFERENCES addresses);
            CREATE TABLE mailboxes (ROWID INTEGER PRIMARY KEY, url TEXT);
            CREATE TABLE messages (
                ROWID INTEGER PRIMARY KEY,
                subject INTEGER REFERENCES subjects,
                sender INTEGER REFERENCES sender_addresses,
                mailbox INTEGER REFERENCES mailboxes,
                date_sent INTEGER,
                date_received INTEGER,
                message_id INTEGER
            );

            INSERT INTO subjects VALUES (1, 'Today');
            INSERT INTO addresses VALUES (1, 'sender@example.com');
            INSERT INTO sender_addresses VALUES (1, 1);
            INSERT INTO mailboxes VALUES (1, 'ews://account/Inbox');
            INSERT INTO messages VALUES (1, 1, 1, 1, 0, 0, 123456);
            "#,
        )
        .expect("seed sqlite");

        let results = search_messages(
            &conn,
            Some("Today"),
            None,
            None,
            None,
            None,
            None,
            None,
            20,
            0,
        )
        .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message_id.as_deref(), Some("123456"));
    }
}
