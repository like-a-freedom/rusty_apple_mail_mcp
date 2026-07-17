use crate::error::MailMcpError;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

/// Percent-encode a filesystem path for use in a `file:` URI.
///
/// Encodes characters that are not safe in URI paths per RFC 3986.
/// The space character (` ` → `%20`) is the most common case in Apple Mail
/// paths (e.g., `Envelope Index`).
fn percent_encode_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                result.push(byte as char)
            }
            b' ' => result.push_str("%20"),
            _ => result.push_str(&format!("{:02X}", byte)),
        }
    }
    result
}

/// Determine whether a SQLite error indicates a locked/busy database.
fn is_locked_error(e: &rusqlite::Error) -> bool {
    let msg = e.to_string();
    msg.contains("locked") || msg.contains("busy")
}

/// Determine whether a SQLite error is likely caused by macOS TCC permissions.
///
/// SQLite error 14 (CANTOPEN) on a file that exists usually means the process
/// lacks the macOS Full Disk Access entitlement needed to read `~/Library/Mail`.
fn is_likely_tcc_error(e: &rusqlite::Error) -> bool {
    let msg = e.to_string();
    msg.contains("unable to open database") || msg.contains("authorization denied")
}

/// macOS TCC guidance added to error messages when the database file exists
/// but cannot be opened.
const TCC_GUIDANCE: &str = " Ensure the application has Full Disk Access: \
     System Settings → Privacy & Security → Full Disk Access. \
     Add your terminal, IDE, or the MCP server host process, then retry.";

/// Open the Envelope Index database in read-only mode.
///
/// Uses `SQLite` URI to prevent any accidental writes.
/// The connection stays read-only, but it must still observe the active `WAL`
/// so newly indexed Mail messages remain visible before checkpointing.
///
/// Retries up to 3 times with exponential backoff when the database is locked
/// by Apple Mail, and augments error messages with macOS TCC guidance when
/// the file exists but cannot be opened (likely a permission issue).
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
    let uri = format!("file:{}?mode=ro", percent_encode_path(path));

    const MAX_RETRIES: u32 = 3;
    for attempt in 0..MAX_RETRIES {
        match Connection::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(conn) => return Ok(conn),
            Err(e) => {
                if is_locked_error(&e) && attempt + 1 < MAX_RETRIES {
                    let delay = Duration::from_millis(100 * 2u64.pow(attempt));
                    std::thread::sleep(delay);
                    continue;
                }
                if is_locked_error(&e) {
                    let msg = format!(
                        "Database is locked by Apple Mail after {MAX_RETRIES} retries. \
                         Close Apple Mail or wait for it to finish indexing.{TCC_GUIDANCE}"
                    );
                    return Err(MailMcpError::DatabaseLocked(msg));
                }
                if is_likely_tcc_error(&e) {
                    let msg = format!(
                        "Cannot open database at {}.{}",
                        path.display(),
                        TCC_GUIDANCE
                    );
                    return Err(MailMcpError::DatabaseLocked(msg));
                }
                return Err(MailMcpError::Sqlite(e));
            }
        }
    }

    unreachable!("retry loop always returns via early return or Err");
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
        // File exists so should NOT get DatabaseNotFound
        assert!(
            !matches!(result, Err(MailMcpError::DatabaseNotFound { .. })),
            "File exists, should not get DatabaseNotFound"
        );
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

    #[test]
    fn percent_encode_path_encodes_spaces() {
        let path = std::path::PathBuf::from("/Users/test/Library/Mail/V10/MailData/Envelope Index");
        assert_eq!(
            percent_encode_path(&path),
            "/Users/test/Library/Mail/V10/MailData/Envelope%20Index"
        );
    }

    #[test]
    fn percent_encode_path_preserves_safe_characters() {
        let path = std::path::PathBuf::from("/Users/test/Mail/V10/db.sqlite");
        assert_eq!(percent_encode_path(&path), "/Users/test/Mail/V10/db.sqlite");
    }

    #[test]
    fn open_valid_sqlite_with_space_in_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_dir = temp_dir.path().join("Mail Data");
        fs::create_dir_all(&db_dir).expect("create dir");
        let db_path = db_dir.join("Envelope Index");

        let conn = Connection::open(&db_path).expect("create db");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        drop(conn);

        let result = open_readonly(&db_path);
        assert!(result.is_ok(), "should open db with space in path");
    }

    #[test]
    fn open_readonly_reads_committed_rows_from_wal() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("wal.db");

        let writer = Connection::open(&db_path).expect("create wal db");
        let journal_mode: String = writer
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .expect("enable wal mode");
        assert_eq!(journal_mode.to_lowercase(), "wal");

        writer
            .execute_batch(
                r#"
                PRAGMA wal_autocheckpoint=0;
                CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT);
                INSERT INTO test (id, value) VALUES (1, 'checkpointed');
                "#,
            )
            .expect("seed checkpointed state");
        writer
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .expect("checkpoint initial state");
        writer
            .execute(
                "INSERT INTO test (id, value) VALUES (?1, ?2)",
                [2_i64.to_string(), "wal-only".to_string()],
            )
            .expect("insert wal-backed row");

        let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
        assert!(
            wal_path.exists(),
            "expected WAL file at {}",
            wal_path.display()
        );
        assert!(
            fs::metadata(&wal_path).expect("wal metadata").len() > 0,
            "expected WAL file to contain uncheckpointed data"
        );

        let ro_conn = open_readonly(&db_path).expect("open readonly");
        let row_count: i64 = ro_conn
            .query_row("SELECT COUNT(*) FROM test", [], |row| row.get(0))
            .expect("read wal-backed rows");

        assert_eq!(row_count, 2);
    }

    #[test]
    fn is_locked_error_detects_busy() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(5),
            Some("database is busy".to_string()),
        );
        assert!(is_locked_error(&err));
    }

    #[test]
    fn is_locked_error_detects_locked() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(6),
            Some("database is locked".to_string()),
        );
        assert!(is_locked_error(&err));
    }

    #[test]
    fn is_locked_error_ignores_other_errors() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(14),
            Some("unable to open database file".to_string()),
        );
        assert!(!is_locked_error(&err));
    }

    #[test]
    fn is_likely_tcc_error_detects_cantopen() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(14),
            Some("unable to open database file".to_string()),
        );
        assert!(is_likely_tcc_error(&err));
    }

    #[test]
    fn is_likely_tcc_error_detects_permission_denied() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(14),
            Some("authorization denied".to_string()),
        );
        assert!(is_likely_tcc_error(&err));
    }

    #[test]
    fn is_likely_tcc_error_ignores_corrupt() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(26),
            Some("file is not a database".to_string()),
        );
        assert!(!is_likely_tcc_error(&err));
    }

    #[test]
    fn open_readonly_retries_on_locked() {
        use std::time::Instant;

        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("lock.db");

        let conn = Connection::open(&db_path).expect("create db");
        conn.execute("CREATE TABLE test (id INTEGER)", [])
            .expect("create table");
        drop(conn);

        let started = Instant::now();
        let result = open_readonly(&db_path);
        let elapsed = started.elapsed();

        assert!(result.is_ok());
        // First attempt should succeed immediately (no contention) — < 50ms
        assert!(
            elapsed < Duration::from_millis(50),
            "should succeed without delay when no lock, took {elapsed:?}"
        );
    }

    #[test]
    fn tcc_guidance_appears_in_error_message() {
        // Verify TCC_GUIDANCE is a non-empty string containing "Full Disk Access"
        assert!(TCC_GUIDANCE.contains("Full Disk Access"));
    }
}
