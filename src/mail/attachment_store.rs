//! Attachment store trait for filesystem-backed attachment access.

use crate::domain::AttachmentContent;
use crate::error::MailMcpError;
use std::path::Path;

/// Trait for attachment content access — defines the seam for attachment extraction.
///
/// Production implementation uses the filesystem (.emlx files); tests use in-memory fakes.
pub trait AttachmentStore: Send + Sync {
    /// Get attachment content by message ID and attachment index.
    fn get_attachment(
        &self,
        message_id: i64,
        attachment_index: usize,
    ) -> Result<AttachmentContent, MailMcpError>;

    /// Get the mail root directory for this store.
    fn mail_root(&self) -> &Path;
}

/// In-memory fake attachment store for testing.
#[derive(Debug, Clone, Default)]
pub struct FakeAttachmentStore {
    pub attachments: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<(i64, usize), AttachmentContent>>,
    >,
}

impl FakeAttachmentStore {
    pub fn insert(&self, message_id: i64, index: usize, content: AttachmentContent) {
        self.attachments
            .lock()
            .unwrap()
            .insert((message_id, index), content);
    }
}

impl AttachmentStore for FakeAttachmentStore {
    fn get_attachment(
        &self,
        message_id: i64,
        attachment_index: usize,
    ) -> Result<AttachmentContent, MailMcpError> {
        self.attachments
            .lock()
            .unwrap()
            .get(&(message_id, attachment_index))
            .cloned()
            .ok_or_else(|| MailMcpError::AttachmentNotFound {
                id: attachment_index.to_string(),
                message_id: message_id.to_string(),
            })
    }

    fn mail_root(&self) -> &Path {
        Path::new("/fake")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AttachmentMeta;

    #[test]
    fn fake_store_works() {
        let store = FakeAttachmentStore::default();
        let meta = AttachmentMeta {
            id: "1:0".to_string(),
            filename: "test.txt".to_string(),
        };
        let content = AttachmentContent::extracted(meta, "Hello world", "text");
        store.insert(1, 0, content.clone());

        let retrieved = store.get_attachment(1, 0).unwrap();
        assert_eq!(retrieved.content, Some("Hello world".to_string()));
        assert_eq!(retrieved.meta.filename, "test.txt");
    }

    #[test]
    fn fake_store_returns_not_found() {
        let store = FakeAttachmentStore::default();
        let err = store.get_attachment(1, 0).unwrap_err();
        assert!(matches!(err, MailMcpError::AttachmentNotFound { .. }));
    }
}
