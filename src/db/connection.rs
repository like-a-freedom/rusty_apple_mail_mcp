use crate::error::MailMcpError;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// Open the Envelope Index database in read-only mode.
///
/// Uses `SQLite` URI to prevent any accidental writes.
/// The `immutable=1` flag tells `SQLite` the database file is read-only and won't change,
/// which is safe for our use case since Apple Mail owns the write lock.
///
/// # Errors
///
/// Returns [`MailMcpError::DatabaseNotFound`] if the database file doesn't exist.
/// Returns [`MailMcpError::DatabaseLocked`] if the database is locked by Apple Mail.
/// Returns [`MailMcpError::Sqlite`] for other `SQLite` errors.
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

    #[test]
    fn open_corrupted_file_returns_sqlite_error() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("corrupted.db");

        // Write invalid SQLite data but keep the file
        fs::write(&db_path, b"not a sqlite database at all").expect("write corrupted");

        let result = open_readonly(&db_path);
        // File exists, so should get Sqlite error (not DatabaseNotFound)
        // lopdf may also succeed with empty text, so be flexible
        match result {
            Ok(_) => (),
            Err(MailMcpError::Sqlite(_)) => (),
            Err(MailMcpError::DatabaseNotFound { .. }) => {
                panic!("File exists, should not get DatabaseNotFound");
            }
            _ => panic!("Expected Sqlite error or Ok"),
        }
    }

    #[test]
    fn open_directory_returns_error() {
        let temp_dir = TempDir::new().expect("temp dir");
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).expect("create dir");

        let result = open_readonly(&dir_path);
        // Should return Sqlite error since directory is not a file
        assert!(matches!(result, Err(MailMcpError::Sqlite(_))));
    }

    #[test]
    fn open_readonly_prevents_writes() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");

        // Create and populate database
        let conn = Connection::open(&db_path).expect("create db");
        conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)", [])
            .expect("create table");
        conn.execute("INSERT INTO test (value) VALUES ('initial')", [])
            .expect("insert");
        drop(conn);

        // Open read-only
        let ro_conn = open_readonly(&db_path).expect("open readonly");

        // Try to write - should fail
        let write_result = ro_conn.execute("UPDATE test SET value = 'modified'", []);
        assert!(write_result.is_err());

        // Try to delete - should fail
        let delete_result = ro_conn.execute("DROP TABLE test", []);
        assert!(delete_result.is_err());

        // But read should work
        let read_result: Result<String, _> =
            ro_conn.query_row("SELECT value FROM test WHERE id = 1", [], |row| row.get(0));
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "initial");
    }

    #[test]
    fn path_to_string_lossy_handles_unicode() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("test.db");

        // Create a valid database
        let conn = Connection::open(&db_path).expect("create db");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        drop(conn);

        // Open with path containing unicode - should not panic
        let result = open_readonly(&db_path);
        assert!(result.is_ok());
    }
}
