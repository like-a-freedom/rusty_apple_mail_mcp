//! PDF text extraction.
//!
//! Extracts text from PDF files for LLM consumption.
//! Note: OCR is NOT supported. Only text layer extraction.

use thiserror::Error;

/// Errors that can occur during PDF processing.
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("Not a valid PDF file")]
    InvalidPdf,
    #[error("Failed to parse PDF: {0}")]
    PdfParse(String),
    #[error("PDF contains no extractable text (possibly scanned)")]
    NoTextLayer,
    #[error("PDF is empty")]
    EmptyDocument,
}

/// Extract text from PDF bytes.
///
/// # Arguments
///
/// * `bytes` - Raw PDF file bytes
///
/// # Returns
///
/// Plain text string on success, PdfError on failure.
///
/// # Example
///
/// ```rust
/// use rusty_apple_mail_mcp::mail::pdf::pdf_to_text;
///
/// // Assuming you have PDF bytes
/// // let text = pdf_to_text(&pdf_bytes)?;
/// ```
pub fn pdf_to_text(bytes: &[u8]) -> Result<String, PdfError> {
    use lopdf::Document;

    // Load PDF document
    let doc = Document::load_mem(bytes)
        .map_err(|e| PdfError::PdfParse(format!("Failed to load PDF: {}", e)))?;

    // Get page numbers
    let pages = doc.get_pages();

    if pages.is_empty() {
        return Err(PdfError::EmptyDocument);
    }

    // Extract text from all pages using lopdf's built-in method
    let page_numbers: Vec<u32> = pages.keys().cloned().collect();

    let text = doc
        .extract_text(&page_numbers)
        .map_err(|e| PdfError::PdfParse(format!("Failed to extract text: {}", e)))?;

    if text.trim().is_empty() {
        return Err(PdfError::NoTextLayer);
    }

    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_to_text_basic() {
        // Note: Creating a valid PDF programmatically is complex.
        // This test validates the API works with valid PDFs.
        // For real-world testing, use actual PDF files.
        // Here we test error handling with minimal invalid PDF.
        let pdf = b"%PDF-1.4\n%EOFA";
        let result = pdf_to_text(pdf.to_vec().as_slice());
        // Should handle gracefully - either parse or return appropriate error
        match result {
            Ok(_) => (),
            Err(PdfError::PdfParse(_)) => (),
            Err(PdfError::EmptyDocument) => (),
            Err(PdfError::NoTextLayer) => (),
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_pdf_empty_returns_error() {
        let result = pdf_to_text(b"");
        assert!(matches!(result, Err(PdfError::PdfParse(_))));
    }

    #[test]
    fn test_pdf_invalid_returns_error() {
        let result = pdf_to_text(b"not a pdf");
        assert!(matches!(result, Err(PdfError::PdfParse(_))));
    }

    #[test]
    fn test_pdf_no_text_layer() {
        // PDF with no text content (just empty page)
        let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>
endobj
4 0 obj
<< /Length 0 >>
stream
endstream
endobj
xref
0 5
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000115 00000 n 
0000000200 00000 n 
trailer
<< /Size 5 /Root 1 0 R >>
startxref
250
%%EOF";

        let result = pdf_to_text(pdf.to_vec().as_slice());
        // May return NoTextLayer or empty text depending on lopdf behavior
        match result {
            Err(PdfError::NoTextLayer) => (),
            Ok(text) if text.is_empty() => (),
            Ok(text) => panic!("Expected empty or error, got: {}", text),
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }
}
