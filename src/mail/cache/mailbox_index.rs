//! Mailbox index cache implementation.
//!
//! This module provides a thread-safe cache for mailbox indexes that map
//! Message-ID headers and numeric stems to .emlx file paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::mail::cache::traits::{Cache, MailboxIndex, MailboxIndexCache, MailboxIndexGuard};

/// Thread-safe implementation of a mailbox index cache.
///
/// Stores mailbox indexes keyed by mailbox directory path.
#[derive(Debug, Default)]
pub struct MailboxIndexCacheImpl {
    inner: Arc<RwLock<HashMap<PathBuf, MailboxIndex>>>,
}

impl MailboxIndexCacheImpl {
    /// Create a new empty mailbox index cache.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new mailbox index cache with a given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::with_capacity(capacity))),
        }
    }
}

impl Clone for MailboxIndexCacheImpl {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Cache<PathBuf, MailboxIndex> for MailboxIndexCacheImpl {
    fn get(&self, key: &PathBuf) -> Option<MailboxIndex> {
        if let Ok(cache) = self.inner.read() {
            cache.get(key).cloned()
        } else {
            None
        }
    }

    fn insert(&self, key: PathBuf, value: MailboxIndex) {
        if let Ok(mut cache) = self.inner.write() {
            cache.insert(key, value);
        }
    }

    fn contains(&self, key: &PathBuf) -> bool {
        if let Ok(cache) = self.inner.read() {
            cache.contains_key(key)
        } else {
            false
        }
    }

    fn remove(&self, key: &PathBuf) -> Option<MailboxIndex> {
        if let Ok(mut cache) = self.inner.write() {
            cache.remove(key)
        } else {
            None
        }
    }

    fn clear(&self) {
        if let Ok(mut cache) = self.inner.write() {
            cache.clear();
        }
    }

    fn len(&self) -> usize {
        if let Ok(cache) = self.inner.read() {
            cache.len()
        } else {
            0
        }
    }
}

impl MailboxIndexCache for MailboxIndexCacheImpl {
    fn get_mut(&mut self, key: &Path) -> Option<MailboxIndexGuard<'_>> {
        // Remove the index while holding the write lock, then drop the lock
        // before creating the guard (which needs &mut self).
        let key = key.to_path_buf();
        let index = {
            let mut cache = self.inner.write().ok()?;
            cache.remove(&key)?
        };
        Some(MailboxIndexGuard::new(key, index, self))
    }

    fn lookup_by_header(&self, mailbox_path: &Path, header: &str) -> Option<PathBuf> {
        let cache = self.inner.read().ok()?;
        let index = cache.get(mailbox_path)?;
        let candidate = index.by_header.get(header)?;
        if candidate.exists() {
            Some(candidate.clone())
        } else {
            None
        }
    }

    fn lookup_by_stem(&self, mailbox_path: &Path, stem: &str) -> Option<PathBuf> {
        let cache = self.inner.read().ok()?;
        let index = cache.get(mailbox_path)?;
        let candidate = index.by_stem.get(stem)?;
        if candidate.exists() {
            Some(candidate.clone())
        } else {
            None
        }
    }

    fn get_raw(&self, path: &PathBuf) -> Option<MailboxIndex> {
        self.get(path)
    }
}

impl MailboxIndexCacheImpl {
    /// Get a reference to the underlying cache for testing purposes.
    #[cfg(test)]
    pub fn inner(&self) -> &Arc<RwLock<HashMap<PathBuf, MailboxIndex>>> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn index_with_entry(path: PathBuf, header: &str, stem: &str) -> MailboxIndex {
        let mut index = MailboxIndex::new();
        index.insert_by_header(header.to_string(), path.clone());
        index.insert_by_stem(stem.to_string(), path);
        index
    }

    #[test]
    fn test_mailbox_index_cache_contains() {
        let cache = MailboxIndexCacheImpl::new();
        let key = PathBuf::from("/mail");

        assert!(!cache.contains(&key));
        cache.insert(key.clone(), MailboxIndex::new());
        assert!(cache.contains(&key));
    }

    #[test]
    fn test_mailbox_index_cache_get_raw() {
        let cache = MailboxIndexCacheImpl::new();
        let key = PathBuf::from("/mail");
        let index = index_with_entry(PathBuf::from("/tmp/1.emlx"), "<msg@1>", "1");

        cache.insert(key.clone(), index.clone());
        let retrieved = cache.get_raw(&key).unwrap();
        assert_eq!(retrieved.by_header.len(), 1);
        assert_eq!(retrieved.by_stem.len(), 1);
    }

    #[test]
    fn test_mailbox_index_cache_get_mut_and_drop_writes_back() {
        let mut cache = MailboxIndexCacheImpl::new();
        let key = PathBuf::from("/mail/mut");
        let mut index = MailboxIndex::new();
        index.insert_by_stem("1".to_string(), PathBuf::from("/tmp/1.emlx"));
        cache.insert(key.clone(), index);

        {
            let mut guard = cache.get_mut(&key).unwrap();
            guard.insert_by_stem("2".to_string(), PathBuf::from("/tmp/2.emlx"));
        } // guard dropped here, writes back to cache

        let retrieved = cache.get_raw(&key).unwrap();
        assert!(retrieved.by_stem.contains_key("1"));
        assert!(retrieved.by_stem.contains_key("2"));
    }

    #[test]
    fn test_mailbox_index_cache_lookup_by_header() {
        let cache = MailboxIndexCacheImpl::new();
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("header_test.emlx");
        std::fs::write(&file, b"h").unwrap();

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file.clone(), "<msg@h>", "123");
        cache.insert(key.clone(), index);

        let result = cache.lookup_by_header(&key, "<msg@h>");
        assert_eq!(result, Some(file));
    }

    #[test]
    fn test_mailbox_index_cache_lookup_by_header_returns_none_when_path_missing() {
        let cache = MailboxIndexCacheImpl::new();
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("ghost.emlx");

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file, "<msg@ghost>", "999");
        cache.insert(key.clone(), index);

        let result = cache.lookup_by_header(&key, "<msg@ghost>");
        assert!(result.is_none());
    }

    #[test]
    fn test_mailbox_index_cache_lookup_by_stem() {
        let cache = MailboxIndexCacheImpl::new();
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("456.emlx");
        std::fs::write(&file, b"s").unwrap();

        let key = temp.path().to_path_buf();
        let index = index_with_entry(file.clone(), "<id>", "456");
        cache.insert(key.clone(), index);

        let result = cache.lookup_by_stem(&key, "456");
        assert_eq!(result, Some(file));
    }

    #[test]
    fn test_mailbox_index_cache_clear() {
        let cache = MailboxIndexCacheImpl::new();
        cache.insert(PathBuf::from("/mail/1"), MailboxIndex::new());
        cache.insert(PathBuf::from("/mail/2"), MailboxIndex::new());
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_mailbox_index_cache_clone() {
        let cache = MailboxIndexCacheImpl::new();
        cache.insert(PathBuf::from("/mail"), MailboxIndex::new());

        let cache2 = cache.clone();
        assert_eq!(cache2.len(), 1);
    }

    // Helper for tests
    use tempfile::TempDir;
}
