//! Cache traits for mail lookup caching.
//!
//! This module defines the trait-based interface for all cache types used
//! in the mail location and parsing system.

use std::path::{Path, PathBuf};

use crate::mail::CacheKey;

/// Generic cache trait for key-value storage.
///
/// Implementations should be thread-safe and handle their own synchronization.
pub trait Cache<K, V>: Send + Sync {
    /// Get a value from cache by key.
    ///
    /// Returns `None` if the key is not present.
    fn get(&self, key: &K) -> Option<V>;

    /// Insert a key-value pair into the cache.
    ///
    /// If the key already exists, the value is replaced.
    fn insert(&self, key: K, value: V);

    /// Check if a key exists in the cache.
    fn contains(&self, key: &K) -> bool;

    /// Remove a key-value pair from the cache.
    fn remove(&self, key: &K) -> Option<V>;

    /// Clear all entries from the cache.
    fn clear(&self);

    /// Get the number of entries in the cache.
    fn len(&self) -> usize;

    /// Check if the cache is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Specialized trait for path caching with filesystem validation.
///
/// Extends the basic Cache trait with path-specific operations.
pub trait PathCache: Cache<CacheKey, PathBuf> {
    /// Get a path from cache if it exists on the filesystem.
    ///
    /// This is useful for stale cache entry detection.
    fn get_valid(&self, key: &CacheKey) -> Option<PathBuf>;

    /// Check if a cached path exists on the filesystem.
    fn exists(&self, key: &CacheKey) -> bool {
        self.get_valid(key).is_some()
    }
}

/// Specialized trait for header caching.
///
/// Caches Message-ID header lookups for emlx files.
pub trait HeaderCache: Cache<PathBuf, Option<String>> {
    /// Get a header for a path, returning None if the path has no header.
    ///
    /// Returns `Some(None)` if the path was cached as headerless.
    /// Returns `None` if the path is not in the cache.
    fn get_header(&self, path: &PathBuf) -> Option<Option<String>> {
        self.get(path)
    }

    /// Cache a path as headerless (no Message-ID header).
    fn cache_headerless(&self, path: PathBuf) {
        self.insert(path, None);
    }

    /// Cache a path with its Message-ID header.
    fn cache_with_header(&self, path: PathBuf, header: String) {
        self.insert(path, Some(header));
    }
}

/// Mailbox index for efficient message lookups.
#[derive(Debug, Clone, Default)]
pub struct MailboxIndex {
    /// Resolved Message-ID header to file path map.
    pub by_header: std::collections::HashMap<String, PathBuf>,
    /// Numeric message stem to file path map.
    pub by_stem: std::collections::HashMap<String, PathBuf>,
    /// Candidate .emlx files whose headers can be loaded lazily.
    pub header_candidates: Vec<PathBuf>,
    /// Indicates whether by_header has already been hydrated.
    pub headers_loaded: bool,
}

impl MailboxIndex {
    /// Create a new empty mailbox index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup a path by Message-ID header.
    pub fn lookup_by_header(&self, header: &str) -> Option<&PathBuf> {
        self.by_header.get(header)
    }

    /// Lookup a path by numeric stem.
    pub fn lookup_by_stem(&self, stem: &str) -> Option<&PathBuf> {
        self.by_stem.get(stem)
    }

    /// Insert a mapping from header to path.
    pub fn insert_by_header(&mut self, header: String, path: PathBuf) {
        self.by_header.insert(header, path);
    }

    /// Insert a mapping from stem to path.
    pub fn insert_by_stem(&mut self, stem: String, path: PathBuf) {
        self.by_stem.insert(stem, path);
    }

    /// Get the number of entries in the index.
    pub fn len(&self) -> usize {
        self.by_header.len() + self.by_stem.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Guard that allows mutating a cached mailbox index and writes it back on drop.
pub struct MailboxIndexGuard<'a> {
    key: PathBuf,
    index: MailboxIndex,
    cache: &'a mut dyn MailboxIndexCache,
}

impl<'a> MailboxIndexGuard<'a> {
    /// Create a new guard for the given key and cache.
    pub fn new(key: PathBuf, index: MailboxIndex, cache: &'a mut dyn MailboxIndexCache) -> Self {
        Self { key, index, cache }
    }
}

impl<'a> std::ops::Deref for MailboxIndexGuard<'a> {
    type Target = MailboxIndex;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

impl<'a> std::ops::DerefMut for MailboxIndexGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.index
    }
}

impl<'a> Drop for MailboxIndexGuard<'a> {
    fn drop(&mut self) {
        self.cache.insert(self.key.clone(), self.index.clone());
    }
}

/// Specialized trait for mailbox index caching.
///
/// Provides access to mailbox indexes with mutable operations.
pub trait MailboxIndexCache: Cache<PathBuf, MailboxIndex> {
    /// Get a mutable guard for a mailbox index.
    ///
    /// The guard will automatically write the index back to the cache when dropped.
    fn get_mut(&mut self, key: &Path) -> Option<MailboxIndexGuard<'_>>;

    /// Lookup a path by Message-ID header in the cached index.
    ///
    /// Returns the path if found and it exists on the filesystem.
    fn lookup_by_header(&self, mailbox_path: &Path, header: &str) -> Option<PathBuf>;

    /// Lookup a path by numeric stem in the cached index.
    ///
    /// Returns the path if found and it exists on the filesystem.
    fn lookup_by_stem(&self, mailbox_path: &Path, stem: &str) -> Option<PathBuf>;

    /// Check if a mailbox index is cached.
    fn contains_index(&self, path: &PathBuf) -> bool {
        self.contains(path)
    }

    /// Get a raw clone of a mailbox index.
    fn get_raw(&self, path: &PathBuf) -> Option<MailboxIndex> {
        self.get(path)
    }
}
