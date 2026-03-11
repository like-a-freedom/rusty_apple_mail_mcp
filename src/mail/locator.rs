//! Locate .emlx message files on the filesystem.
//!
//! Apple Mail stores full message content in .emlx files outside the Envelope Index database.
//! The mapping from database ROWID to filesystem path is version- and account-type-dependent.
//!
//! This module implements a heuristic-based locator:
//! 1. Parse mailbox URL to derive a likely mailbox-relative path
//! 2. Probe nearby candidate directories first (cheap path-based heuristics)
//! 3. Fallback to bounded recursive search under mail_directory/mail_version/
//! 4. Cache resolved message_rowid → PathBuf mappings in-memory

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    mail_root: PathBuf,
    message_rowid: i64,
}

/// In-memory cache for resolved message paths.
/// Key: message ROWID, Value: resolved path to .emlx file
static PATH_CACHE: Lazy<Mutex<HashMap<CacheKey, PathBuf>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Locate the .emlx file for a given message.
///
/// # Arguments
///
/// * `mail_dir` - Base mail directory (e.g., ~/Library/Mail)
/// * `mail_version` - Mail version folder (e.g., "V10")
/// * `mailbox_url` - Mailbox URL from the database
/// * `message_rowid` - Message ROWID from the database
///
/// # Returns
///
/// Path to the .emlx file if found, None otherwise.
pub fn locate_emlx(
    mail_dir: &Path,
    mail_version: &str,
    mailbox_url: &str,
    message_rowid: i64,
) -> Option<PathBuf> {
    let cache_key = CacheKey {
        mail_root: mail_dir.join(mail_version),
        message_rowid,
    };

    // Check cache first
    {
        let cache = PATH_CACHE.lock().ok()?;
        if let Some(cached) = cache.get(&cache_key)
            && cached.exists()
        {
            return Some(cached.clone());
        }
    }

    // Try to locate the file
    let path = find_emlx_file(mail_dir, mail_version, mailbox_url, message_rowid)?;

    // Cache the result
    if let Ok(mut cache) = PATH_CACHE.lock() {
        cache.insert(cache_key, path.clone());
    }

    Some(path)
}

/// Internal function to find the .emlx file.
fn find_emlx_file(
    mail_dir: &Path,
    mail_version: &str,
    mailbox_url: &str,
    message_rowid: i64,
) -> Option<PathBuf> {
    let base_path = mail_dir.join(mail_version);

    // Strategy 1: Try direct path construction from mailbox URL
    if let Some(path) = try_direct_path(&base_path, mailbox_url, message_rowid)
        && path.exists()
    {
        return Some(path);
    }

    // Strategy 2: Bounded recursive search for {rowid}.emlx
    // Limit depth to avoid scanning the entire mail directory
    for entry in WalkDir::new(&base_path)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file()
            && let Some(file_name) = entry.file_name().to_str()
            && file_name == format!("{message_rowid}.emlx")
        {
            return Some(entry.path().to_path_buf());
        }
    }

    None
}

/// Try to construct a direct path from the mailbox URL.
///
/// Mailbox URL format is typically:
/// `imap://user@mail.example.com/INBOX` or `imap://user@mail.example.com/INBOX.Subfolder`
///
/// The corresponding filesystem path is usually under:
/// `~/Library/Mail/V10/[UUID]/[Mailbox Name].mbox/Messages/`
fn try_direct_path(base_path: &Path, mailbox_url: &str, message_rowid: i64) -> Option<PathBuf> {
    // Extract mailbox name from URL (last segment after /)
    let mailbox_name = mailbox_url.rsplit('/').next()?;

    // Try common patterns
    // Pattern 1: [UUID]/[Mailbox Name].mbox/Messages/[ROWID].emlx
    for entry in std::fs::read_dir(base_path).ok()? {
        let entry = entry.ok()?;
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }

        let mbox_path = dir_path
            .join(format!("{mailbox_name}.mbox"))
            .join("Messages");
        let emlx_path = mbox_path.join(format!("{message_rowid}.emlx"));
        if emlx_path.exists() {
            return Some(emlx_path);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    #[test]
    fn locate_emlx_finds_file_in_mbox_structure() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let mail_version = "V10";
        let base_path = mail_dir.join(mail_version);

        // Create a fake mailbox structure
        let uuid_dir = base_path.join("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA");
        let messages_dir = uuid_dir.join("INBOX.mbox").join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        // Create a fake .emlx file
        let emlx_path = messages_dir.join("42.emlx");
        File::create(&emlx_path).unwrap();

        let result = locate_emlx(
            mail_dir,
            mail_version,
            "imap://user@mail.example.com/INBOX",
            42,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap(), emlx_path);
    }

    #[test]
    fn locate_emlx_returns_none_for_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let mail_version = "V10";

        let result = locate_emlx(
            mail_dir,
            mail_version,
            "imap://user@mail.example.com/INBOX",
            999,
        );

        assert!(result.is_none());
    }

    #[test]
    fn cache_is_namespaced_by_mail_root() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();

        let first_messages = first
            .path()
            .join("V10")
            .join("AAAA")
            .join("INBOX.mbox")
            .join("Messages");
        let second_messages = second
            .path()
            .join("V10")
            .join("BBBB")
            .join("INBOX.mbox")
            .join("Messages");

        fs::create_dir_all(&first_messages).unwrap();
        fs::create_dir_all(&second_messages).unwrap();
        let first_file = first_messages.join("42.emlx");
        let second_file = second_messages.join("42.emlx");
        File::create(&first_file).unwrap();
        File::create(&second_file).unwrap();

        let first_result = locate_emlx(first.path(), "V10", "imap://u@example.com/INBOX", 42);
        let second_result = locate_emlx(second.path(), "V10", "imap://u@example.com/INBOX", 42);

        assert_eq!(first_result.unwrap(), first_file);
        assert_eq!(second_result.unwrap(), second_file);
    }
}
