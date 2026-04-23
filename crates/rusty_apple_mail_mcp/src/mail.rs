//! Mail file reading utilities: locate, parse, and extract content.

pub mod docx;
pub mod extractor;
pub mod locator;
pub mod parser;
pub mod pdf;
pub mod pptx;
pub mod xlsx;

pub use docx::{DocxError, docx_to_markdown};
pub use extractor::{ExtractionResult, extract_text, html_to_plain_text};
pub use locator::{
    locate_emlx, locate_emlx_quick, locate_emlx_quick_with_hints, locate_emlx_with_hints,
};
pub use parser::{
    ParsedEmail, RawAttachment, parse_emlx, parse_emlx_without_attachment_content,
    raw_attachments_to_meta,
};
pub use pdf::{PdfError, pdf_to_text};
pub use pptx::{PptxError, pptx_to_text};
pub use xlsx::{XlsxError, xlsx_to_csv};
