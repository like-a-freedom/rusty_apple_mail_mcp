//! Test support utilities for integration tests.

use rusqlite::Connection;
use rusty_apple_mail_mcp::accounts::AccountMetadata;
use rusty_apple_mail_mcp::config::MailConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

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

        -- Seed data
        INSERT INTO subjects VALUES (1, 'Q3 Review'), (2, 'Budget Planning');
        INSERT INTO addresses VALUES (1, 'alice@example.com'), (2, 'bob@example.com');
        INSERT INTO sender_addresses VALUES (1, 1);
        INSERT INTO mailboxes VALUES
            (1, 'imap://account-a/INBOX'),
            (2, 'ews://account-b/Inbox');
        
        -- Use CoreData epoch: 2024-09-15 = Unix timestamp - 978307200
        INSERT INTO message_global_data VALUES (10, 111, '<msg1@mail>');
        INSERT INTO message_global_data VALUES (20, 222, '<msg2@mail>');
        INSERT INTO messages VALUES (1, 1, 1, 1, 748051200, 748051200, '<msg1@mail>', 10);
        INSERT INTO messages VALUES (2, 2, 1, 2, 766627200, 766627200, '<msg2@mail>', 20);
        
        INSERT INTO recipients VALUES (1, 2, 1), (2, 2, 1);
        "#,
    )
    .expect("seed test schema");
    conn
}

/// Build a temporary Apple Mail-like directory and a matching config for tests.
pub fn make_test_config() -> (TempDir, MailConfig) {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    let mail_version = "V10".to_string();
    let db_dir = mail_directory.join(&mail_version).join("MailData");
    std::fs::create_dir_all(&db_dir).expect("mail data dir");
    std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");

    let config = MailConfig::from_parts_with_accounts(
        mail_directory,
        mail_version,
        None,
        make_account_metadata(),
    )
    .expect("config");
    (temp_dir, config)
}

/// Build a temporary Apple Mail-like directory and a config restricted to one account.
pub fn make_restricted_test_config(allowed_account_id: &str) -> (TempDir, MailConfig) {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    let mail_version = "V10".to_string();
    let db_dir = mail_directory.join(&mail_version).join("MailData");
    std::fs::create_dir_all(&db_dir).expect("mail data dir");
    std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db file");

    let config = MailConfig::from_parts_with_accounts(
        mail_directory,
        mail_version,
        Some(vec![allowed_account_id.to_string()]),
        make_account_metadata(),
    )
    .expect("restricted config");
    (temp_dir, config)
}

/// Create synthetic friendly account metadata for tests.
pub fn make_account_metadata() -> HashMap<String, AccountMetadata> {
    HashMap::from([
        (
            "imap://account-a".to_string(),
            AccountMetadata {
                account_id: "imap://account-a".to_string(),
                account_name: Some("Personal Gmail".to_string()),
                email: Some("solovey.anton@gmail.com".to_string()),
                username: Some("solovey.anton@gmail.com".to_string()),
                source_identifier: "account-a".to_string(),
                account_type: "imap".to_string(),
            },
        ),
        (
            "ews://account-b".to_string(),
            AccountMetadata {
                account_id: "ews://account-b".to_string(),
                account_name: Some("Kaspersky".to_string()),
                email: Some("anton.solovey@kaspersky.com".to_string()),
                username: Some("KL\\solovey".to_string()),
                source_identifier: "account-b".to_string(),
                account_type: "ews".to_string(),
            },
        ),
    ])
}

/// Write an `.emlx` file into an account-specific synthetic mailbox tree for a message rowid.
pub fn seed_emlx_in_account(
    config: &MailConfig,
    account_dir: &str,
    mailbox_name: &str,
    rowid: i64,
    raw_email: &str,
) -> PathBuf {
    let messages_dir = config
        .mail_directory
        .join(&config.mail_version)
        .join(account_dir)
        .join(format!("{mailbox_name}.mbox"))
        .join("Messages");
    std::fs::create_dir_all(&messages_dir).expect("messages dir");

    let emlx_path = messages_dir.join(format!("{rowid}.emlx"));
    let emlx_content = format!("{}\n{}", raw_email.len(), raw_email);
    std::fs::write(&emlx_path, emlx_content).expect("write emlx");
    emlx_path
}

/// Write an `.emlx` file into a nested mailbox tree with optional UUID/Data fanout.
pub fn seed_emlx_in_nested_mailbox(
    config: &MailConfig,
    account_dir: &str,
    mailbox_segments: &[&str],
    file_stem: &str,
    raw_email: &str,
) -> PathBuf {
    let mut mailbox_dir = config
        .mail_directory
        .join(&config.mail_version)
        .join(account_dir);
    for segment in mailbox_segments {
        mailbox_dir = mailbox_dir.join(format!("{segment}.mbox"));
    }

    let messages_dir = mailbox_dir
        .join("UUID-1234")
        .join("Data")
        .join("4")
        .join("8")
        .join("Messages");
    std::fs::create_dir_all(&messages_dir).expect("messages dir");

    let emlx_path = messages_dir.join(format!("{file_stem}.emlx"));
    let emlx_content = format!("{}\n{}", raw_email.len(), raw_email);
    std::fs::write(&emlx_path, emlx_content).expect("write emlx");
    emlx_path
}
