//! Shared in-memory caches for `.emlx` lookup.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

/// Cache key for resolved message paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Fully qualified mail root used to namespace the cache.
    pub mail_root: PathBuf,
    /// Message row ID from the Envelope Index database.
    pub message_rowid: i64,
}

/// Mailbox-local index used to speed up `Message-ID` and filename lookups.
#[derive(Debug, Clone, Default)]
pub struct MailboxIndex {
    /// Resolved `Message-ID` header to file path map.
    pub by_header: HashMap<String, PathBuf>,
    /// Numeric message stem to file path map.
    pub by_stem: HashMap<String, PathBuf>,
    /// Candidate `.emlx` files whose headers can be loaded lazily.
    pub header_candidates: Vec<PathBuf>,
    /// Indicates whether `by_header` has already been hydrated.
    pub headers_loaded: bool,
}

static PATH_CACHE: LazyLock<Mutex<HashMap<CacheKey, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static HEADER_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static MAILBOX_INDEX_CACHE: LazyLock<Mutex<HashMap<PathBuf, MailboxIndex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Guard that allows mutating a cached mailbox index and writes it back on drop.
pub struct MailboxIndexGuard {
    key: PathBuf,
    index: MailboxIndex,
}

impl Deref for MailboxIndexGuard {
    type Target = MailboxIndex;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

impl DerefMut for MailboxIndexGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.index
    }
}

impl Drop for MailboxIndexGuard {
    fn drop(&mut self) {
        if let Ok(mut cache) = MAILBOX_INDEX_CACHE.lock() {
            cache.insert(self.key.clone(), self.index.clone());
        }
    }
}

/// Clear all locator caches.
pub fn clear_all_caches() {
    if let Ok(mut cache) = PATH_CACHE.lock() {
        cache.clear();
    }
    if let Ok(mut cache) = HEADER_CACHE.lock() {
        cache.clear();
    }
    if let Ok(mut cache) = MAILBOX_INDEX_CACHE.lock() {
        cache.clear();
    }
}

/// Read a cached resolved message path.
pub fn path_cache_get(key: &CacheKey) -> Option<PathBuf> {
    let cache = PATH_CACHE.lock().ok()?;
    let cached = cache.get(key)?.clone();
    cached.exists().then_some(cached)
}

/// Insert a resolved message path into the cache.
pub fn path_cache_insert(key: CacheKey, path: PathBuf) {
    if let Ok(mut cache) = PATH_CACHE.lock() {
        cache.insert(key, path);
    }
}

/// Read a cached `Message-ID` header lookup result.
///
/// Returns `None` on cache miss and `Some(None)` when the path was cached as headerless.
pub fn header_cache_get(path: &PathBuf) -> Option<Option<String>> {
    let cache = HEADER_CACHE.lock().ok()?;
    cache.get(path).cloned()
}

/// Insert a cached `Message-ID` header lookup result.
pub fn header_cache_insert(path: PathBuf, header: Option<String>) {
    if let Ok(mut cache) = HEADER_CACHE.lock() {
        cache.insert(path, header);
    }
}

/// Returns `true` if a mailbox index is already cached.
pub fn mailbox_index_cache_contains(path: &PathBuf) -> bool {
    MAILBOX_INDEX_CACHE
        .lock()
        .map(|cache| cache.contains_key(path))
        .unwrap_or(false)
}

/// Remove a mailbox index from the cache and return a guard that writes it back on drop.
pub fn mailbox_index_cache_get_mut(path: &PathBuf) -> Option<MailboxIndexGuard> {
    let mut cache = MAILBOX_INDEX_CACHE.lock().ok()?;
    let index = cache.remove(path)?;
    Some(MailboxIndexGuard {
        key: path.clone(),
        index,
    })
}

/// Insert a fully built mailbox index into the cache.
pub fn mailbox_index_cache_insert(path: PathBuf, index: MailboxIndex) {
    if let Ok(mut cache) = MAILBOX_INDEX_CACHE.lock() {
        cache.insert(path, index);
    }
}

/// Insert a mailbox index into the cache without extra processing.
pub fn mailbox_index_cache_insert_raw(path: PathBuf, index: MailboxIndex) {
    mailbox_index_cache_insert(path, index);
}

/// Read a cloned mailbox index from the cache.
pub fn mailbox_index_cache_get_raw(path: &PathBuf) -> Option<MailboxIndex> {
    let cache = MAILBOX_INDEX_CACHE.lock().ok()?;
    cache.get(path).cloned()
}

/// Lookup a cached mailbox path by `Message-ID` header.
pub fn mailbox_index_lookup_by_header(path: &PathBuf, header: &str) -> Option<PathBuf> {
    let cache = MAILBOX_INDEX_CACHE.lock().ok()?;
    let candidate = cache.get(path)?.by_header.get(header)?.clone();
    candidate.exists().then_some(candidate)
}

