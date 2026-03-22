//! Mail file reading utilities: locate, parse, and extract content.

mod extractor;
mod locator;
mod parser;

pub use extractor::{ExtractionResult, extract_text, html_to_plain_text};
pub use locator::{
    locate_emlx, locate_emlx_quick, locate_emlx_quick_with_hints, locate_emlx_with_hints,
};
pub use parser::{
    ParsedEmail, RawAttachment, parse_emlx, parse_emlx_without_attachment_content,
    raw_attachments_to_meta,
};
