//! Mail caching module.
//!
//! This module provides a deep, trait-based caching system for mail lookup operations.
//! It replaces the previous shallow cache implementation with a more maintainable and
//! testable architecture.
//!
//! # Architecture
//!
//! The caching system is organized around several key traits:
//!
//! - [`Cache`] - Generic key-value cache interface
//! - [`PathCache`] - Specialized cache for message paths with filesystem validation
//! - [`HeaderCache`] - Specialized cache for Message-ID headers
//! - [`MailboxIndexCache`] - Specialized cache for mailbox indexes
//!
//! # Usage
//!
//! For most use cases, use the [`CacheRegistry`] which provides a unified interface:
//!
//! ```rust
//! use crate::mail::cache::{CacheRegistry, CacheKey};
//!
//! let registry = CacheRegistry::new();
//!
//! // Store a path
//! let key = CacheKey::new("/Library/Mail/V10", 12345);
//! registry.insert_path(key, PathBuf::from("/path/to/message.emlx"));
//!
//! // Retrieve a path
//! if let Some(path) = registry.get_valid_path(&key) {
//!     // Use the path
//! }
//! ```

// Declare submodules
pub mod header;
pub mod mailbox_index;
pub mod path;
pub mod registry;
pub mod traits;

// Re-export main types from submodules
pub use header::HeaderCacheImpl;
pub use mailbox_index::MailboxIndexCacheImpl;
pub use path::PathCacheImpl;
pub use registry::{
    CacheRegistry, clear_all_caches, global_registry, global_registry_read, global_registry_write,
};
pub use traits::{
    Cache, HeaderCache, MailboxIndex, MailboxIndexCache, MailboxIndexGuard, PathCache,
};

// CacheKey definition
use std::path::PathBuf;

/// Cache key for resolved message paths.
///
/// Combines the mail root directory and message row ID to uniquely identify
/// a message across different mail versions and accounts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Fully qualified mail root used to namespace the cache.
    pub mail_root: PathBuf,
    /// Message row ID from the Envelope Index database.
    pub message_rowid: i64,
}

impl CacheKey {
    /// Create a new cache key from a mail root and message row ID.
    #[must_use]
    pub fn new(mail_root: impl Into<PathBuf>, message_rowid: i64) -> Self {
        Self {
            mail_root: mail_root.into(),
            message_rowid,
        }
    }
}

// Backward compatibility layer
// ============================
// These functions and statics maintain backward compatibility with the old
// cache implementation during migration. They will be removed once all
// code has been migrated to the new CacheRegistry-based system.

use std::sync::{Mutex, OnceLock};

/// Legacy global path cache - DEPRECATED, use CacheRegistry instead
static LEGACY_PATH_CACHE: OnceLock<Mutex<std::collections::HashMap<CacheKey, PathBuf>>> =
    OnceLock::new();

/// Legacy global header cache - DEPRECATED, use CacheRegistry instead
static LEGACY_HEADER_CACHE: OnceLock<Mutex<std::collections::HashMap<PathBuf, Option<String>>>> =
    OnceLock::new();

/// Legacy global mailbox index cache - DEPRECATED, use CacheRegistry instead
static LEGACY_MAILBOX_INDEX_CACHE: OnceLock<
    Mutex<std::collections::HashMap<PathBuf, traits::MailboxIndex>>,
> = OnceLock::new();

/// Get a value from the legacy path cache.
///
/// # Deprecated
/// Use [`CacheRegistry::get_path`] or [`CacheRegistry::get_valid_path`] instead.
#[deprecated(since = "1.8.0", note = "Use CacheRegistry::get_valid_path instead")]
pub fn path_cache_get(key: &CacheKey) -> Option<PathBuf> {
    let cache = LEGACY_PATH_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let guard = cache.lock().ok()?;
    let cached = guard.get(key)?.clone();
    cached.exists().then_some(cached)
}

/// Insert a value into the legacy path cache.
///
/// # Deprecated
/// Use [`CacheRegistry::insert_path`] instead.
#[deprecated(since = "1.8.0", note = "Use CacheRegistry::insert_path instead")]
pub fn path_cache_insert(key: CacheKey, path: PathBuf) {
    let cache = LEGACY_PATH_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, path);
    }
}

/// Get a value from the legacy header cache.
///
/// # Deprecated
/// Use [`CacheRegistry::get_header`] instead.
#[deprecated(since = "1.8.0", note = "Use CacheRegistry::get_header instead")]
pub fn header_cache_get(path: &PathBuf) -> Option<Option<String>> {
    let cache = LEGACY_HEADER_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let guard = cache.lock().ok()?;
    guard.get(path).cloned()
}

