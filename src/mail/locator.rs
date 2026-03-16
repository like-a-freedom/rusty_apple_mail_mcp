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
use std::fs;
use std::io::{BufRead, BufReader};
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
static PATH_CACHE: Lazy<Mutex<HashMap<CacheKey, PathBuf>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Default)]
struct MailboxIndex {
    by_header: HashMap<String, PathBuf>,
    by_stem: HashMap<String, PathBuf>,
}

static MAILBOX_INDEX_CACHE: Lazy<Mutex<HashMap<PathBuf, MailboxIndex>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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
    locate_emlx_with_hints(
        mail_dir,
        mail_version,
        mailbox_url,
        message_rowid,
        &[message_rowid.to_string()],
        None,
    )
}

/// Locate the `.emlx` file using fast exact-path hints first, then a mailbox-local cached index.
pub fn locate_emlx_with_hints(
    mail_dir: &Path,
    mail_version: &str,
    mailbox_url: &str,
    message_rowid: i64,
    numeric_hints: &[String],
    message_id_header: Option<&str>,
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

    let mut candidate_ids = numeric_hints.to_vec();
    if !candidate_ids
        .iter()
        .any(|candidate| candidate == &message_rowid.to_string())
    {
        candidate_ids.push(message_rowid.to_string());
    }

    let mailbox_dirs = candidate_mailbox_directories(&mail_dir.join(mail_version), mailbox_url);

    if let Some(header) = message_id_header {
        for mailbox_dir in &mailbox_dirs {
            if let Some(path) = lookup_mailbox_header(mailbox_dir, header) {
                if let Ok(mut cache) = PATH_CACHE.lock() {
                    cache.insert(cache_key.clone(), path.clone());
                }
                return Some(path);
            }
        }
    }

    if let Some(path) = find_emlx_file(mail_dir, mail_version, mailbox_url, &candidate_ids, false) {
        if let Ok(mut cache) = PATH_CACHE.lock() {
            cache.insert(cache_key, path.clone());
        }
        return Some(path);
    }

    for mailbox_dir in &mailbox_dirs {
        if let Some(path) = lookup_mailbox_index(mailbox_dir, &candidate_ids, message_id_header) {
            if let Ok(mut cache) = PATH_CACHE.lock() {
                cache.insert(cache_key.clone(), path.clone());
            }
            return Some(path);
        }
    }

    let path = find_emlx_file(mail_dir, mail_version, mailbox_url, &candidate_ids, true)?;

    if let Ok(mut cache) = PATH_CACHE.lock() {
        cache.insert(cache_key, path.clone());
    }

    Some(path)
}

/// Locate the `.emlx` file using cache and direct-path heuristics only.
///
/// This variant intentionally avoids recursive directory walking and is suitable
/// for list/search operations where latency matters more than exhaustive lookup.
pub fn locate_emlx_quick(
    mail_dir: &Path,
    mail_version: &str,
    mailbox_url: &str,
    message_rowid: i64,
) -> Option<PathBuf> {
    locate_emlx_quick_with_hints(
        mail_dir,
        mail_version,
        mailbox_url,
        message_rowid,
        &[message_rowid.to_string()],
        None,
    )
}

/// Locate the `.emlx` file using fast hints and mailbox-local indexes only.
///
/// This variant avoids recursive directory walking, making it suitable for list
/// operations that still need reliable matching by `Message-ID` or alternate
/// numeric stems.
pub fn locate_emlx_quick_with_hints(
    mail_dir: &Path,
    mail_version: &str,
    mailbox_url: &str,
    message_rowid: i64,
    numeric_hints: &[String],
    message_id_header: Option<&str>,
) -> Option<PathBuf> {
    let cache_key = CacheKey {
        mail_root: mail_dir.join(mail_version),
        message_rowid,
    };

    {
        let cache = PATH_CACHE.lock().ok()?;
        if let Some(cached) = cache.get(&cache_key)
            && cached.exists()
        {
            return Some(cached.clone());
        }
    }

    let mut candidate_ids = numeric_hints.to_vec();
    if !candidate_ids
        .iter()
        .any(|candidate| candidate == &message_rowid.to_string())
    {
        candidate_ids.push(message_rowid.to_string());
    }

    let mailbox_dirs = candidate_mailbox_directories(&mail_dir.join(mail_version), mailbox_url);

    if let Some(header) = message_id_header {
        for mailbox_dir in &mailbox_dirs {
            if let Some(path) = lookup_mailbox_header(mailbox_dir, header) {
                if let Ok(mut cache) = PATH_CACHE.lock() {
                    cache.insert(cache_key.clone(), path.clone());
                }
                return Some(path);
            }
        }
    }

    if let Some(path) = find_emlx_file(mail_dir, mail_version, mailbox_url, &candidate_ids, false) {
        if let Ok(mut cache) = PATH_CACHE.lock() {
            cache.insert(cache_key.clone(), path.clone());
        }
        return Some(path);
    }

    for mailbox_dir in &mailbox_dirs {
        if let Some(path) = lookup_mailbox_index(mailbox_dir, &candidate_ids, message_id_header) {
            if let Ok(mut cache) = PATH_CACHE.lock() {
                cache.insert(cache_key.clone(), path.clone());
            }
            return Some(path);
        }
    }

    None
}

