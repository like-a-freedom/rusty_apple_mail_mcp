//! Cache registry for managing all mail-related caches.
//!
//! This module provides a centralized registry that holds all cache instances
//! and provides a unified interface for cache operations.

use std::path::{Path, PathBuf};
use std::sync::{MutexGuard, RwLock};

use crate::mail::CacheKey;
use crate::mail::cache::{
    header::HeaderCacheImpl,
    mailbox_index::MailboxIndexCacheImpl,
    path::PathCacheImpl,
    traits::{Cache, HeaderCache, MailboxIndexCache, PathCache},
};

/// Central registry holding all mail-related caches.
///
/// This struct provides a unified interface for accessing different cache types
/// and manages their lifetimes together.
#[derive(Debug, Default)]
pub struct CacheRegistry {
    path_cache: RwLock<PathCacheImpl>,
    header_cache: RwLock<HeaderCacheImpl>,
    mailbox_index_cache: RwLock<MailboxIndexCacheImpl>,
}

impl CacheRegistry {
    /// Create a new cache registry with default cache implementations.
    pub fn new() -> Self {
        Self {
            path_cache: RwLock::new(PathCacheImpl::new()),
            header_cache: RwLock::new(HeaderCacheImpl::new()),
            mailbox_index_cache: RwLock::new(MailboxIndexCacheImpl::new()),
        }
    }

    /// Create a new cache registry with caches of specified capacities.
    pub fn with_capacities(
        path_capacity: usize,
        header_capacity: usize,
        mailbox_capacity: usize,
    ) -> Self {
        Self {
            path_cache: RwLock::new(PathCacheImpl::with_capacity(path_capacity)),
            header_cache: RwLock::new(HeaderCacheImpl::with_capacity(header_capacity)),
            mailbox_index_cache: RwLock::new(MailboxIndexCacheImpl::with_capacity(
                mailbox_capacity,
            )),
        }
    }

    /// Get a reference to the path cache.
    pub fn path_cache(&self) -> &RwLock<PathCacheImpl> {
        &self.path_cache
    }

    /// Get a reference to the header cache.
    pub fn header_cache(&self) -> &RwLock<HeaderCacheImpl> {
        &self.header_cache
    }

    /// Get a reference to the mailbox index cache.
    pub fn mailbox_index_cache(&self) -> &RwLock<MailboxIndexCacheImpl> {
        &self.mailbox_index_cache
    }

    /// Clear all caches.
    pub fn clear_all(&self) {
        self.path_cache.write().unwrap().clear();
        self.header_cache.write().unwrap().clear();
        self.mailbox_index_cache.write().unwrap().clear();
    }

    /// Get the total number of entries across all caches.
    pub fn total_len(&self) -> usize {
        self.path_cache.read().unwrap().len()
            + self.header_cache.read().unwrap().len()
            + self.mailbox_index_cache.read().unwrap().len()
    }
}

impl Clone for CacheRegistry {
    fn clone(&self) -> Self {
        Self {
            path_cache: RwLock::new(self.path_cache.read().unwrap().clone()),
            header_cache: RwLock::new(self.header_cache.read().unwrap().clone()),
            mailbox_index_cache: RwLock::new(self.mailbox_index_cache.read().unwrap().clone()),
        }
    }
}

/// Convenience methods for accessing caches through the registry.
impl CacheRegistry {
    // Path cache convenience methods

    /// Get a path from the path cache.
    pub fn get_path(&self, key: &CacheKey) -> Option<PathBuf> {
        self.path_cache.read().unwrap().get(key)
    }

    /// Get a valid path from the path cache (checks filesystem existence).
    pub fn get_valid_path(&self, key: &CacheKey) -> Option<PathBuf> {
        self.path_cache.read().unwrap().get_valid(key)
    }

    /// Insert a path into the path cache.
    pub fn insert_path(&self, key: CacheKey, path: PathBuf) {
        self.path_cache.write().unwrap().insert(key, path);
    }

    /// Check if a path is cached.
    pub fn contains_path(&self, key: &CacheKey) -> bool {
        self.path_cache.read().unwrap().contains(key)
    }

    // Header cache convenience methods

    /// Get a header from the header cache.
    pub fn get_header(&self, path: &PathBuf) -> Option<Option<String>> {
        self.header_cache.read().unwrap().get(path)
    }

    /// Insert a header into the header cache.
    pub fn insert_header(&self, path: PathBuf, header: Option<String>) {
        self.header_cache.write().unwrap().insert(path, header);
    }

    /// Cache a path as headerless.
    pub fn cache_headerless(&self, path: PathBuf) {
        self.header_cache.write().unwrap().cache_headerless(path);
    }

    /// Cache a path with its header.
    pub fn cache_with_header(&self, path: PathBuf, header: String) {
        self.header_cache
            .write()
            .unwrap()
            .cache_with_header(path, header);
    }

    /// Check if a header is cached.
    pub fn contains_header(&self, path: &PathBuf) -> bool {
        self.header_cache.read().unwrap().contains(path)
    }

    // Mailbox index cache convenience methods

    /// Check if a mailbox index is cached.
    pub fn contains_mailbox_index(&self, path: &PathBuf) -> bool {
        self.mailbox_index_cache
            .read()
            .unwrap()
            .contains_index(path)
    }

