//! SQLite implementation of the MailRepository trait.

use crate::db::accounts::AccountRow;
use crate::db::connection::open_readonly;
use crate::db::epoch::detect_epoch_offset_seconds;
use crate::db::mailboxes::{count_messages_in_mailbox, list_mailboxes};
use crate::db::messages::{MessageRow, get_message_by_id, get_recipients, search_messages};
use crate::db::{MailRepository, MessageMetadata, SearchParams};
use crate::error::MailMcpError;
use rusqlite::Connection;
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// SQLite-backed mail repository.
///
/// Uses a Mutex-protected connection since rusqlite::Connection with NO_MUTEX
/// is not thread-safe for shared access.
#[derive(Debug)]
pub struct SqliteMailRepository {
    conn: Arc<Mutex<Connection>>,
    epoch_offset: i64,
}

impl SqliteMailRepository {
    /// Create a new repository from a database path.
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self, MailMcpError> {
        let conn = open_readonly(db_path)?;
        let epoch_offset = detect_epoch_offset_seconds(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            epoch_offset,
        })
    }

    /// Create a new repository from an existing connection (useful for tests).
    pub fn from_connection(conn: Connection) -> Result<Self, MailMcpError> {
        let epoch_offset = detect_epoch_offset_seconds(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            epoch_offset,
        })
    }

    /// Get the detected epoch offset.
    pub fn epoch_offset(&self) -> i64 {
        self.epoch_offset
    }

    /// Get a reference to the underlying SQLite connection mutex.
    pub fn conn(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

impl MailRepository for SqliteMailRepository {
    fn search_messages(&self, params: SearchParams) -> Result<Vec<MessageRow>, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        search_messages(
            &conn,
            params.subject_query.as_deref(),
            params.date_from,
            params.date_to,
            params.sender.as_deref(),
            params.participant.as_deref(),
            params.account.as_deref(),
            params.allowed_accounts.as_deref(),
            params.mailbox.as_deref(),
            params.limit,
            params.offset,
        )
    }

    fn get_message(&self, id: i64) -> Result<Option<MessageRow>, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        get_message_by_id(&conn, id)
    }

    fn get_recipients(&self, message_id: i64) -> Result<Vec<(String, i32)>, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        get_recipients(&conn, message_id)
    }

    fn list_mailboxes(&self) -> Result<Vec<(i64, String)>, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        list_mailboxes(&conn)
    }

    fn list_accounts(&self) -> Result<Vec<AccountRow>, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        crate::db::accounts::list_accounts(&conn)
    }

    fn count_messages_in_mailbox(&self, mailbox_id: i64) -> Result<i64, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        count_messages_in_mailbox(&conn, mailbox_id)
    }

    fn detect_epoch_offset(&self) -> Result<i64, MailMcpError> {
        Ok(self.epoch_offset)
    }

    fn address_exists(&self, address: &str) -> Result<bool, MailMcpError> {
        let conn = self.conn.lock().unwrap();
        crate::db::messages::address_exists(&conn, address)
    }

    fn get_message_metadata(
        &self,
        message_ids: &[i64],
    ) -> Result<HashMap<i64, MessageMetadata>, MailMcpError> {
        if message_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn.lock().unwrap();
        let placeholders = std::iter::repeat_n("?", message_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r"
            SELECT
                m.ROWID,
                sm.summary,
                COUNT(att.ROWID)
            FROM messages m
            LEFT JOIN summaries sm ON sm.ROWID = m.summary
            LEFT JOIN attachments att ON att.message = m.ROWID
            WHERE m.ROWID IN ({placeholders})
            GROUP BY m.ROWID, sm.summary
            "
        );
        let params: Vec<&dyn rusqlite::ToSql> = message_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |row| {
            let attachment_count: i64 = row.get(2)?;
            Ok((
                row.get::<_, i64>(0)?,
                MessageMetadata {
                    summary: row.get(1)?,
                    attachment_count: u32::try_from(attachment_count.max(0)).unwrap_or(u32::MAX),
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

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_repo() -> (TempDir, SqliteMailRepository) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create a minimal valid SQLite database
        let conn = Connection::open(&db_path).unwrap();
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
                message_id TEXT,
                global_message_id INTEGER
            );
            CREATE TABLE message_global_data (
                ROWID INTEGER PRIMARY KEY,
                message_id INTEGER,
                message_id_header TEXT
            );
            CREATE TABLE recipients (
                message INTEGER REFERENCES messages,
                address INTEGER REFERENCES addresses,
                type INTEGER
            );
            "#,
        ).unwrap();
        drop(conn);

        let repo = SqliteMailRepository::new(&db_path).unwrap();
        (temp_dir, repo)
    }

    #[test]
    fn repo_creation_works() {
        let (_temp, repo) = make_test_repo();
        assert_eq!(repo.epoch_offset(), 0);
    }

    #[test]
    fn search_empty_db_returns_empty() {
        let (_temp, repo) = make_test_repo();
        let results = repo.search_messages(SearchParams::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn list_mailboxes_empty() {
        let (_temp, repo) = make_test_repo();
        let mailboxes = repo.list_mailboxes().unwrap();
        assert!(mailboxes.is_empty());
    }

    #[test]
    fn list_accounts_empty() {
        let (_temp, repo) = make_test_repo();
        let accounts = repo.list_accounts().unwrap();
        assert!(accounts.is_empty());
    }

    #[test]
    fn get_message_not_found() {
        let (_temp, repo) = make_test_repo();
        let msg = repo.get_message(999).unwrap();
        assert!(msg.is_none());
    }

    #[test]
    fn detect_epoch_offset_works() {
        let (_temp, repo) = make_test_repo();
        let offset = repo.detect_epoch_offset().unwrap();
        assert_eq!(offset, 0);
    }

    #[test]
    fn as_any_works() {
        let (_temp, repo) = make_test_repo();
        let any = repo.as_any();
        assert!(any.is::<SqliteMailRepository>());
    }
}