/// Lookup a cached mailbox path by numeric filename stem.
pub fn mailbox_index_lookup_by_stem(path: &PathBuf, stem: &str) -> Option<PathBuf> {
    let cache = MAILBOX_INDEX_CACHE.lock().ok()?;
    let candidate = cache.get(path)?.by_stem.get(stem)?.clone();
    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn index_with_entry(path: PathBuf, header: &str, stem: &str) -> MailboxIndex {
        let mut index = MailboxIndex::default();
        index.by_header.insert(header.to_string(), path.clone());
        index.by_stem.insert(stem.to_string(), path);
        index
    }

    #[test]
    fn path_cache_roundtrip() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("123.emlx");
        std::fs::write(&file_path, b"content").unwrap();

        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 42,
        };
        assert!(path_cache_get(&key).is_none());
        path_cache_insert(key.clone(), file_path.clone());
        assert_eq!(path_cache_get(&key), Some(file_path));
    }

    #[test]
    fn path_cache_returns_none_for_stale_entry() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("stale.emlx");
        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 99,
        };
        path_cache_insert(key.clone(), file_path.clone());
        assert!(path_cache_get(&key).is_none());
        std::fs::write(&file_path, b"content").unwrap();
        assert!(path_cache_get(&key).is_some());
    }

    #[test]
    fn header_cache_roundtrip() {
        let path = PathBuf::from("/tmp/test.emlx");
        assert!(header_cache_get(&path).is_none());
        header_cache_insert(path.clone(), Some("<msg@id>".to_string()));
        assert_eq!(header_cache_get(&path), Some(Some("<msg@id>".to_string())));
    }

    #[test]
    fn header_cache_stores_none_for_headerless() {
        let path = PathBuf::from("/tmp/noheader.emlx");
        header_cache_insert(path.clone(), None);
        assert_eq!(header_cache_get(&path), Some(None));
    }

    #[test]
    fn mailbox_index_cache_contains_true_after_insert() {
        let key = PathBuf::from("/mail");
        assert!(!mailbox_index_cache_contains(&key));
        mailbox_index_cache_insert(key.clone(), MailboxIndex::default());
        assert!(mailbox_index_cache_contains(&key));
    }

    #[test]
    fn mailbox_index_cache_get_mut_and_drop_writes_back() {
        let key = PathBuf::from("/mail/mut");
        let mut index = MailboxIndex::default();
        index.by_stem.insert("1".to_string(), PathBuf::from("/tmp/1.emlx"));
        mailbox_index_cache_insert(key.clone(), index);

        let mut guard = mailbox_index_cache_get_mut(&key).unwrap();
        guard.by_stem.insert("2".to_string(), PathBuf::from("/tmp/2.emlx"));
        drop(guard);

        let retrieved = mailbox_index_cache_get_raw(&key).unwrap();
        assert!(retrieved.by_stem.contains_key("1"));
        assert!(retrieved.by_stem.contains_key("2"));
    }

    #[test]
    fn mailbox_index_cache_get_mut_returns_none_for_missing() {
        assert!(mailbox_index_cache_get_mut(&PathBuf::from("/nonexistent")).is_none());
    }

    #[test]
    fn mailbox_index_cache_insert_raw_delegates() {
        let key = PathBuf::from("/raw");
        mailbox_index_cache_insert_raw(key.clone(), MailboxIndex::default());
        assert!(mailbox_index_cache_contains(&key));
    }

    #[test]
    fn mailbox_index_cache_get_raw_roundtrip() {
        let key = PathBuf::from("/getraw");
        let mut index = MailboxIndex::default();
        index.by_header.insert("<h>".to_string(), PathBuf::from("/h.emlx"));
        mailbox_index_cache_insert(key.clone(), index.clone());
        assert_eq!(mailbox_index_cache_get_raw(&key).unwrap().by_header.get("<h>"), Some(&PathBuf::from("/h.emlx")));
    }

    #[test]
    fn mailbox_index_lookup_by_header_found_when_path_exists() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("header_test.emlx");
        std::fs::write(&file, b"h").unwrap();

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file.clone(), "<msg@h>", "123");
        mailbox_index_cache_insert(key.clone(), index);

        let result = mailbox_index_lookup_by_header(&key, "<msg@h>");
        assert_eq!(result, Some(file));
    }

    #[test]
    fn mailbox_index_lookup_by_header_returns_none_when_path_missing() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("ghost.emlx");

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file, "<msg@ghost>", "999");
        mailbox_index_cache_insert(key.clone(), index);

        let result = mailbox_index_lookup_by_header(&key, "<msg@ghost>");
        assert!(result.is_none());
    }

    #[test]
    fn mailbox_index_lookup_by_header_returns_none_for_missing_key() {
        let result = mailbox_index_lookup_by_header(&PathBuf::from("/missing"), "<any>");
        assert!(result.is_none());
    }

    #[test]
    fn mailbox_index_lookup_by_stem_found_when_path_exists() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("456.emlx");
        std::fs::write(&file, b"s").unwrap();

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file.clone(), "<id>", "456");
        mailbox_index_cache_insert(key.clone(), index);

        let result = mailbox_index_lookup_by_stem(&key, "456");
        assert_eq!(result, Some(file));
    }

    #[test]
    fn mailbox_index_lookup_by_stem_returns_none_when_path_missing() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("ghost_stem.emlx");

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file, "<stem_ghost>", "789");
        mailbox_index_cache_insert(key.clone(), index);

        let result = mailbox_index_lookup_by_stem(&key, "789");
        assert!(result.is_none());
    }

    #[test]
    fn mailbox_index_lookup_by_stem_returns_none_for_missing_key() {
        let result = mailbox_index_lookup_by_stem(&PathBuf::from("/missing"), "789");
        assert!(result.is_none());
    }

    #[test]
    fn clear_all_caches_clears_everything() {
        let key = PathBuf::from("/clear");
        mailbox_index_cache_insert(key.clone(), MailboxIndex::default());
        assert!(mailbox_index_cache_contains(&key));

        let cache_key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 1,
        };
        path_cache_insert(cache_key.clone(), PathBuf::from("/tmp/f.emlx"));

        clear_all_caches();
        assert!(!mailbox_index_cache_contains(&key));
    }
}
