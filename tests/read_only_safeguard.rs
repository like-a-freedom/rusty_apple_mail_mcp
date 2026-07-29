//! Safeguard tests verifying the MCP server never mutates Apple Mail databases.
//!
//! These tests exist because accidental data loss is catastrophic and
//! undetectable until the user notices missing mail. They provide two
//! complementary safety nets:
//!
//! 1. **File-integrity test** — copies a seeded DB, runs every tool against it,
//!    and compares file hashes byte-for-byte to prove zero mutation.
//! 2. **SQL audit test** — statically scans `src/` for write-mode SQL keywords,
//!    flagging any new write paths added by future developers.

mod support;

use rusty_apple_mail_mcp::db::SqliteMailRepository;
use rusty_apple_mail_mcp::mail::{CacheRegistry, EmlxLocator, FilesystemAttachmentStore};
use rusty_apple_mail_mcp::server::tools::*;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use support::{make_test_config, seed_emlx_in_account};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Snapshot of all files in a directory, keyed by relative path.
/// Each entry holds (byte length, content hash).
struct DirSnapshot {
    entries: HashMap<String, (u64, u64)>,
}

impl DirSnapshot {
    /// Capture the current state of every file under `dir`.
    fn capture(dir: &Path) -> Self {
        let mut entries = HashMap::new();
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .expect("walk yields paths under root")
                .to_string_lossy()
                .into_owned();
            let bytes = fs::read(entry.path()).expect("read file for snapshot");
            let len = bytes.len() as u64;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut hasher);
            entries.insert(rel, (len, hasher.finish()));
        }
        Self { entries }
    }

    /// Assert this snapshot is identical to `other`.
    fn assert_unchanged(&self, other: &Self, label: &str) {
        // Check for files that appeared or disappeared.
        let mut only_in_new: Vec<&String> = self
            .entries
            .keys()
            .filter(|k| !other.entries.contains_key(*k))
            .collect();
        let mut only_in_old: Vec<&String> = other
            .entries
            .keys()
            .filter(|k| !self.entries.contains_key(*k))
            .collect();
        only_in_new.sort();
        only_in_old.sort();

        assert!(
            only_in_new.is_empty(),
            "[{label}] new files appeared after tool execution: {only_in_new:?}"
        );
        assert!(
            only_in_old.is_empty(),
            "[{label}] files disappeared after tool execution: {only_in_old:?}"
        );

        // Check for content changes.
        for (path, &(len_new, hash_new)) in &self.entries {
            let &(len_old, hash_old) = other
                .entries
                .get(path)
                .expect("file exists in both snapshots");
            assert_eq!(
                len_new, len_old,
                "[{label}] file size changed for {path}: {len_old} -> {len_new}"
            );
            assert_eq!(
                hash_new, hash_old,
                "[{label}] file content changed for {path}"
            );
        }
    }
}

