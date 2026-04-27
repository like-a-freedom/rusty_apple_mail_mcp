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