/// Internal function to find the .emlx file.
fn find_emlx_file(
    mail_dir: &Path,
    mail_version: &str,
    mailbox_url: &str,
    candidate_ids: &[String],
    allow_recursive_scan: bool,
) -> Option<PathBuf> {
    let base_path = mail_dir.join(mail_version);
    let mailbox_dirs = candidate_mailbox_directories(&base_path, mailbox_url);

    // Strategy 1: Try direct path construction from mailbox URL
    if let Some(path) = try_direct_path(&mailbox_dirs, candidate_ids)
        && path.exists()
    {
        return Some(path);
    }

    if allow_recursive_scan {
        for mailbox_dir in mailbox_dirs {
            for entry in WalkDir::new(&mailbox_dir)
                .max_depth(8)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file()
                    && let Some(file_name) = entry.file_name().to_str()
                    && matches_candidate(file_name, candidate_ids)
                {
                    return Some(entry.path().to_path_buf());
                }
            }
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
fn try_direct_path(mailbox_dirs: &[PathBuf], candidate_ids: &[String]) -> Option<PathBuf> {
    mailbox_dirs
        .iter()
        .find_map(|mailbox_dir| find_candidate_in_mailbox_dir(mailbox_dir, candidate_ids))
}

fn parse_mailbox_url(mailbox_url: &str) -> Option<(&str, Vec<String>)> {
    let scheme_end = mailbox_url.find("://")?;
    let rest = &mailbox_url[scheme_end + 3..];
    let slash = rest.find('/')?;
    let account_id = &rest[..slash];
    let path_part = &rest[slash + 1..];

    let segments = path_part.split('/').map(percent_decode).collect::<Vec<_>>();
    Some((account_id, segments))
}

fn candidate_mailbox_directories(base_path: &Path, mailbox_url: &str) -> Vec<PathBuf> {
    let Some((account_id, segments)) = parse_mailbox_url(mailbox_url) else {
        return Vec::new();
    };

    let mut roots = vec![base_path.join(account_id)];
    if let Ok(entries) = fs::read_dir(base_path) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() && !roots.iter().any(|root| root == &path) {
                roots.push(path);
            }
        }
    }

    roots
        .into_iter()
        .map(|root| build_mailbox_path(root, &segments))
        .collect()
}

fn build_mailbox_path(mut mailbox_dir: PathBuf, segments: &[String]) -> PathBuf {
    for segment in segments {
        mailbox_dir = mailbox_dir.join(format!("{}.mbox", percent_decode(segment)));
    }
    mailbox_dir
}

fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut decoded = String::with_capacity(segment.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&segment[index + 1..index + 3], 16)
        {
            decoded.push(value as char);
            index += 3;
            continue;
        }

        decoded.push(bytes[index] as char);
        index += 1;
    }

    decoded
}

fn file_name_candidates(candidate_id: &str) -> [String; 2] {
    [
        format!("{candidate_id}.emlx"),
        format!("{candidate_id}.partial.emlx"),
    ]
}

fn matches_candidate(file_name: &str, candidate_ids: &[String]) -> bool {
    candidate_ids.iter().any(|candidate_id| {
        let [emlx, partial] = file_name_candidates(candidate_id);
        file_name == emlx || file_name == partial
    })
}