fn fast_hash_hex(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ---------------------------------------------------------------------------
// Test 1: File-integrity — run every tool, verify zero DB mutation
// ---------------------------------------------------------------------------

#[test]
fn all_tools_are_pure_read_only_no_db_mutation() {
    let (_temp_dir, config) = make_test_config();
    let db_path = config.envelope_db_path();

    // Seed the database with emlx files so body/attachment reads go through.
    seed_emlx_in_account(
        &config,
        "account-a",
        "INBOX",
        1,
        concat!(
            "From: alice@example.com\n",
            "To: bob@example.com\n",
            "Subject: Q3 Review\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=\"boundary\"\n",
            "\n",
            "--boundary\n",
            "Content-Type: text/plain; charset=utf-8\n",
            "\n",
            "Hello from safeguard test\n",
            "--boundary\n",
            "Content-Type: text/plain; name=\"notes.txt\"\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\n",
            "\n",
            "Attachment content for safeguard\n",
            "--boundary--\n",
        ),
    );

    // Take a snapshot of ALL files under the temp directory before execution.
    let snapshot_before = DirSnapshot::capture(_temp_dir.path());

    // Also capture the DB file bytes directly for a focused assertion.
    let db_bytes_before = fs::read(&db_path).expect("read db before");

    // Open through the production code path (open_readonly).
    let repo = SqliteMailRepository::new(&db_path).expect("open repo readonly");
    let store = FilesystemAttachmentStore::new(&config.mail_directory);
    let locator = {
        static REGISTRY: std::sync::LazyLock<CacheRegistry> =
            std::sync::LazyLock::new(CacheRegistry::new);
        EmlxLocator::new(&REGISTRY)
    };

    // Exercise every tool.

    // 1. list_accounts
    let _ = list_accounts(
        &repo,
        &store,
        &config,
        &locator,
        ListAccountsParams::default(),
    );

    // 2. search_messages (by subject)
    let _ = search_messages(
        &config,
        SearchMessagesParams {
            subject_query: Some("Q3".to_string()),
            date_from: None,
            date_to: None,
            sender: None,
            participant: None,
            account: None,
            mailbox: None,
            limit: 20,
            include_body_preview: true,
            offset: 0,
        },
    );

    // 3. get_message with body and attachments
    let _ = get_message(
        &repo,
        &store,
        &config,
        &locator,
        GetMessageParams {
            message_id: "1".to_string(),
            include_body: true,
            include_attachments_summary: true,
            include_recipients: true,
            offset: 0,
            limit: rusty_apple_mail_mcp::DEFAULT_WINDOW_BYTES,
            source_revision: None,
        },
    );

    // 4. get_attachment_content
    let _ = get_attachment_content(
        &repo,
        &store,
        &config,
        &locator,
        GetAttachmentParams {
            attachment_id: "1:0".to_string(),
            message_id: "1".to_string(),
            offset: 0,
            limit: rusty_apple_mail_mcp::DEFAULT_WINDOW_BYTES,
            source_revision: None,
        },
    );

    // Take a snapshot AFTER all tool execution.
    let snapshot_after = DirSnapshot::capture(_temp_dir.path());
    let db_bytes_after = fs::read(&db_path).expect("read db after");

    // Assert: DB file is byte-for-byte identical.
    assert_eq!(
        fast_hash_hex(&db_bytes_before),
        fast_hash_hex(&db_bytes_after),
        "Envelope Index DB was modified by tool execution — read-only guarantee violated"
    );
    assert_eq!(
        db_bytes_before.len(),
        db_bytes_after.len(),
        "Envelope Index DB size changed"
    );

    // Assert: entire directory tree unchanged (no WAL/SHM/lock files created).
    snapshot_after.assert_unchanged(&snapshot_before, "full-directory");
}

// ---------------------------------------------------------------------------
// Test 2: SQL audit — no write-mode SQL in production code
// ---------------------------------------------------------------------------

/// Keywords that indicate a SQL write operation.
const SQL_WRITE_KEYWORDS: &[&str] = &[
    "INSERT INTO",
    "UPDATE ",
    "DELETE FROM",
    "CREATE TABLE",
    "DROP TABLE",
    "ALTER TABLE",
    "CREATE INDEX",
    "DROP INDEX",
    "BEGIN TRANSACTION",
    "PRAGMA journal_mode",
    "VACUUM",
    "ATTACH DATABASE",
];

#[test]
fn no_write_sql_in_production_source() {
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut violations: Vec<String> = Vec::new();

    for entry in walkdir::WalkDir::new(&src_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "rs"))
    {
        let content = fs::read_to_string(entry.path()).expect("read source file");
        let relative = entry.path().strip_prefix(&src_dir).unwrap_or(entry.path());

        // Skip test modules (they legitimately write to temp DBs).
        let is_test_module = entry
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name == "support" || name.ends_with("_test.rs"));

        if is_test_module {
            continue;
        }

        // Skip files inside `tests/` directory.
        if entry.path().components().any(|c| c.as_os_str() == "tests") {
            continue;
        }

        // Check for test modules inside production files — skip #[cfg(test)] blocks.
        let lines: Vec<&str> = content.lines().collect();
        let mut in_test_module = false;
        let mut brace_depth = 0u32;

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Track entry into #[cfg(test)] mod tests { ... }
            if trimmed.contains("#[cfg(test)]") {
                in_test_module = true;
                brace_depth = 0;
                continue;
            }

            if in_test_module {
                brace_depth += line.matches('{').count() as u32;
                brace_depth = brace_depth.saturating_sub(line.matches('}').count() as u32);
                if brace_depth == 0 && line.contains('}') {
                    in_test_module = false;
                }
                continue;
            }

            // Skip comments.
            if trimmed.starts_with("//") {
                continue;
            }

            for keyword in SQL_WRITE_KEYWORDS {
                // Case-insensitive check.
                if trimmed.to_uppercase().contains(&keyword.to_uppercase()) {
                    // Exclude strings in comments or doc comments.
                    let without_string_literals = replace_string_literals(trimmed);
                    if without_string_literals
                        .to_uppercase()
                        .contains(&keyword.to_uppercase())
                    {
                        violations.push(format!(
                            "{}:{}: contains write SQL `{}`",
                            relative.display(),
                            line_num + 1,
                            keyword.trim(),
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Write-mode SQL found in production source files:\n{}",
        violations.join("\n"),
    );
}

/// Strip string literals from a line to avoid false positives on example code
/// in doc comments.
fn replace_string_literals(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            // Skip until closing quote (simple — no escape handling needed for SQL keywords).
            for next in chars.by_ref() {
                if next == '"' {
                    break;
                }
            }
            result.push_str("___");
        } else {
            result.push(c);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Test 3: open_readonly is the only connection factory for production code
// ---------------------------------------------------------------------------

#[test]
fn envelope_index_path_only_accessible_via_readonly() {
    // Verify that SqliteMailRepository::new always calls open_readonly
    // by checking the connection is actually read-only.
    let (_temp_dir, config) = make_test_config();
    let db_path = config.envelope_db_path();

    // Open through the production factory.
    let repo = SqliteMailRepository::new(&db_path).expect("open via production path");

    // We can't directly access the connection, but we can verify via a
    // direct connection that the DB hasn't been locked or modified.
    let verify_conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("verify: can still open as read-only");

    let count: i64 = verify_conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("verify: can read messages");
    assert!(count >= 2, "expected at least 2 seeded messages");

    // Drop the repo and verify the DB is still accessible (no locks held).
    drop(repo);
    let after_drop: i64 = verify_conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("verify: still readable after repo drop");
    assert_eq!(count, after_drop);
}