/// Insert a value into the legacy header cache.
///
/// # Deprecated
/// Use [`CacheRegistry::insert_header`] or [`CacheRegistry::cache_with_header`] instead.
#[deprecated(since = "1.8.0", note = "Use CacheRegistry::cache_with_header instead")]
pub fn header_cache_insert(path: PathBuf, header: Option<String>) {
    let cache = LEGACY_HEADER_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(path, header);
    }
}

/// Check if a mailbox index is in the legacy cache.
///
/// # Deprecated
/// Use [`CacheRegistry::contains_mailbox_index`] instead.
#[deprecated(
    since = "1.8.0",
    note = "Use CacheRegistry::contains_mailbox_index instead"
)]
pub fn mailbox_index_cache_contains(path: &PathBuf) -> bool {
    let cache =
        LEGACY_MAILBOX_INDEX_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let guard = cache.lock().ok();
    guard.map(|c| c.contains_key(path)).unwrap_or(false)
}

/// Get a mutable guard for a mailbox index from the legacy cache.
///
/// # Deprecated
/// Use [`CacheRegistry::get_mailbox_index_mut`] instead.
#[deprecated(
    since = "1.8.0",
    note = "Use CacheRegistry::get_mailbox_index_mut instead"
)]
pub fn mailbox_index_cache_get_mut(path: &PathBuf) -> Option<traits::MailboxIndex> {
    let cache =
        LEGACY_MAILBOX_INDEX_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));

    let mut guard = cache.lock().ok()?;
    guard.remove(path)
}

/// Insert a mailbox index into the legacy cache.
///
/// # Deprecated
/// Use [`CacheRegistry::insert_mailbox_index`] instead.
#[deprecated(
    since = "1.8.0",
    note = "Use CacheRegistry::insert_mailbox_index instead"
)]
pub fn mailbox_index_cache_insert(path: PathBuf, index: traits::MailboxIndex) {
    let cache =
        LEGACY_MAILBOX_INDEX_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(path, index);
    }
}

/// Get a raw mailbox index from the legacy cache.
///
/// # Deprecated
/// Use [`CacheRegistry::get_mailbox_index`] instead.
#[deprecated(since = "1.8.0", note = "Use CacheRegistry::get_mailbox_index instead")]
pub fn mailbox_index_cache_get_raw(path: &PathBuf) -> Option<traits::MailboxIndex> {
    let cache =
        LEGACY_MAILBOX_INDEX_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let guard = cache.lock().ok()?;
    guard.get(path).cloned()
}

/// Lookup a path by header in the legacy mailbox index cache.
///
/// # Deprecated
/// Use [`CacheRegistry::lookup_mailbox_by_header`] instead.
#[deprecated(
    since = "1.8.0",
    note = "Use CacheRegistry::lookup_mailbox_by_header instead"
)]
pub fn mailbox_index_lookup_by_header(path: &PathBuf, header: &str) -> Option<PathBuf> {
    let cache =
        LEGACY_MAILBOX_INDEX_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let guard = cache.lock().ok()?;
    let index = guard.get(path)?;
    let candidate = index.by_header.get(header)?;
    candidate.exists().then_some(candidate.clone())
}

/// Lookup a path by stem in the legacy mailbox index cache.
///
/// # Deprecated
/// Use [`CacheRegistry::lookup_mailbox_by_stem`] instead.
#[deprecated(
    since = "1.8.0",
    note = "Use CacheRegistry::lookup_mailbox_by_stem instead"
)]
pub fn mailbox_index_lookup_by_stem(path: &PathBuf, stem: &str) -> Option<PathBuf> {
    let cache =
        LEGACY_MAILBOX_INDEX_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let guard = cache.lock().ok()?;
    let index = guard.get(path)?;
    let candidate = index.by_stem.get(stem)?;
    candidate.exists().then_some(candidate.clone())
}

/// Insert a mailbox index into the legacy cache without extra processing.
///
/// # Deprecated
/// Use [`CacheRegistry::insert_mailbox_index`] instead.
#[deprecated(
    since = "1.8.0",
    note = "Use CacheRegistry::insert_mailbox_index instead"
)]
pub fn mailbox_index_cache_insert_raw(path: PathBuf, index: traits::MailboxIndex) {
    #[allow(deprecated)]
    mailbox_index_cache_insert(path, index);
}

/// Clear all legacy caches.
///
/// # Deprecated
/// Use [`clear_all_caches`] instead.
#[deprecated(since = "1.8.0", note = "Use clear_all_caches instead")]
pub fn clear_all_caches_legacy() {
    clear_all_caches();
}
