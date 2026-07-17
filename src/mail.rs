//! Mail file reading utilities: locate, parse, and extract content.

pub mod attachment_store;
pub mod cache;
pub mod docx;
pub mod extract;
pub mod extractor;
pub mod filesystem_attachment_store;
pub mod locator;
pub mod parser;
pub mod pdf;
pub mod pptx;
pub mod xlsx;

pub use attachment_store::{AttachmentStore, FakeAttachmentStore};
pub use cache::{
    Cache, CacheKey, CacheRegistry, HeaderCache, HeaderCacheImpl, MailboxIndex, MailboxIndexCache,
    MailboxIndexCacheImpl, MailboxIndexGuard, PathCache, PathCacheImpl,
};

pub use docx::docx_to_markdown;
pub use extract::{ExtractionError, extract_text};
pub use extractor::{html_to_markdown, html_to_plain_text, strip_quoted_replies};
pub use filesystem_attachment_store::FilesystemAttachmentStore;
pub use locator::EmlxLocator;
pub use parser::{
    ParsedEmail, RawAttachment, parse_emlx, parse_emlx_without_attachment_content,
    raw_attachments_to_meta,
};
pub use pdf::pdf_to_text;
pub use pptx::pptx_to_text;
pub use xlsx::xlsx_to_csv;
