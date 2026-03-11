//! Mail file reading utilities: locate, parse, and extract content.

mod extractor;
mod locator;
mod parser;

pub use extractor::{ExtractionResult, extract_text};
pub use locator::{locate_emlx, locate_emlx_quick};
pub use parser::{ParsedEmail, RawAttachment, parse_emlx, raw_attachments_to_meta};
