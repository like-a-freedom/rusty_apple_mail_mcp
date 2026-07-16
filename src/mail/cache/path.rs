//! Path cache implementation for resolved message paths.
//!
//! This module provides a thread-safe cache for storing and retrieving
//! the filesystem paths of .emlx files keyed by mail root and message row ID.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::mail::CacheKey;
use crate::mail::cache::traits::{Cache, PathCache};

/// Thread-safe implementation of a path cache.
///
/// Uses a RwLock for concurrent access, allowing multiple readers
/// or a single writer at any time.
#[derive(Debug, Default)]
pub struct PathCacheImpl {
    inner: Arc<RwLock<HashMap<CacheKey, PathBuf>>>,
}

impl PathCacheImpl {
    /// Create a new empty path cache.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new path cache with a given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::with_capacity(capacity))),
        }
    }
}

impl Clone for PathCacheImpl {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Cache<CacheKey, PathBuf> for PathCacheImpl {
    fn get(&self, key: &CacheKey) -> Option<PathBuf> {
        if let Ok(cache) = self.inner.read() {
            cache.get(key).cloned()
        } else {
            None
        }
    }

    fn insert(&self, key: CacheKey, value: PathBuf) {
        if let Ok(mut cache) = self.inner.write() {
            cache.insert(key, value);
        }
    }

    fn contains(&self, key: &CacheKey) -> bool {
        if let Ok(cache) = self.inner.read() {
            cache.contains_key(key)
        } else {
            false
        }
    }

    fn remove(&self, key: &CacheKey) -> Option<PathBuf> {
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

impl PathCache for PathCacheImpl {
    fn get_valid(&self, key: &CacheKey) -> Option<PathBuf> {
        let path = self.get(key)?;
        if path.exists() {
            Some(path)
        } else {
            // Remove stale entry
            self.remove(key);
            None
        }
    }
}

impl PathCacheImpl {
    /// Get a reference to the underlying cache for testing purposes.
    #[cfg(test)]
    pub fn inner(&self) -> &Arc<RwLock<HashMap<CacheKey, PathBuf>>> {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_path_cache_roundtrip() {
        let cache = PathCacheImpl::new();
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("123.emlx");
        std::fs::write(&file_path, b"content").unwrap();

        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 42,
        };

        assert!(cache.get(&key).is_none());
        cache.insert(key.clone(), file_path.clone());
        assert_eq!(cache.get(&key), Some(file_path));
    }

    #[test]
    fn test_path_cache_get_valid_returns_none_for_stale_entry() {
        let cache = PathCacheImpl::new();
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("stale.emlx");

        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 99,
        };

        cache.insert(key.clone(), file_path.clone());
        assert!(cache.get(&key).is_some());
        assert!(cache.get_valid(&key).is_none());

        // After stale removal, re-insert and verify the file is now found
        std::fs::write(&file_path, b"content").unwrap();
        cache.insert(key.clone(), file_path.clone());
        assert!(cache.get_valid(&key).is_some());
    }

    #[test]
    fn test_path_cache_contains() {
        let cache = PathCacheImpl::new();
        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 1,
        };

        assert!(!cache.contains(&key));
        cache.insert(key.clone(), PathBuf::from("/tmp/test.emlx"));
        assert!(cache.contains(&key));
    }

    #[test]
    fn test_path_cache_remove() {
        let cache = PathCacheImpl::new();
        let key = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 1,
        };
        let value = PathBuf::from("/tmp/test.emlx");

        cache.insert(key.clone(), value.clone());
        assert_eq!(cache.remove(&key), Some(value));
        assert!(cache.remove(&key).is_none());
    }

    #[test]
    fn test_path_cache_clear() {
        let cache = PathCacheImpl::new();
        let key1 = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 1,
        };
        let key2 = CacheKey {
            mail_root: PathBuf::from("/"),
            message_rowid: 2,
        };

        cache.insert(key1.clone(), PathBuf::from("/tmp/1.emlx"));
        cache.insert(key2.clone(), PathBuf::from("/tmp/2.emlx"));
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(!cache.contains(&key1));
        assert!(!cache.contains(&key2));
    }

    #[test]
    fn test_path_cache_len() {
        let cache = PathCacheImpl::new();
        assert_eq!(cache.len(), 0);

        cache.insert(
            CacheKey {
                mail_root: PathBuf::from("/"),
                message_rowid: 1,
            },
            PathBuf::from("/tmp/1.emlx"),
        );
        assert_eq!(cache.len(), 1);

        cache.insert(
            CacheKey {
                mail_root: PathBuf::from("/"),
                message_rowid: 2,
            },
            PathBuf::from("/tmp/2.emlx"),
        );
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_path_cache_clone() {
        let cache = PathCacheImpl::new();
        cache.insert(
            CacheKey {
                mail_root: PathBuf::from("/"),
                message_rowid: 1,
            },
            PathBuf::from("/tmp/1.emlx"),
        );

        let cache2 = cache.clone();
        assert_eq!(cache2.len(), 1);
    }
}
