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
//! use std::path::PathBuf;
//! use rusty_apple_mail_mcp::mail::cache::{CacheRegistry, CacheKey};
//!
//! let registry = CacheRegistry::new();
//! let path = PathBuf::from("/path/to/message.emlx");
//! registry.insert_path(CacheKey::new("/Library/Mail/V10", 12345), path.clone());
//!
//! let key = CacheKey::new("/Library/Mail/V10", 12345);
//! if let Some(_found) = registry.get_valid_path(&key) {
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
pub use registry::CacheRegistry;
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
