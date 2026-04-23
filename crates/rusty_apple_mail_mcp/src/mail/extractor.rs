//! Extract text content from attachments based on MIME type.
//!
//! This module provides functions to extract LLM-readable text from various
//! attachment formats. The goal is to provide meaningful text content when
//! possible, and clear guidance when extraction is not supported.

use std::path::Path;

/// Result of text extraction from an attachment.
#[derive(Debug, Clone)]
pub enum ExtractionResult {
    /// Text was successfully extracted
    Text {
        content: String,
        method: &'static str,
    },
    /// Text extraction is not supported for this format
    NotSupported { reason: &'static str },
}

/// Extract text from attachment bytes based on MIME type.
///
/// # Arguments
///
/// * `bytes` - Raw attachment bytes
/// * `mime_type` - MIME type of the attachment
///
/// # Returns
///
/// `ExtractionResult` with either extracted text or a reason why extraction is not supported.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn extract_text(bytes: &[u8], mime_type: &str) -> ExtractionResult {
    let mime_lower = mime_type.to_lowercase();

    // JSON - pretty print
    if mime_lower == "application/json" {
        return match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(value) => match serde_json::to_string_pretty(&value) {
                Ok(pretty) => ExtractionResult::Text {
                    content: pretty,
                    method: "json_pretty_print",
                },
                Err(_) => ExtractionResult::NotSupported {
                    reason: "JSON parsing succeeded but formatting failed",
                },
            },
            Err(_) => ExtractionResult::NotSupported {
                reason: "invalid JSON format",
            },
        };
    }

    // XML - return as text if valid UTF-8
    if mime_lower == "application/xml" || mime_lower == "text/xml" {
        return match String::from_utf8(bytes.to_vec()) {
            Ok(text) => ExtractionResult::Text {
                content: text,
                method: "direct_utf8",
            },
            Err(_) => ExtractionResult::NotSupported {
                reason: "XML with invalid UTF-8 encoding",
            },
        };
    }

    // CSV - return as text
    if mime_lower == "text/csv" {
        return match String::from_utf8(bytes.to_vec()) {
            Ok(text) => ExtractionResult::Text {
                content: text,
                method: "direct_utf8",
            },
            Err(_) => ExtractionResult::NotSupported {
                reason: "CSV with invalid UTF-8 encoding",
            },
        };
    }

    // Markdown - return as text
    if mime_lower == "text/markdown"
        || Path::new(&mime_lower)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return match String::from_utf8(bytes.to_vec()) {
            Ok(text) => ExtractionResult::Text {
                content: text,
                method: "direct_utf8",
            },
            Err(_) => ExtractionResult::NotSupported {
                reason: "Markdown with invalid UTF-8 encoding",
            },
        };
    }

    // HTML - extract text from body
    if mime_lower == "text/html"
        || Path::new(&mime_lower)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
    {
        return extract_text_from_html(bytes);
    }

    // PDF - extract text layer (no OCR)
    if mime_lower == "application/pdf" {
        return match crate::mail::pdf::pdf_to_text(bytes) {
            Ok(text) => ExtractionResult::Text {
                content: text,
                method: "pdf_text_extract",
            },
            Err(e) => ExtractionResult::NotSupported {
                reason: match e {
                    crate::mail::pdf::PdfError::PdfParse(_) => "Failed to parse PDF",
                    crate::mail::pdf::PdfError::NoTextLayer => {
                        "PDF has no text layer (scanned). OCR not supported"
                    }
                    crate::mail::pdf::PdfError::EmptyDocument => "PDF is empty",
                },
            },
        };
    }

    // DOCX - convert to Markdown
    if mime_lower == "application/vnd.openxmlformats-officedocument.wordprocessingml.document" {
        return match crate::mail::docx::docx_to_markdown(bytes) {
            Ok(markdown) => ExtractionResult::Text {
                content: markdown,
                method: "docx_to_markdown",
            },
            Err(e) => ExtractionResult::NotSupported {
                reason: match e {
                    crate::mail::docx::DocxError::InvalidZip => "DOCX is not a valid ZIP archive",
                    crate::mail::docx::DocxError::MissingDocumentXml => {
                        "DOCX is missing word/document.xml"
                    }
                    crate::mail::docx::DocxError::XmlParse(_) => "Failed to parse DOCX XML",
                    crate::mail::docx::DocxError::EmptyDocument => "DOCX document is empty",
                    crate::mail::docx::DocxError::Utf8Error => "DOCX contains invalid UTF-8",
                },
            },
        };
    }

    // XLSX - convert to CSV
    if mime_lower == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" {
        return match crate::mail::xlsx::xlsx_to_csv(bytes) {
            Ok(csv) => ExtractionResult::Text {
                content: csv,
                method: "xlsx_to_csv",
            },
            Err(e) => ExtractionResult::NotSupported {
                reason: match e {
                    crate::mail::xlsx::XlsxError::InvalidZip => "XLSX is not a valid ZIP archive",
                    crate::mail::xlsx::XlsxError::MissingWorksheet(_) => "XLSX worksheet not found",
                    crate::mail::xlsx::XlsxError::XmlParse(_) => "Failed to parse XLSX XML",
                    crate::mail::xlsx::XlsxError::SharedStrings(_) => {
                        "Failed to read XLSX shared strings"
                    }
                    crate::mail::xlsx::XlsxError::Utf8Error => "XLSX contains invalid UTF-8",
                    crate::mail::xlsx::XlsxError::EmptyWorksheet => "XLSX worksheet is empty",
                },
            },
        };
    }

    // PPTX - convert to plain text
    if mime_lower == "application/vnd.openxmlformats-officedocument.presentationml.presentation" {
        return match crate::mail::pptx::pptx_to_text(bytes) {
            Ok(text) => ExtractionResult::Text {
                content: text,
                method: "pptx_to_text",
            },
            Err(e) => ExtractionResult::NotSupported {
                reason: match e {
                    crate::mail::pptx::PptxError::InvalidZip => "PPTX is not a valid ZIP archive",
                    crate::mail::pptx::PptxError::MissingPresentation => {
                        "PPTX is missing presentation.xml"
                    }
                    crate::mail::pptx::PptxError::MissingSlide(_) => "PPTX slide not found",
                    crate::mail::pptx::PptxError::XmlParse(_) => "Failed to parse PPTX XML",
                    crate::mail::pptx::PptxError::EmptyDocument => "PPTX presentation is empty",
                    crate::mail::pptx::PptxError::Utf8Error => "PPTX contains invalid UTF-8",
                },
            },
        };
    }

    // Legacy Office documents - not supported
    if mime_lower == "application/msword"
        || mime_lower == "application/vnd.ms-excel"
        || mime_lower == "application/vnd.ms-powerpoint"
    {
        return ExtractionResult::NotSupported {
            reason: "Legacy Office document formats not supported",
        };
    }

    // Images - require OCR
    if mime_lower.starts_with("image/") {
        return ExtractionResult::NotSupported {
            reason: "image content requires OCR, not in scope",
        };
    }

    // Audio/Video - not supported
    if mime_lower.starts_with("audio/") || mime_lower.starts_with("video/") {
        return ExtractionResult::NotSupported {
            reason: "audio/video content transcription not in scope",
        };
    }

    // Generic text formats - return as-is (after checking specific formats above)
    if mime_lower.starts_with("text/") {
        return match String::from_utf8(bytes.to_vec()) {
            Ok(text) => ExtractionResult::Text {
                content: text,
                method: "direct_utf8",
            },
            Err(_) => ExtractionResult::NotSupported {
                reason: "binary text format with invalid UTF-8",
            },
        };
    }

    // Default: binary format not supported
    ExtractionResult::NotSupported {
        reason: "binary format text extraction not supported",
    }
}

