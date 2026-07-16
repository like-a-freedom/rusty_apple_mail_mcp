//! Text extraction from attachment data using the Strategy pattern.
//!
//! Provides a unified `TextExtractor` trait, a registry of extractors indexed
//! by MIME type, and concrete implementations for each supported format.

mod registry;
mod traits;

pub use registry::ExtractorRegistry;
pub use traits::{ExtractionError, TextExtractor};

use crate::mail::docx::docx_to_markdown;
use crate::mail::pdf::pdf_to_text;
use crate::mail::pptx::pptx_to_text;
use crate::mail::xlsx::xlsx_to_csv;

/// Convenience function: create a default registry and extract text.
pub fn extract_text(bytes: &[u8], mime_type: &str) -> Result<String, ExtractionError> {
    let registry = ExtractorRegistry::builtin();
    registry.extract(bytes, mime_type)
}

// --- Concrete extractors ---

/// Extractor for DOCX (Word) documents.
pub struct DocxExtractor;

impl TextExtractor for DocxExtractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        docx_to_markdown(bytes)
    }
}

/// Extractor for XLSX (Excel) spreadsheets.
pub struct XlsxExtractor;

impl TextExtractor for XlsxExtractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        xlsx_to_csv(bytes)
    }
}

/// Extractor for PPTX (PowerPoint) presentations.
pub struct PptxExtractor;

impl TextExtractor for PptxExtractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        pptx_to_text(bytes)
    }
}

/// Extractor for PDF documents.
pub struct PdfExtractor;

impl TextExtractor for PdfExtractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        pdf_to_text(bytes)
    }
}

/// Extractor for HTML content.
pub struct HtmlExtractor;

impl TextExtractor for HtmlExtractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ExtractionError::Utf8Error)?;
        Ok(crate::mail::html_to_markdown(text))
    }
}

/// Extractor for plain text content (JSON, XML, CSV, Markdown, text/*).
pub struct PlainTextExtractor;

impl TextExtractor for PlainTextExtractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ExtractionError::Utf8Error)?;
        Ok(text.to_string())
    }
}

/// Extractor for RFC822 email attachments.
pub struct Rfc822Extractor;

impl TextExtractor for Rfc822Extractor {
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ExtractionError::Utf8Error)?;
        Ok(text.to_string())
    }
}