fn find_candidate_in_mailbox_dir(mailbox_dir: &Path, candidate_ids: &[String]) -> Option<PathBuf> {
    for candidate_id in candidate_ids {
        let [emlx_name, partial_name] = file_name_candidates(candidate_id);

        for file_name in [&emlx_name, &partial_name] {
            let direct_messages = mailbox_dir.join("Messages").join(file_name);
            if direct_messages.exists() {
                return Some(direct_messages);
            }
        }

        for entry in fs::read_dir(mailbox_dir).ok()?.filter_map(Result::ok) {
            let child = entry.path();
            if !child.is_dir() {
                continue;
            }

            for file_name in [&emlx_name, &partial_name] {
                let child_messages = child.join("Messages").join(file_name);
                if child_messages.exists() {
                    return Some(child_messages);
                }
            }

            let data_root = child.join("Data");
            if !data_root.is_dir() {
                continue;
            }

            for level_one in fs::read_dir(&data_root).ok()?.filter_map(Result::ok) {
                let level_one_path = level_one.path();
                if !level_one_path.is_dir() {
                    continue;
                }

                for level_two in fs::read_dir(&level_one_path).ok()?.filter_map(Result::ok) {
                    let level_two_path = level_two.path();
                    if !level_two_path.is_dir() {
                        continue;
                    }

                    for file_name in [&emlx_name, &partial_name] {
                        let hashed_messages = level_two_path.join("Messages").join(file_name);
                        if hashed_messages.exists() {
                            return Some(hashed_messages);
                        }
                    }
                }
            }
        }
    }

    None
}

fn lookup_mailbox_index(
    mailbox_dir: &Path,
    candidate_ids: &[String],
    message_id_header: Option<&str>,
) -> Option<PathBuf> {
    if let Ok(cache) = MAILBOX_INDEX_CACHE.lock()
        && let Some(index) = cache.get(mailbox_dir)
    {
        if let Some(header) = message_id_header
            && let Some(path) = index.by_header.get(header)
            && path.exists()
        {
            return Some(path.clone());
        }

        for candidate_id in candidate_ids {
            if let Some(path) = index.by_stem.get(candidate_id)
                && path.exists()
            {
                return Some(path.clone());
            }
        }
    }

    let index = build_mailbox_index(mailbox_dir)?;
    let matched = if let Some(header) = message_id_header {
        index.by_header.get(header).cloned()
    } else {
        None
    }
    .or_else(|| {
        candidate_ids
            .iter()
            .find_map(|candidate_id| index.by_stem.get(candidate_id).cloned())
    });

    if let Ok(mut cache) = MAILBOX_INDEX_CACHE.lock() {
        cache.insert(mailbox_dir.to_path_buf(), index);
    }

    matched
}

fn lookup_mailbox_header(mailbox_dir: &Path, message_id_header: &str) -> Option<PathBuf> {
    if let Ok(cache) = MAILBOX_INDEX_CACHE.lock()
        && let Some(index) = cache.get(mailbox_dir)
        && let Some(path) = index.by_header.get(message_id_header)
        && path.exists()
    {
        return Some(path.clone());
    }

    let index = build_mailbox_index(mailbox_dir)?;
    let matched = index.by_header.get(message_id_header).cloned();

    if let Ok(mut cache) = MAILBOX_INDEX_CACHE.lock() {
        cache.insert(mailbox_dir.to_path_buf(), index);
    }

    matched
}

fn build_mailbox_index(mailbox_dir: &Path) -> Option<MailboxIndex> {
    if !mailbox_dir.exists() {
        return None;
    }

    let mut index = MailboxIndex::default();
    for entry in WalkDir::new(mailbox_dir)
        .max_depth(8)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let Some(file_name) = entry.file_name().to_str() else {
            continue;
        };
        if !(file_name.ends_with(".emlx") || file_name.ends_with(".partial.emlx")) {
            continue;
        }

        let path = entry.path().to_path_buf();
        let stem = file_name
            .trim_end_matches(".partial.emlx")
            .trim_end_matches(".emlx");
        index
            .by_stem
            .entry(stem.to_string())
            .or_insert(path.clone());

        if let Some(header) = extract_message_id_header(&path) {
            index.by_header.entry(header).or_insert(path);
        }
    }

    Some(index)
}

