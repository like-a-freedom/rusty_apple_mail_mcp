//! Domain types for email attachments.

use schemars::JsonSchema;
use serde::Serialize;

/// Metadata about an email attachment.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttachmentMeta {
    /// Stable attachment identifier (format: "`{message_rowid}:{attachment_index}`")
    pub id: String,
    /// Filename of the attachment
    pub filename: String,
}

/// Format of the extracted attachment content.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentFormat {
    /// Text was successfully extracted from the attachment
    ExtractedText,
    /// Content is not available in text form
    NotAvailable,
}

/// Content of an attachment, suitable for LLM consumption.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttachmentContent {
    #[serde(flatten)]
    pub meta: AttachmentMeta,
    /// Format of the content
    pub content_format: ContentFormat,
    /// Extracted text content (if available)
    pub content: Option<String>,
    /// Method used to extract the content
    pub extraction_method: Option<String>,
}

impl AttachmentContent {
    /// Create an `AttachmentContent` with extracted text.
    pub fn extracted(
        meta: AttachmentMeta,
        content: impl Into<String>,
        method: impl Into<String>,
    ) -> Self {
        Self {
            meta,
            content_format: ContentFormat::ExtractedText,
            content: Some(content.into()),
            extraction_method: Some(method.into()),
        }
    }

    /// Create an `AttachmentContent` indicating content is not available.
    pub fn not_available(meta: AttachmentMeta, reason: impl Into<String>) -> Self {
        Self {
            meta,
            content_format: ContentFormat::NotAvailable,
            content: None,
            extraction_method: Some(reason.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_meta_serialization() {
        let meta = AttachmentMeta {
            id: "42:0".to_string(),
            filename: "test.pdf".to_string(),
        };

        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("test.pdf"));
        assert!(!json.contains("mime_type"));
        assert!(!json.contains("size_bytes"));
    }

    #[test]
    fn attachment_content_extracted() {
        let meta = AttachmentMeta {
            id: "42:0".to_string(),
            filename: "test.txt".to_string(),
        };

        let content = AttachmentContent::extracted(meta.clone(), "Hello, World!", "direct_read");

        assert!(matches!(
            content.content_format,
            ContentFormat::ExtractedText
        ));
        assert_eq!(content.content, Some("Hello, World!".to_string()));
        assert_eq!(content.extraction_method, Some("direct_read".to_string()));
    }

    #[test]
    fn attachment_content_not_available() {
        let meta = AttachmentMeta {
            id: "42:0".to_string(),
            filename: "image.png".to_string(),
        };

        let content = AttachmentContent::not_available(
            meta.clone(),
            "image content requires OCR, not in scope",
        );

        assert!(matches!(
            content.content_format,
            ContentFormat::NotAvailable
        ));
        assert!(content.content.is_none());
    }
}