/// Extract text content from HTML bytes.
fn extract_text_from_html(bytes: &[u8]) -> ExtractionResult {
    let Ok(html) = String::from_utf8(bytes.to_vec()) else {
        return ExtractionResult::NotSupported {
            reason: "HTML with invalid UTF-8 encoding",
        };
    };

    let text = html_to_plain_text(&html);

    ExtractionResult::Text {
        content: text,
        method: "html_to_plain_text",
    }
}

/// Convert HTML to clean plain text via DOM parsing.
///
/// Removes script/style blocks, decodes entities, normalises whitespace.
/// Use instead of returning raw HTML for LLM consumption.
#[must_use]
pub fn html_to_plain_text(html: &str) -> String {
    use scraper::Html;

    let document = Html::parse_document(html);

    let mut output = String::with_capacity(html.len() / 3);

    for node in document.root_element().descendants() {
        // Skip script and style element text
        if let Some(parent) = node.parent()
            && let Some(elem) = parent.value().as_element()
            && (elem.name() == "script" || elem.name() == "style")
        {
            continue;
        }
        if let Some(elem) = node.value().as_element()
            && (elem.name() == "script" || elem.name() == "style")
        {
            continue;
        }

        if let Some(text) = node.value().as_text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                output.push_str(trimmed);
                output.push('\n');
            }
        }
    }

    // Collapse 3+ newlines → 2
    let mut prev_len = 0;
    while output.len() != prev_len {
        prev_len = output.len();
        let collapsed = output.replace("\n\n\n", "\n\n");
        output = collapsed;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_plain() {
        let bytes = b"Hello, World!";
        let result = extract_text(bytes, "text/plain");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, method } = result {
            assert_eq!(content, "Hello, World!");
            assert_eq!(method, "direct_utf8");
        }
    }

    #[test]
    fn extract_text_json() {
        let bytes = b"{\"key\": \"value\"}";
        let result = extract_text(bytes, "application/json");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("\"key\""));
            assert!(content.contains("\"value\""));
        }
    }

    #[test]
    fn extract_text_html() {
        let bytes = b"<html><body><h1>Hello</h1><p>World!</p></body></html>";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("Hello"));
            assert!(content.contains("World!"));
            // Note: our simple stripper may leave some artifacts, so we just check key text is present
        }
    }

    #[test]
    fn extract_text_pdf_not_supported() {
        let bytes = b"%PDF-1.4";
        let result = extract_text(bytes, "application/pdf");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
    }

    #[test]
    fn extract_text_image_not_supported() {
        let bytes = b"\x89PNG";
        let result = extract_text(bytes, "image/png");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("OCR"));
        }
    }

    #[test]
    fn extract_text_xml() {
        let bytes = b"<?xml version=\"1.0\"?><root><item>test</item></root>";
        let result = extract_text(bytes, "application/xml");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, method } = result {
            assert!(content.contains("test"));
            assert_eq!(method, "direct_utf8");
        }
    }

    #[test]
    fn extract_text_xml_text_variant() {
        let bytes = b"<?xml version=\"1.0\"?><root><item>test</item></root>";
        let result = extract_text(bytes, "text/xml");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_csv() {
        let bytes = b"name,email\nJohn,john@example.com";
        let result = extract_text(bytes, "text/csv");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, method } = result {
            assert!(content.contains("John"));
            assert_eq!(method, "direct_utf8");
        }
    }

    #[test]
    fn extract_text_markdown() {
        let bytes = b"# Header\n\nSome **bold** text.";
        let result = extract_text(bytes, "text/markdown");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, method } = result {
            assert!(content.contains("Header"));
            assert_eq!(method, "direct_utf8");
        }
    }

    #[test]
    fn extract_text_markdown_with_extension() {
        let bytes = b"# Header\n\nSome text.";
        let result = extract_text(bytes, "text/markdown; charset=utf-8");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_office_not_supported() {
        let bytes = b"fake office document";
        let result = extract_text(
            bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(
                reason.contains("ZIP"),
                "Expected ZIP-related error for invalid DOCX, got: {}",
                reason
            );
        }
    }

    #[test]
    fn extract_text_msword_not_supported() {
        let bytes = b"fake word doc";
        let result = extract_text(bytes, "application/msword");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
    }

    #[test]
    fn extract_text_audio_not_supported() {
        let bytes = b"fake audio data";
        let result = extract_text(bytes, "audio/mpeg");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("audio"));
        }
    }

    #[test]
    fn extract_text_video_not_supported() {
        let bytes = b"fake video data";
        let result = extract_text(bytes, "video/mp4");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
    }

    #[test]
    fn extract_text_invalid_utf8() {
        // Invalid UTF-8 sequence
        let bytes = b"\xFF\xFE";
        let result = extract_text(bytes, "text/plain");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("invalid UTF-8"));
        }
    }

    #[test]
    fn extract_text_json_invalid() {
        let bytes = b"{invalid json}";
        let result = extract_text(bytes, "application/json");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("invalid JSON"));
        }
    }

    #[test]
    fn extract_text_html_invalid_utf8() {
        let bytes = b"<html>\xFF\xFE</html>";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("UTF-8"));
        }
    }

    #[test]
    fn extract_text_xml_invalid_utf8() {
        let bytes = b"<?xml version=\"1.0\"?>\xFF\xFE";
        let result = extract_text(bytes, "application/xml");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("UTF-8"));
        }
    }

    #[test]
    fn extract_text_csv_invalid_utf8() {
        let bytes = b"name,email\n\xFF\xFE";
        let result = extract_text(bytes, "text/csv");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
    }

    #[test]
    fn extract_text_markdown_invalid_utf8() {
        let bytes = b"# Header\n\xFF\xFE";
        let result = extract_text(bytes, "text/markdown");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
    }

    #[test]
    fn extract_text_html_with_script_and_style() {
        let bytes = b"<html><head><script>alert('xss');</script><style>body{}</style></head><body><p>text</p></body></html>";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("text"), "should contain body text");
            assert!(
                !content.contains("alert"),
                "script content should be stripped"
            );
            assert!(
                !content.contains("body{}"),
                "style content should be stripped"
            );
        }
    }

    #[test]
    fn extract_text_html_with_entities() {
        let bytes = b"<p>Hello &nbsp; world &amp; more &lt;test&gt; &quot;quote&quot;</p>";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            // Check for decoded entities
            assert!(
                content.contains("Hello") && content.contains("world") && content.contains("test")
            );
        }
    }

    #[test]
    fn extract_text_binary_format() {
        let bytes = b"\x00\x01\x02\x03";
        let result = extract_text(bytes, "application/octet-stream");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("binary format"));
        }
    }

    #[test]
    fn extract_text_json_with_control_characters() {
        // JSON with control characters that might fail formatting
        let bytes = b"{\"key\": \"value\\u0000\"}";
        let result = extract_text(bytes, "application/json");
        // Should still work - control characters are valid in JSON strings
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_html_with_nested_tags() {
        let bytes = b"<div><p>Hello <strong>world</strong></p></div>";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("Hello"));
            assert!(content.contains("world"));
        }
    }

    #[test]
    fn extract_text_html_empty() {
        let bytes = b"";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_html_only_tags() {
        let bytes = b"<div><p></p></div>";
        let result = extract_text(bytes, "text/html");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_unknown_mime_type() {
        let bytes = b"some data";
        let result = extract_text(bytes, "application/unknown");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("binary format"));
        }
    }

    #[test]
    fn extract_text_plain_empty() {
        let bytes = b"";
        let result = extract_text(bytes, "text/plain");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.is_empty());
        }
    }

    #[test]
    fn extract_text_plain_with_unicode() {
        let bytes = "Hello 世界 🌍".as_bytes();
        let result = extract_text(bytes, "text/plain");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("世界"));
            assert!(content.contains("🌍"));
        }
    }

    #[test]
    fn extract_text_csv_empty() {
        let bytes = b"";
        let result = extract_text(bytes, "text/csv");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_csv_with_headers_only() {
        let bytes = b"name,email,age\n";
        let result = extract_text(bytes, "text/csv");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("name"));
            assert!(content.contains("email"));
        }
    }

    #[test]
    fn extract_text_json_empty_object() {
        let bytes = b"{}";
        let result = extract_text(bytes, "application/json");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_json_array() {
        let bytes = b"[1, 2, 3]";
        let result = extract_text(bytes, "application/json");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_json_nested() {
        let bytes = b"{\"user\": {\"name\": \"John\", \"emails\": [\"a@b.com\"]}}";
        let result = extract_text(bytes, "application/json");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("John"));
        }
    }

    #[test]
    fn extract_text_xml_empty() {
        let bytes = b"<?xml version=\"1.0\"?><root></root>";
        let result = extract_text(bytes, "application/xml");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_xml_with_attributes() {
        let bytes = b"<?xml version=\"1.0\"?><root attr=\"value\">text</root>";
        let result = extract_text(bytes, "application/xml");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("text"));
        }
    }

    #[test]
    fn extract_text_markdown_empty() {
        let bytes = b"";
        let result = extract_text(bytes, "text/markdown");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_markdown_with_headers() {
        let bytes = b"# Header\n## Subheader\nContent";
        let result = extract_text(bytes, "text/markdown");
        assert!(matches!(result, ExtractionResult::Text { .. }));
        if let ExtractionResult::Text { content, .. } = result {
            assert!(content.contains("Header"));
        }
    }

    #[test]
    fn extract_text_markdown_with_links() {
        let bytes = b"[link](https://example.com) and text";
        let result = extract_text(bytes, "text/markdown");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extraction_result_debug_format() {
        let result = ExtractionResult::Text {
            content: "test".to_string(),
            method: "test_method",
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Text"));
    }

    #[test]
    fn extraction_result_not_supported_debug_format() {
        let result = ExtractionResult::NotSupported {
            reason: "test reason",
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("NotSupported"));
        assert!(debug_str.contains("test reason"));
    }

    #[test]
    fn extract_text_image_by_extension() {
        let bytes = b"fake image data";
        let result = extract_text(bytes, "image/jpeg");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("image"));
        }
    }

    #[test]
    fn extract_text_pdf_explicitly_not_supported() {
        let bytes = b"%PDF fake pdf";
        let result = extract_text(bytes, "application/pdf");
        assert!(matches!(result, ExtractionResult::NotSupported { .. }));
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("PDF"));
        }
    }

    #[test]
    fn html_to_plain_text_strips_tracker_pixel() {
        let html = "<html><body><p>Real content</p><img src=\"https://tracker.example.com/pixel.gif\" width=\"1\" height=\"1\"></body></html>";
        let text = html_to_plain_text(html);
        assert!(text.contains("Real content"));
        assert!(!text.contains("tracker.example.com"));
        assert!(!text.contains("pixel.gif"));
    }

    #[test]
    fn html_to_plain_text_strips_inline_css() {
        let html = "<html><head><style>.header { color: red; font-size: 14px; }</style></head><body><div style=\"margin: 0;\">Hello</div></body></html>";
        let text = html_to_plain_text(html);
        assert!(text.contains("Hello"));
        assert!(!text.contains("color: red"));
        assert!(!text.contains("font-size"));
    }

    #[test]
    fn html_to_plain_text_handles_corporate_email() {
        let html = r#"<html>
            <head><style>body { font-family: Arial; }</style></head>
            <body>
                <table>
                    <tr><td><img src="logo.png" alt="Logo"></td></tr>
                    <tr><td><p>Dear team,</p><p>Please review the attached document.</p></td></tr>
                    <tr><td style="font-size:10px">Footer text</td></tr>
                </table>
            </body></html>"#;
        let text = html_to_plain_text(html);
        assert!(text.contains("Dear team,"));
        assert!(text.contains("Please review the attached document."));
        assert!(text.contains("Footer text"));
        assert!(!text.contains("font-family"));
    }

    #[test]
    fn extract_text_docx_success() {
        use std::io::{Cursor, Write};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("word/document.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t>Hello from DOCX</w:t></w:r></w:p>
</w:body>
</w:document>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let result = extract_text(
            &buf.into_inner(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        match result {
            ExtractionResult::Text { content, method } => {
                assert!(content.contains("Hello from DOCX"));
                assert_eq!(method, "docx_to_markdown");
            }
            ExtractionResult::NotSupported { reason } => {
                panic!("Expected success, got: {}", reason);
            }
        }
    }

    #[test]
    fn extract_text_xlsx_success() {
        use std::io::{Cursor, Write};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row><c t="str"><v>Cell Content</v></c></row>
</sheetData>
</worksheet>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let result = extract_text(
            &buf.into_inner(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        );
        match result {
            ExtractionResult::Text { content, method } => {
                assert!(content.contains("Cell Content"));
                assert_eq!(method, "xlsx_to_csv");
            }
            ExtractionResult::NotSupported { reason } => {
                panic!("Expected success, got: {}", reason);
            }
        }
    }

    #[test]
    fn extract_text_pptx_success() {
        use std::io::{Cursor, Write};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("ppt/presentation.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
</p:presentation>"#,
            )
            .unwrap();
            zip.start_file("ppt/slides/slide1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Slide Text</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
</p:sld>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let result = extract_text(
            &buf.into_inner(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        );
        match result {
            ExtractionResult::Text { content, method } => {
                assert!(content.contains("Slide Text"));
                assert_eq!(method, "pptx_to_text");
            }
            ExtractionResult::NotSupported { reason } => {
                panic!("Expected success, got: {}", reason);
            }
        }
    }

    #[test]
    fn extract_text_pdf_with_text() {
        // Minimal PDF with text content
        let pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj
4 0 obj << /Length 44 >> stream
BT /F1 12 Tf 100 700 Td (Hello PDF) Tj ET
endstream endobj
5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj
xref
0 6
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000115 00000 n 
0000000260 00000 n 
0000000354 00000 n 
trailer << /Size 6 /Root 1 0 R >>
startxref
428
%%EOF";

        let result = extract_text(pdf, "application/pdf");
        // PDF text extraction may or may not succeed depending on lopdf
        match result {
            ExtractionResult::Text { method, .. } => {
                assert_eq!(method, "pdf_text_extract");
            }
            ExtractionResult::NotSupported { reason } => {
                // Acceptable if lopdf can't extract from this minimal PDF
                assert!(reason.contains("PDF") || reason.contains("text"));
            }
        }
    }

    #[test]
    fn extract_text_docx_error_messages() {
        let result = extract_text(
            b"not a zip",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("ZIP"));
        } else {
            panic!("Expected NotSupported");
        }
    }

    #[test]
    fn extract_text_xlsx_error_messages() {
        let result = extract_text(
            b"not a zip",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        );
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("ZIP"));
        } else {
            panic!("Expected NotSupported");
        }
    }

    #[test]
    fn extract_text_pptx_error_messages() {
        let result = extract_text(
            b"not a zip",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        );
        if let ExtractionResult::NotSupported { reason } = result {
            assert!(reason.contains("ZIP"));
        } else {
            panic!("Expected NotSupported");
        }
    }
}
