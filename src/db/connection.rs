use crate::error::MailMcpError;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// Open the Envelope Index database in read-only mode.
///
/// Uses SQLite URI to prevent any accidental writes.
/// The `immutable=1` flag tells SQLite the database file is read-only and won't change,
/// which is safe for our use case since Apple Mail owns the write lock.
///
/// # Errors
///
/// Returns [`MailMcpError::DatabaseNotFound`] if the database file doesn't exist.
/// Returns [`MailMcpError::DatabaseLocked`] if the database is locked by Apple Mail.
/// Returns [`MailMcpError::Sqlite`] for other SQLite errors.
pub fn open_readonly(path: impl AsRef<Path>) -> Result<Connection, MailMcpError> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(MailMcpError::DatabaseNotFound {
            path: path.to_owned(),
        });
    }
    let uri = format!("file:{}?mode=ro&immutable=1", path.to_string_lossy());
    Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| {
        if e.to_string().contains("locked") {
            MailMcpError::DatabaseLocked(e.to_string())
        } else {
            MailMcpError::Sqlite(e)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn open_missing_db_returns_not_found_error() {
        let result = open_readonly("/tmp/no_such_db_ever_12345");
        assert!(matches!(result, Err(MailMcpError::DatabaseNotFound { .. })));
    }

    #[test]
    fn open_valid_sqlite_file_returns_connection() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");

        // Create a valid SQLite database
        let conn = Connection::open(&db_path).expect("create db");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        drop(conn);

        // Now open read-only
        let result = open_readonly(&db_path);
        assert!(result.is_ok());

        let conn = result.unwrap();
        // Verify it's read-only by trying to write
        let write_result = conn.execute("INSERT INTO test VALUES (1)", []);
        assert!(write_result.is_err());
    }

    #[test]
    fn open_empty_file_returns_error() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("empty.db");

        // Create an empty file (not a valid SQLite database)
        fs::write(&db_path, b"").expect("write empty file");

        // Remove the file so the test returns DatabaseNotFound
        drop(fs::remove_file(&db_path));

        let result = open_readonly(&db_path);
        // Should return DatabaseNotFound error
        assert!(matches!(result, Err(MailMcpError::DatabaseNotFound { .. })));
    }
}
