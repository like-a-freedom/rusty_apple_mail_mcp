//! Header cache implementation for Message-ID header lookups.
//!
//! This module provides a thread-safe cache for storing and retrieving
//! Message-ID headers extracted from .emlx files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::mail::cache::traits::{Cache, HeaderCache};

/// Thread-safe implementation of a header cache.
///
/// Maps file paths to their Message-ID headers (or None for headerless files).
#[derive(Debug, Default)]
pub struct HeaderCacheImpl {
    inner: Arc<RwLock<HashMap<PathBuf, Option<String>>>>,
}

impl HeaderCacheImpl {
    /// Create a new empty header cache.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new header cache with a given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::with_capacity(capacity))),
        }
    }
}

impl Clone for HeaderCacheImpl {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Cache<PathBuf, Option<String>> for HeaderCacheImpl {
    fn get(&self, key: &PathBuf) -> Option<Option<String>> {
        let cache = self.inner.read().ok()?;
        cache.get(key).cloned()
    }

    fn insert(&self, key: PathBuf, value: Option<String>) {
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

    fn remove(&self, key: &PathBuf) -> Option<Option<String>> {
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

impl HeaderCache for HeaderCacheImpl {
    fn cache_headerless(&self, path: PathBuf) {
        self.insert(path, None);
    }

    fn cache_with_header(&self, path: PathBuf, header: String) {
        self.insert(path, Some(header));
    }
}

impl HeaderCacheImpl {
    /// Get a reference to the underlying cache for testing purposes.
    #[cfg(test)]
    pub fn inner(&self) -> &Arc<RwLock<HashMap<PathBuf, Option<String>>>> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_header_cache_roundtrip() {
        let cache = HeaderCacheImpl::new();
        let path = PathBuf::from("/tmp/test.emlx");
        let header = Some("<msg@id>".to_string());

        assert!(cache.get(&path).is_none());
        cache.insert(path.clone(), header.clone());
        assert_eq!(cache.get(&path), Some(header));
    }

    #[test]
    fn test_header_cache_stores_none_for_headerless() {
        let cache = HeaderCacheImpl::new();
        let path = PathBuf::from("/tmp/noheader.emlx");

        cache.cache_headerless(path.clone());
        assert_eq!(cache.get(&path), Some(None));
    }

    #[test]
    fn test_header_cache_stores_header() {
        let cache = HeaderCacheImpl::new();
        let path = PathBuf::from("/tmp/withheader.emlx");
        let header = "<test@message.id>".to_string();

        cache.cache_with_header(path.clone(), header.clone());
        assert_eq!(cache.get(&path), Some(Some(header)));
    }

    #[test]
    fn test_header_cache_contains() {
        let cache = HeaderCacheImpl::new();
        let path = PathBuf::from("/tmp/test.emlx");

        assert!(!cache.contains(&path));
        cache.insert(path.clone(), Some("header".to_string()));
        assert!(cache.contains(&path));
    }

    #[test]
    fn test_header_cache_remove() {
        let cache = HeaderCacheImpl::new();
        let path = PathBuf::from("/tmp/test.emlx");
        let value = Some("header".to_string());

        cache.insert(path.clone(), value.clone());
        assert_eq!(cache.remove(&path), Some(value));
        assert!(cache.remove(&path).is_none());
    }

    #[test]
    fn test_header_cache_clear() {
        let cache = HeaderCacheImpl::new();
        cache.insert(PathBuf::from("/tmp/1.emlx"), Some("h1".to_string()));
        cache.insert(PathBuf::from("/tmp/2.emlx"), Some("h2".to_string()));
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_header_cache_len() {
        let cache = HeaderCacheImpl::new();
        assert_eq!(cache.len(), 0);

        cache.insert(PathBuf::from("/tmp/1.emlx"), Some("h1".to_string()));
        assert_eq!(cache.len(), 1);

        cache.insert(PathBuf::from("/tmp/2.emlx"), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_header_cache_clone() {
        let cache = HeaderCacheImpl::new();
        cache.insert(PathBuf::from("/tmp/1.emlx"), Some("h1".to_string()));

        let cache2 = cache.clone();
        assert_eq!(cache2.len(), 1);
    }
}
