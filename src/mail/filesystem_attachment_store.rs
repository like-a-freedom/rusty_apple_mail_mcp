//! Filesystem implementation of `AttachmentStore`.

use crate::AttachmentContent;
use crate::error::MailMcpError;
use std::path::{Path, PathBuf};

/// Filesystem-backed attachment store.
///
/// Uses the existing mail locator and parser to find and extract attachments.
#[derive(Debug)]
pub struct FilesystemAttachmentStore {
    mail_root: PathBuf,
    #[expect(dead_code)]
    config_allowed_accounts: Option<Vec<String>>,
}

impl FilesystemAttachmentStore {
    /// Create a new attachment store for the given mail root.
    pub fn new(mail_root: impl Into<PathBuf>) -> Self {
        Self {
            mail_root: mail_root.into(),
            config_allowed_accounts: None,
        }
    }

    /// Create a new attachment store with account allowlist.
    pub fn with_allowed_accounts(
        mail_root: impl Into<PathBuf>,
        allowed_accounts: Option<Vec<String>>,
    ) -> Self {
        Self {
            mail_root: mail_root.into(),
            config_allowed_accounts: allowed_accounts,
        }
    }

    /// Get the mail root directory.
    pub fn mail_root(&self) -> &Path {
        &self.mail_root
    }
}

impl crate::mail::AttachmentStore for FilesystemAttachmentStore {
    fn get_attachment(
        &self,
        _message_id: i64,
        _attachment_index: usize,
    ) -> Result<AttachmentContent, MailMcpError> {
        // We need to load the message to get attachment metadata
        // For now, return an error indicating this needs the full message loading
        Err(MailMcpError::Validation(
            "FilesystemAttachmentStore.get_attachment requires message context. Use get_message_with_attachments instead."
                .to_string(),
        ))
    }

    fn mail_root(&self) -> &Path {
        &self.mail_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn filesystem_store_creation() {
        let temp_dir = TempDir::new().unwrap();
        let store = FilesystemAttachmentStore::new(temp_dir.path());
        assert_eq!(store.mail_root(), temp_dir.path());
    }
}