    /// Get a raw mailbox index.
    pub fn get_mailbox_index(
        &self,
        path: &PathBuf,
    ) -> Option<crate::mail::cache::traits::MailboxIndex> {
        self.mailbox_index_cache.read().unwrap().get_raw(path)
    }

    /// Insert a mailbox index.
    pub fn insert_mailbox_index(
        &self,
        path: PathBuf,
        index: crate::mail::cache::traits::MailboxIndex,
    ) {
        self.mailbox_index_cache
            .write()
            .unwrap()
            .insert(path, index);
    }

    /// Lookup a path by header in a mailbox index.
    pub fn lookup_mailbox_by_header(&self, mailbox_path: &Path, header: &str) -> Option<PathBuf> {
        self.mailbox_index_cache
            .read()
            .unwrap()
            .lookup_by_header(mailbox_path, header)
    }

    /// Lookup a path by stem in a mailbox index.
    pub fn lookup_mailbox_by_stem(&self, mailbox_path: &Path, stem: &str) -> Option<PathBuf> {
        self.mailbox_index_cache
            .read()
            .unwrap()
            .lookup_by_stem(mailbox_path, stem)
    }
}

// Global registry for backward compatibility
use once_cell::sync::OnceCell;
use std::sync::Mutex;

/// Global cache registry instance.
///
/// This provides a singleton cache registry for use throughout the application.
/// In the future, this should be replaced with dependency injection.
static GLOBAL_REGISTRY: OnceCell<Mutex<CacheRegistry>> = OnceCell::new();

/// Get or initialize the global cache registry.
pub fn global_registry() -> &'static Mutex<CacheRegistry> {
    GLOBAL_REGISTRY.get_or_init(|| Mutex::new(CacheRegistry::new()))
}

/// Get a lock on the global registry for reading.
pub fn global_registry_read() -> MutexGuard<'static, CacheRegistry> {
    global_registry().lock().unwrap()
}

/// Get a lock on the global registry for writing.
pub fn global_registry_write() -> MutexGuard<'static, CacheRegistry> {
    global_registry().lock().unwrap()
}

/// Clear all global caches.
pub fn clear_all_caches() {
    global_registry_write().clear_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_registry_new() {
        let registry = CacheRegistry::new();
        assert_eq!(registry.total_len(), 0);
    }

    #[test]
    fn test_registry_path_cache_operations() {
        let registry = CacheRegistry::new();
        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 42,
        };
        let path = PathBuf::from("/tmp/test.emlx");

        assert!(!registry.contains_path(&key));
        registry.insert_path(key.clone(), path.clone());
        assert!(registry.contains_path(&key));
        assert_eq!(registry.get_path(&key), Some(path));
    }

    #[test]
    fn test_registry_header_cache_operations() {
        let registry = CacheRegistry::new();
        let path = PathBuf::from("/tmp/test.emlx");
        let header = Some("<msg@id>".to_string());

        assert!(!registry.contains_header(&path));
        registry.insert_header(path.clone(), header.clone());
        assert!(registry.contains_header(&path));
        assert_eq!(registry.get_header(&path), Some(header));
    }

    #[test]
    fn test_registry_mailbox_index_cache_operations() {
        let registry = CacheRegistry::new();
        let mailbox_path = PathBuf::from("/mail/inbox");
        let mut index = crate::mail::cache::traits::MailboxIndex::new();
        index.insert_by_header("<msg@1>".to_string(), PathBuf::from("/mail/inbox/1.emlx"));

        assert!(!registry.contains_mailbox_index(&mailbox_path));
        registry.insert_mailbox_index(mailbox_path.clone(), index.clone());
        assert!(registry.contains_mailbox_index(&mailbox_path));

        let retrieved = registry.get_mailbox_index(&mailbox_path).unwrap();
        assert_eq!(retrieved.by_header.len(), 1);
    }

    #[test]
    fn test_registry_clear_all() {
        let registry = CacheRegistry::new();

        // Add entries to all caches
        registry.insert_path(
            CacheKey {
                mail_root: PathBuf::from("/"),
                message_rowid: 1,
            },
            PathBuf::from("/tmp/1.emlx"),
        );
        registry.insert_header(PathBuf::from("/tmp/1.emlx"), Some("h1".to_string()));
        registry.insert_mailbox_index(
            PathBuf::from("/mail"),
            crate::mail::cache::traits::MailboxIndex::new(),
        );

        assert!(registry.total_len() > 0);
        registry.clear_all();
        assert_eq!(registry.total_len(), 0);
    }

    #[test]
    fn test_registry_clone() {
        let registry = CacheRegistry::new();
        registry.insert_path(
            CacheKey {
                mail_root: PathBuf::from("/"),
                message_rowid: 1,
            },
            PathBuf::from("/tmp/1.emlx"),
        );

        let registry2 = registry.clone();
        assert_eq!(registry2.total_len(), 1);
    }

    #[test]
    fn test_global_registry() {
        let registry = global_registry();
        let guard = registry.lock().unwrap();
        assert_eq!(guard.total_len(), 0);
    }
}
