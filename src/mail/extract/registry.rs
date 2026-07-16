//! Registry of text extractors keyed by MIME type.
//!
//! Provides dynamic MIME-based dispatch to the appropriate extractor.

use super::traits::{ExtractionError, TextExtractor};

/// Registry of text extractors indexed by MIME type.
pub struct ExtractorRegistry {
    extractors: Vec<(&'static [&'static str], Box<dyn TextExtractor>)>,
}

impl ExtractorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
        }
    }

    /// Register an extractor for one or more MIME types.
    pub fn register(
        &mut self,
        mime_types: &'static [&'static str],
        extractor: Box<dyn TextExtractor>,
    ) {
        self.extractors.push((mime_types, extractor));
    }

    /// Extract text from bytes by looking up an extractor for the MIME type.
    pub fn extract(&self, bytes: &[u8], mime_type: &str) -> Result<String, ExtractionError> {
        let mime_lower = mime_type.to_lowercase();
        for (mime_types, extractor) in &self.extractors {
            if mime_types.iter().any(|m| *m == mime_lower) {
                return extractor.extract(bytes);
            }
            for m in mime_types.iter() {
                if m.ends_with('/') && mime_lower.starts_with(m) {
                    return extractor.extract(bytes);
                }
            }
        }
        Err(ExtractionError::UnsupportedMime(mime_type.to_string()))
    }

    /// Build the default registry with all built-in extractors.
    pub fn builtin() -> Self {
        let mut reg = Self::new();
        reg.register(
            &["application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
            Box::new(super::DocxExtractor),
        );
        reg.register(
            &["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"],
            Box::new(super::XlsxExtractor),
        );
        reg.register(
            &["application/vnd.openxmlformats-officedocument.presentationml.presentation"],
            Box::new(super::PptxExtractor),
        );
        reg.register(&["application/pdf"], Box::new(super::PdfExtractor));
        reg.register(&["text/html"], Box::new(super::HtmlExtractor));
        reg.register(&["text/"], Box::new(super::PlainTextExtractor));
        reg.register(
            &[
                "application/json",
                "application/xml",
                "text/xml",
                "text/csv",
                "text/markdown",
            ],
            Box::new(super::PlainTextExtractor),
        );
        reg.register(&["message/rfc822"], Box::new(super::Rfc822Extractor));
        reg
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