fn extract_message_id_header(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let _byte_count = lines.next()?.ok()?;

    let mut current_name = String::new();
    let mut current_value = String::new();
    for line in lines.take(200) {
        let line = line.ok()?;
        if line.trim().is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            current_value.push_str(line.trim());
            continue;
        }

        if current_name.eq_ignore_ascii_case("Message-ID") {
            return Some(current_value.trim().to_string());
        }

        if let Some((name, value)) = line.split_once(':') {
            current_name.clear();
            current_name.push_str(name.trim());
            current_value.clear();
            current_value.push_str(value.trim());
        }
    }

    if current_name.eq_ignore_ascii_case("Message-ID") {
        return Some(current_value.trim().to_string());
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

    #[test]
    fn locate_emlx_quick_uses_direct_path_without_recursive_scan() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let base_path = mail_dir.join("V10");
        let uuid_dir = base_path.join("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA");
        let messages_dir = uuid_dir.join("Inbox.mbox").join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        let emlx_path = messages_dir.join("7.emlx");
        File::create(&emlx_path).unwrap();

        let result = locate_emlx_quick(mail_dir, "V10", "ews://account/Inbox", 7);
        assert_eq!(result, Some(emlx_path));
    }

    #[test]
    fn locate_emlx_prefers_account_specific_directory_hint() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let base_path = mail_dir.join("V10");

        let wrong_messages = base_path
            .join("other-account")
            .join("Inbox.mbox")
            .join("Messages");
        let right_messages = base_path
            .join("account-b")
            .join("Inbox.mbox")
            .join("Messages");
        fs::create_dir_all(&wrong_messages).unwrap();
        fs::create_dir_all(&right_messages).unwrap();
        let wrong_file = wrong_messages.join("9.emlx");
        let right_file = right_messages.join("9.emlx");
        File::create(&wrong_file).unwrap();
        File::create(&right_file).unwrap();

        let result = locate_emlx(mail_dir, "V10", "ews://account-b/Inbox", 9);
        assert_eq!(result, Some(right_file));
    }

    #[test]
    fn locate_emlx_finds_message_in_nested_mailbox_path() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let messages_dir = mail_dir
            .join("V10")
            .join("account-b")
            .join("Inbox.mbox")
            .join("Internal services.mbox")
            .join("Confluence.mbox")
            .join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        let emlx_path = messages_dir.join("194184.emlx");
        File::create(&emlx_path).unwrap();

        let result = locate_emlx(
            mail_dir,
            "V10",
            "ews://account-b/Inbox/Internal%20services/Confluence",
            194184,
        );

        assert_eq!(result, Some(emlx_path));
    }

    #[test]
    fn locate_emlx_finds_message_in_uuid_data_subtree_for_nested_mailbox() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let messages_dir = mail_dir
            .join("V10")
            .join("account-b")
            .join("Inbox.mbox")
            .join("Internal services.mbox")
            .join("Confluence.mbox")
            .join("UUID-1234")
            .join("Data")
            .join("4")
            .join("8")
            .join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        let emlx_path = messages_dir.join("194184.emlx");
        File::create(&emlx_path).unwrap();

        let result = locate_emlx(
            mail_dir,
            "V10",
            "ews://account-b/Inbox/Internal%20services/Confluence",
            194184,
        );

        assert_eq!(result, Some(emlx_path));
    }

    #[test]
    fn locate_emlx_with_hints_matches_by_message_id_header_when_filename_differs() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let messages_dir = mail_dir
            .join("V10")
            .join("account-b")
            .join("Inbox.mbox")
            .join("Internal services.mbox")
            .join("Confluence.mbox")
            .join("UUID-1234")
            .join("Data")
            .join("4")
            .join("8")
            .join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        let emlx_path = messages_dir.join("79665.emlx");
        fs::write(
            &emlx_path,
            concat!(
                "121\n",
                "Message-ID: <confluence@example.com>\n",
                "Subject: Nested\n",
                "\n",
                "Body\n"
            ),
        )
        .unwrap();

        let result = locate_emlx_with_hints(
            mail_dir,
            "V10",
            "ews://account-b/Inbox/Internal%20services/Confluence",
            194184,
            &["194184".to_string(), "99974".to_string()],
            Some("<confluence@example.com>"),
        );

        assert_eq!(result, Some(emlx_path));
    }

    #[test]
    fn locate_emlx_with_hints_prefers_message_id_header_over_wrong_numeric_hint() {
        let temp_dir = TempDir::new().unwrap();
        let mail_dir = temp_dir.path();
        let messages_dir = mail_dir
            .join("V10")
            .join("account-b")
            .join("Inbox.mbox")
            .join("Messages");
        fs::create_dir_all(&messages_dir).unwrap();

        let wrong_numeric_file = messages_dir.join("99974.emlx");
        fs::write(
            &wrong_numeric_file,
            concat!(
                "105\n",
                "Message-ID: <wrong@example.com>\n",
                "Subject: Wrong\n",
                "\n",
                "Wrong body\n"
            ),
        )
        .unwrap();

        let correct_header_file = messages_dir.join("79665.emlx");
        fs::write(
            &correct_header_file,
            concat!(
                "123\n",
                "Message-ID: <right@example.com>\n",
                "Subject: Correct\n",
                "\n",
                "Correct body\n"
            ),
        )
        .unwrap();

        let result = locate_emlx_with_hints(
            mail_dir,
            "V10",
            "ews://account-b/Inbox",
            194184,
            &["194184".to_string(), "99974".to_string()],
            Some("<right@example.com>"),
        );

        assert_eq!(result, Some(correct_header_file));
    }
}
