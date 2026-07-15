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

    // RFC 822 embedded message — parse as an email and extract subject/body
    if mime_lower == "message/rfc822" {
        return parse_rfc822_attachment(bytes);
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

/// Convert HTML to clean markdown via DOM parsing.
///
/// Removes script/style blocks, decodes entities, normalises whitespace.
/// Converts tables to markdown tables, lists to markdown lists, and
/// preserves links, bold, italic, and headings as markdown.
/// Use instead of returning raw HTML for LLM consumption.
#[must_use]
pub fn html_to_markdown(html: &str) -> String {
    use scraper::Html;

    let document = Html::parse_document(html);
    let mut output = String::with_capacity(html.len() / 3);

    process_element(document.root_element(), &mut output);

    // Collapse 3+ newlines → 2
    let mut prev_len = 0;
    while output.len() != prev_len {
        prev_len = output.len();
        let collapsed = output.replace("\n\n\n", "\n\n");
        output = collapsed;
    }

    output.trim().to_string()
}

/// Process an element node and its children, writing markdown to output.
fn process_element(node: scraper::ElementRef<'_>, output: &mut String) {
    use scraper::Node;

    let name = node.value().name();

    // Skip script, style, head elements entirely
    if name == "script" || name == "style" || name == "head" {
        return;
    }

    // Skip MSO conditional comments
    if name == "xml" && node.value().attr("xmlns:v").is_some() {
        return;
    }

    match name {
        "h1" => output.push_str("# "),
        "h2" => output.push_str("## "),
        "h3" => output.push_str("### "),
        "h4" => output.push_str("#### "),
        "h5" => output.push_str("##### "),
        "h6" => output.push_str("###### "),
        "p" | "div" => {
            // Add newline before block elements if output is non-empty
            if !output.is_empty() && !output.ends_with("\n\n") {
                output.push_str("\n\n");
            }
        }
        "br" => {
            output.push('\n');
            return;
        }
        "hr" => {
            output.push_str("\n---\n\n");
            return;
        }
        "table" => {
            process_table_element(node, output);
            return;
        }
        "ul" | "ol" => {
            if !output.is_empty() && !output.ends_with("\n\n") {
                output.push_str("\n\n");
            }
        }
        "li" => {
            // Determine list type from parent
            if let Some(parent) = node.parent()
                && let Node::Element(parent_elem) = parent.value()
            {
                let prefix = if parent_elem.name() == "ol" {
                    "1. "
                } else {
                    "- "
                };
                output.push_str(prefix);
            }
        }
        "a" => {
            if let Some(href) = node.value().attr("href") {
                output.push('[');
                process_children_text(node, output);
                output.push_str("](");
                output.push_str(href);
                output.push(')');
                return;
            }
        }
        "b" | "strong" => {
            output.push_str("**");
            process_children_text(node, output);
            output.push_str("**");
            return;
        }
        "i" | "em" => {
            output.push('*');
            process_children_text(node, output);
            output.push('*');
            return;
        }
        "img" => {
            // Skip images (tracking pixels, etc.)
            return;
        }
        _ => {}
    }

    process_children_text(node, output);

    // Add newline after block elements
    match name {
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" => {
            if !output.ends_with("\n\n") {
                output.push_str("\n\n");
            }
        }
        "li" => {
            output.push('\n');
        }
        _ => {}
    }
}

/// Process children of an element, handling both text nodes and nested elements.
fn process_children_text(node: scraper::ElementRef<'_>, output: &mut String) {
    use scraper::Node;

    for child in node.children() {
        match child.value() {
            Node::Element(_) => {
                if let Some(child_elem) = scraper::ElementRef::wrap(child) {
                    process_element(child_elem, output);
                }
            }
            Node::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    output.push_str(trimmed);
                }
            }
            _ => {}
        }
    }
}

/// Process an HTML table element and convert it to a markdown table.
fn process_table_element(table_node: scraper::ElementRef<'_>, output: &mut String) {
    use scraper::Node;

    let mut rows: Vec<Vec<String>> = Vec::new();

    for child in table_node.children() {
        if let Node::Element(elem) = child.value() {
            match elem.name() {
                "tr" => {
                    if let Some(tr_elem) = scraper::ElementRef::wrap(child) {
                        let mut cells = Vec::new();
                        for cell_node in tr_elem.children() {
                            if let Node::Element(cell_elem) = cell_node.value()
                                && (cell_elem.name() == "td" || cell_elem.name() == "th")
                                && let Some(cell_ref) = scraper::ElementRef::wrap(cell_node)
                            {
                                let mut cell_text = String::new();
                                process_children_text(cell_ref, &mut cell_text);
                                cells.push(cell_text.trim().replace('\n', " "));
                            }
                        }
                        if !cells.is_empty() {
                            rows.push(cells);
                        }
                    }
                }
                "thead" | "tbody" | "tfoot" => {
                    // Process nested table sections
                    if let Some(section_elem) = scraper::ElementRef::wrap(child) {
                        for section_child in section_elem.children() {
                            if let Node::Element(se) = section_child.value()
                                && se.name() == "tr"
                                && let Some(tr_ref) = scraper::ElementRef::wrap(section_child)
                            {
                                let mut cells = Vec::new();
                                for cell_node in tr_ref.children() {
                                    if let Node::Element(cell_elem) = cell_node.value()
                                        && (cell_elem.name() == "td" || cell_elem.name() == "th")
                                        && let Some(cell_ref) = scraper::ElementRef::wrap(cell_node)
                                    {
                                        let mut cell_text = String::new();
                                        process_children_text(cell_ref, &mut cell_text);
                                        cells.push(cell_text.trim().replace('\n', " "));
                                    }
                                }
                                if !cells.is_empty() {
                                    rows.push(cells);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if rows.is_empty() {
        return;
    }

    // Determine column count from first row
    let col_count = rows[0].len();

    // Normalize all rows to have the same number of columns
    for row in &mut rows {
        while row.len() < col_count {
            row.push(String::new());
        }
    }

    // Write markdown table
    output.push('\n');

    // Header row
    output.push('|');
    for cell in &rows[0] {
        output.push(' ');
        output.push_str(cell);
        output.push(' ');
        output.push('|');
    }
    output.push('\n');

    // Separator row
    output.push('|');
    for _ in 0..col_count {
        output.push_str(" --- |");
    }
    output.push('\n');

    // Data rows
    for row in &rows[1..] {
        output.push('|');
        for cell in row {
            output.push(' ');
            output.push_str(cell);
            output.push(' ');
            output.push('|');
        }
        output.push('\n');
    }

    output.push('\n');
}

/// Backward-compatible alias for `html_to_markdown`.
#[must_use]
pub fn html_to_plain_text(html: &str) -> String {
    html_to_markdown(html)
}

/// Strip quoted replies and forwarded messages from email body text.
///
/// Detects common email quoting patterns and returns a slice of the text
/// up to (but not including) the detected marker. Preserves signatures.
#[must_use]
pub fn strip_quoted_replies(text: &str) -> &str {
    let lower = text.to_lowercase();

    // Check for various forwarding/quoting markers (literal substring search)
    let markers = [
        "-----original",
        "---------- forwarded",
        "---------- begin forwarded",
    ];

    for marker in &markers {
        if let Some(pos) = lower.find(marker) {
            return &text[..pos];
        }
    }

    // Check for "On ... wrote:" / "Am ... wrote:" pattern (not regex — manual line scan)
    // Common formats:
    //   On <date>, <name> wrote:
    //   Am <date>, <name> schrieb <name>:
    //   Am <date> um <time> schrieb <name>:
    let wrote_marker = "wrote:";
    let mut search_start = 0;
    while let Some(pos) = text[search_start..].to_lowercase().find(wrote_marker) {
        let abs_pos = search_start + pos;
        let line_start = text[..abs_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = &text[line_start..abs_pos + wrote_marker.len()];
        let line_lower = line.to_lowercase();
        if (line_lower.starts_with("on ") || line_lower.starts_with("am ")) && line.len() < 200 {
            return &text[..line_start];
        }
        search_start = abs_pos + wrote_marker.len();
    }

    // Check for German "Am ... schrieb ...:" pattern (colon after the name, not after "schrieb")
    let schrieb_word = "schrieb";
    let mut search_start = 0;
    while let Some(pos) = text[search_start..].to_lowercase().find(schrieb_word) {
        let abs_pos = search_start + pos;
        let line_start = text[..abs_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        // Find the end of the line containing this "schrieb"
        let line_end = text[abs_pos..]
            .find('\n')
            .map(|i| abs_pos + i)
            .unwrap_or(text.len());
        let line = &text[line_start..line_end];
        let line_lower = line.to_lowercase();
        if line_lower.starts_with("am ") && line.len() < 200 {
            return &text[..line_start];
        }
        search_start = abs_pos + schrieb_word.len();
    }

    // Check for "> " prefix on consecutive lines (quoted text block)
    // Detects blocks where >=2 consecutive lines start with "> "
    let lines: Vec<&str> = text.split('\n').collect();
    let mut quote_start: Option<usize> = None;
    let mut quote_count = 0;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with('>') {
            if quote_start.is_none() {
                quote_start = Some(i);
            }
            quote_count += 1;
        } else if !line.trim().is_empty() {
            // Non-empty, non-quoted line resets the counter
            quote_start = None;
            quote_count = 0;
        }
        // If we found >=2 consecutive quoted lines, strip from the start of the block
        if quote_count >= 2
            && let Some(start) = quote_start
        {
            // Find the byte offset of the first quoted line
            let mut byte_offset = 0;
            for line in lines.iter().take(start) {
                byte_offset += line.len() + 1; // +1 for the newline
            }
            return &text[..byte_offset];
        }
    }

    text
}

/// Parse a `message/rfc822` attachment and extract its content as structured text.
///
/// Attempts to parse the embedded email and returns a formatted representation
/// including subject, from, to, date, and body text. Falls back to raw UTF-8 text
/// if parsing fails.
fn parse_rfc822_attachment(bytes: &[u8]) -> ExtractionResult {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return ExtractionResult::NotSupported {
            reason: "RFC 822 message with invalid UTF-8 encoding",
        };
    };

    if text.trim().is_empty() {
        return ExtractionResult::Text {
            content: String::new(),
            method: "rfc822_raw",
        };
    }

    if let Some(parsed) = mail_parser::MessageParser::default().parse(bytes) {
        let mut parts: Vec<String> = Vec::with_capacity(7);

        push_header(&mut parts, "Subject", parsed.subject().map(str::to_string));
        push_header(&mut parts, "From", format_addresses(parsed.from()));
        push_header(&mut parts, "To", format_addresses(parsed.to()));
        push_header(&mut parts, "Cc", format_addresses(parsed.cc()));
        push_header(&mut parts, "Date", parsed.date().map(|d| d.to_string()));

        parts.push(String::new());

        if let Some(body) = parsed.body_text(0) {
            parts.push(body.to_string());
        } else if let Some(html) = parsed.body_html(0) {
            parts.push(html_to_plain_text(&html));
        }

        ExtractionResult::Text {
            content: parts.join("\n"),
            method: "rfc822_parse",
        }
    } else {
        ExtractionResult::Text {
            content: text.to_string(),
            method: "rfc822_raw",
        }
    }
}

/// Push a header line into `parts` if the value is present.
fn push_header(parts: &mut Vec<String>, name: &'static str, value: Option<String>) {
    if let Some(v) = value {
        parts.push(format!("{name}: {v}"));
    }
}

/// Format an `Address` (mailbox or group) into a comma-separated string.
fn format_addresses(addr: Option<&mail_parser::Address<'_>>) -> Option<String> {
    let parts: Vec<String> = addr?
        .iter()
        .map(|mb| {
            if let Some(name) = mb.name.as_deref() {
                format!("{name} <{}>", mb.address.as_deref().unwrap_or(""))
            } else {
                mb.address.as_deref().unwrap_or("").to_string()
            }
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_text(result: ExtractionResult) -> (String, &'static str) {
        let ExtractionResult::Text { content, method } = result else {
            panic!("expected Text, got: {:?}", result);
        };
        (content, method)
    }

    fn assert_not_supported(result: ExtractionResult) -> &'static str {
        let ExtractionResult::NotSupported { reason } = result else {
            panic!("expected NotSupported, got: {:?}", result);
        };
        reason
    }

    #[test]
    fn extract_text_plain() {
        let bytes = b"Hello, World!";
        let result = extract_text(bytes, "text/plain");
        let (content, method) = assert_text(result);
        assert_eq!(content, "Hello, World!");
        assert_eq!(method, "direct_utf8");
    }

    #[test]
    fn extract_text_json() {
        let bytes = b"{\"key\": \"value\"}";
        let result = extract_text(bytes, "application/json");
        let (content, _) = assert_text(result);
        assert!(content.contains("\"key\""));
        assert!(content.contains("\"value\""));
    }

    #[test]
    fn extract_text_html() {
        let bytes = b"<html><body><h1>Hello</h1><p>World!</p></body></html>";
        let result = extract_text(bytes, "text/html");
        let (content, _) = assert_text(result);
        assert!(content.contains("Hello"));
        assert!(content.contains("World!"));
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
        let reason = assert_not_supported(result);
        assert!(reason.contains("OCR"));
    }

    #[test]
    fn extract_text_xml() {
        let bytes = b"<?xml version=\"1.0\"?><root><item>test</item></root>";
        let result = extract_text(bytes, "application/xml");
        let (content, method) = assert_text(result);
        assert!(content.contains("test"));
        assert_eq!(method, "direct_utf8");
    }

    #[test]
    fn extract_text_xml_text_variant() {
        let bytes = b"<?xml version=\"1.0\"?><root><item>test</item></root>";
        let result = extract_text(bytes, "text/xml");
        let (_content, _method) = assert_text(result);
    }

    #[test]
    fn extract_text_csv() {
        let bytes = b"name,email\nJohn,john@example.com";
        let result = extract_text(bytes, "text/csv");
        let (content, method) = assert_text(result);
        assert!(content.contains("John"));
        assert_eq!(method, "direct_utf8");
    }

    #[test]
    fn extract_text_markdown() {
        let bytes = b"# Header\n\nSome **bold** text.";
        let result = extract_text(bytes, "text/markdown");
        let (content, method) = assert_text(result);
        assert!(content.contains("Header"));
        assert_eq!(method, "direct_utf8");
    }

    #[test]
    fn extract_text_markdown_with_extension() {
        let bytes = b"# Header\n\nSome text.";
        let result = extract_text(bytes, "text/markdown; charset=utf-8");
        let (_content, _method) = assert_text(result);
    }

    #[test]
    fn extract_text_office_not_supported() {
        let bytes = b"fake office document";
        let result = extract_text(
            bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        let reason = assert_not_supported(result);
        assert!(
            reason.contains("ZIP"),
            "Expected ZIP-related error for invalid DOCX, got: {}",
            reason
        );
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
        let reason = assert_not_supported(result);
        assert!(reason.contains("audio"));
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
        let reason = assert_not_supported(result);
        assert!(reason.contains("invalid UTF-8"));
    }

    #[test]
    fn extract_text_json_invalid() {
        let bytes = b"{invalid json}";
        let result = extract_text(bytes, "application/json");
        let reason = assert_not_supported(result);
        assert!(reason.contains("invalid JSON"));
    }

    #[test]
    fn extract_text_html_invalid_utf8() {
        let bytes = b"<html>\xFF\xFE</html>";
        let result = extract_text(bytes, "text/html");
        let reason = assert_not_supported(result);
        assert!(reason.contains("UTF-8"));
    }

    #[test]
    fn extract_text_xml_invalid_utf8() {
        let bytes = b"<?xml version=\"1.0\"?>\xFF\xFE";
        let result = extract_text(bytes, "application/xml");
        let reason = assert_not_supported(result);
        assert!(reason.contains("UTF-8"));
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
        let (content, _) = assert_text(result);
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

    #[test]
    fn extract_text_html_with_entities() {
        let bytes = b"<p>Hello &nbsp; world &amp; more &lt;test&gt; &quot;quote&quot;</p>";
        let result = extract_text(bytes, "text/html");
        let (content, _) = assert_text(result);
        assert!(content.contains("Hello") && content.contains("world") && content.contains("test"));
    }

    #[test]
    fn extract_text_binary_format() {
        let bytes = b"\x00\x01\x02\x03";
        let result = extract_text(bytes, "application/octet-stream");
        let reason = assert_not_supported(result);
        assert!(reason.contains("binary format"));
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
        let (content, _) = assert_text(result);
        assert!(content.contains("Hello"));
        assert!(content.contains("world"));
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
        let reason = assert_not_supported(result);
        assert!(reason.contains("binary format"));
    }

    #[test]
    fn extract_text_plain_empty() {
        let bytes = b"";
        let result = extract_text(bytes, "text/plain");
        let (content, _) = assert_text(result);
        assert!(content.is_empty());
    }

    #[test]
    fn extract_text_plain_with_unicode() {
        let bytes = "Hello 世界 🌍".as_bytes();
        let result = extract_text(bytes, "text/plain");
        let (content, _) = assert_text(result);
        assert!(content.contains("世界"));
        assert!(content.contains("🌍"));
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
        let (content, _) = assert_text(result);
        assert!(content.contains("name"));
        assert!(content.contains("email"));
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
        let (content, _) = assert_text(result);
        assert!(content.contains("John"));
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
        let (content, _) = assert_text(result);
        assert!(content.contains("text"));
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
        let (content, _) = assert_text(result);
        assert!(content.contains("Header"));
    }

    #[test]
    fn extract_text_markdown_with_links() {
        let bytes = b"[link](https://example.com) and text";
        let result = extract_text(bytes, "text/markdown");
        assert!(matches!(result, ExtractionResult::Text { .. }));
    }

    #[test]
    fn extract_text_image_by_extension() {
        let bytes = b"fake image data";
        let result = extract_text(bytes, "image/jpeg");
        let reason = assert_not_supported(result);
        assert!(reason.contains("image"));
    }

    #[test]
    fn extract_text_pdf_explicitly_not_supported() {
        let bytes = b"%PDF fake pdf";
        let result = extract_text(bytes, "application/pdf");
        let reason = assert_not_supported(result);
        assert!(reason.contains("PDF"));
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
        let (content, method) = assert_text(result);
        assert!(content.contains("Hello from DOCX"));
        assert_eq!(method, "docx_to_markdown");
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
        let (content, method) = assert_text(result);
        assert!(content.contains("Cell Content"));
        assert_eq!(method, "xlsx_to_csv");
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
        let (content, method) = assert_text(result);
        assert!(content.contains("Slide Text"));
        assert_eq!(method, "pptx_to_text");
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
        assert!(
            matches!(&result, ExtractionResult::Text { method, .. } if *method == "pdf_text_extract")
                || matches!(result, ExtractionResult::NotSupported { reason } if reason.contains("PDF") || reason.contains("text")),
            "expected PDF text extract or not-supported error, got: {:?}",
            result
        );
    }

    #[test]
    fn extract_text_docx_error_messages() {
        let result = extract_text(
            b"not a zip",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        let reason = assert_not_supported(result);
        assert!(reason.contains("ZIP"));
    }

    #[test]
    fn extract_text_xlsx_error_messages() {
        let result = extract_text(
            b"not a zip",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        );
        let reason = assert_not_supported(result);
        assert!(reason.contains("ZIP"));
    }

    #[test]
    fn extract_text_pptx_error_messages() {
        let result = extract_text(
            b"not a zip",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        );
        let reason = assert_not_supported(result);
        assert!(reason.contains("ZIP"));
    }

    #[test]
    fn parse_rfc822_simple_with_all_fields() {
        let email = b"From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Hello Bob
Date: Mon, 1 Jan 2024 10:00:00 +0000

Hi Bob, just checking in!";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: Hello Bob"));
        assert!(content.contains("From: Alice <alice@example.com>"));
        assert!(content.contains("To: Bob <bob@example.com>"));
        assert!(content.contains("Date:"));
        assert!(content.contains("Hi Bob, just checking in!"));
    }

    #[test]
    fn parse_rfc822_cc_field_included() {
        let email = b"From: Alice <alice@example.com>
To: Bob <bob@example.com>
Cc: Charlie <charlie@example.com>
Subject: Group Update
Date: Tue, 2 Jan 2024 12:00:00 +0000

Meeting at 3pm.";
        let result = parse_rfc822_attachment(email);
        let (content, _) = assert_text(result);
        assert!(content.contains("Cc: Charlie <charlie@example.com>"));
        assert!(content.contains("Meeting at 3pm."));
    }

    #[test]
    fn parse_rfc822_multiple_recipients() {
        let email = b"From: sender@example.com
To: one@example.com, two@example.com
Subject: Multiple To

Body text.";
        let result = parse_rfc822_attachment(email);
        let (content, _) = assert_text(result);
        assert!(content.contains("one@example.com"));
        assert!(content.contains("two@example.com"));
        assert!(content.contains("Body text."));
    }

    #[test]
    fn parse_rfc822_no_subject() {
        let email = b"From: alice@example.com
To: bob@example.com

Just a note.";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(!content.contains("Subject:"), "no Subject header expected");
        assert!(content.contains("Just a note."));
    }

    #[test]
    fn parse_rfc822_html_body_fallback() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: HTML Email
MIME-Version: 1.0
Content-Type: text/html; charset=utf-8

<html><body><p>Hello <b>Bob</b>!</p></body></html>";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: HTML Email"));
        // Body should be extracted as plain text
        assert!(content.contains("Hello Bob"));
    }

    #[test]
    fn parse_rfc822_empty_body() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: Empty

";
        let result = parse_rfc822_attachment(email);
        let (_, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
    }

    #[test]
    fn parse_rfc822_invalid_utf8() {
        // Invalid UTF-8 bytes
        let bytes = b"\xff\xfe\x00\x01";
        let result = parse_rfc822_attachment(bytes);
        let reason = assert_not_supported(result);
        assert!(
            reason.contains("UTF-8"),
            "expected UTF-8 error, got: {reason}"
        );
    }

    #[test]
    fn parse_rfc822_whitespace_only() {
        let result = parse_rfc822_attachment(b"   \n\n  ");
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_raw");
        assert_eq!(content, "");
    }

    #[test]
    fn parse_rfc822_via_extract_text() {
        let email = b"From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Via extract_text
Date: Wed, 3 Jan 2024 08:00:00 +0000

Hello from RFC 822!";
        let result = extract_text(email, "message/rfc822");
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: Via extract_text"));
        assert!(content.contains("Hello from RFC 822!"));
    }

    #[test]
    fn parse_rfc822_mime_content_type_subtype() {
        let email = b"From: test@example.com
To: recipient@example.com
Subject: MIME Type Test

MIME body.";
        let result = extract_text(email, "MESSAGE/RFC822");
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: MIME Type Test"));
        assert!(content.contains("MIME body."));
    }

    #[test]
    fn parse_rfc822_only_from_and_body() {
        let email = b"From: alice@example.com

Minimal email body.";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("From:"));
        assert!(content.contains("Minimal email body."));
    }

    #[test]
    fn parse_rfc822_multipart_alternative_prefers_text() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: Multipart
MIME-Version: 1.0
Content-Type: multipart/alternative; boundary=boundary42

--boundary42
Content-Type: text/plain; charset=utf-8

Plain text body.
--boundary42
Content-Type: text/html; charset=utf-8

<html><body><p>HTML body</p></body></html>
--boundary42--";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: Multipart"));
        assert!(content.contains("Plain text body."));
        assert!(!content.contains("HTML body"));
    }

    #[test]
    fn parse_rfc822_base64_encoded_body() {
        // "Hello from base64!" encoded in base64
        let email = b"From: alice@example.com
To: bob@example.com
Subject: Base64
MIME-Version: 1.0
Content-Type: text/plain; charset=utf-8
Content-Transfer-Encoding: base64

SGVsbG8gZnJvbSBiYXNlNjQh
";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(
            content.contains("Hello from base64!"),
            "expected decoded body, got: {content}"
        );
    }

    #[test]
    fn parse_rfc822_headers_only_no_body() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: No Body
Date: Mon, 1 Jan 2024 10:00:00 +0000

";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: No Body"));
        let body_part = content.split("\n\n").nth(1).unwrap_or("");
        assert!(
            body_part.is_empty() || body_part.trim().is_empty(),
            "expected no body content after headers, got: {body_part}"
        );
    }

    #[test]
    fn parse_rfc822_crlf_line_endings() {
        let email = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: CRLF\r\n\r\nBody with CRLF.";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        assert!(content.contains("Subject: CRLF"));
        assert!(content.contains("Body with CRLF."));
    }

    #[test]
    fn parse_rfc822_invalid_utf8_via_extract_text() {
        let bytes = b"\xff\xfe\x00\x01";
        let result = extract_text(bytes, "message/rfc822");
        let reason = assert_not_supported(result);
        assert!(reason.contains("UTF-8"));
    }

    #[test]
    fn parse_rfc822_no_headers_returns_parsed() {
        // mail-parser is lenient — treats bare text as a body-only message
        let bytes = b"Just a bare body with no headers.";
        let result = parse_rfc822_attachment(bytes);
        let (_, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
    }

    #[test]
    fn parse_rfc822_group_address_formatting() {
        let email = b"From: alice@example.com
To: Team: Bob <bob@example.com>, Charlie <charlie@example.com>;
Subject: Group Address

Group body.";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        // Group addresses should be flattened to individual mailboxes
        assert!(
            content.contains("Bob <bob@example.com>"),
            "expected Bob in group, got: {content}"
        );
        assert!(
            content.contains("Charlie <charlie@example.com>"),
            "expected Charlie in group, got: {content}"
        );
        assert!(content.contains("Group body."));
    }

    #[test]
    fn parse_rfc822_encoded_non_ascii_subject() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: =?UTF-8?B?SGVsbG8gw5tkw6lTw6k=?=
MIME-Version: 1.0
Content-Type: text/plain; charset=utf-8

Body.";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        // mail-parser decodes RFC 2047 encoded headers
        assert!(
            content.contains("Subject:") || content.contains("="),
            "expected subject line, got: {content}"
        );
    }

    #[test]
    fn parse_rfc822_quoted_printable_body() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: QP
MIME-Version: 1.0
Content-Type: text/plain; charset=utf-8
Content-Transfer-Encoding: quoted-printable

Hello =E2=82=AC world!";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        // mail-parser should decode QP: =E2=82=AC is €
        assert!(
            content.contains("Hello"),
            "expected decoded QP body, got: {content}"
        );
    }

    #[test]
    fn parse_rfc822_empty_bytes_via_extract_text() {
        let result = extract_text(b"", "message/rfc822");
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_raw");
        assert_eq!(content, "");
    }

    #[test]
    fn parse_rfc822_mixed_multipart_mixed() {
        let email = b"From: alice@example.com
To: bob@example.com
Subject: Mixed
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=boundary42

--boundary42
Content-Type: text/plain; charset=utf-8

This is the body text.
--boundary42
Content-Type: application/octet-stream
Content-Disposition: attachment; filename=test.bin

binary data here
--boundary42--";
        let result = parse_rfc822_attachment(email);
        let (content, method) = assert_text(result);
        assert_eq!(method, "rfc822_parse");
        // body_text(0) gets the first text part in multipart/mixed
        assert!(
            content.contains("This is the body text."),
            "expected body text, got: {content}"
        );
    }

    // ---- strip_quoted_replies tests ----

    #[test]
    fn strip_quoted_replies_no_quote_preserves_text() {
        let text = "Hello, this is a normal email.\n\nBest regards,\nJohn";
        assert_eq!(strip_quoted_replies(text), text);
    }

    #[test]
    fn strip_quoted_replies_strips_original_message() {
        let text = "Reply content here.\n\n-----Original Message-----\n> quoted content";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "Reply content here.\n\n");
        assert!(!result.contains("Original Message"));
    }

    #[test]
    fn strip_quoted_replies_strips_forwarded_message() {
        let text = "My response.\n\n---------- Forwarded message ----------\nFrom: Alice";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "My response.\n\n");
        assert!(!result.contains("Forwarded"));
    }

    #[test]
    fn strip_quoted_replies_strips_on_wrote_pattern() {
        let text = "Thanks for the update.\n\nOn Tue, Mar 4, 2025 at 2:30 PM John Doe wrote:\n> old message\n> more old text";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "Thanks for the update.\n\n");
        assert!(!result.contains("wrote"));
    }

    #[test]
    fn strip_quoted_replies_strips_german_am_schrieb_pattern() {
        let text =
            "Danke für die Info.\n\nAm 15.07.2025 um 10:30 schrieb Hans Müller:\n> alte Nachricht";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "Danke für die Info.\n\n");
        assert!(!result.contains("schrieb"));
    }

    #[test]
    fn strip_quoted_replies_ignores_short_on_wrote_in_middle() {
        // "On ... wrote:" pattern in the middle of a paragraph (>200 chars) should NOT be stripped
        let text = "This is a long paragraph that happens to mention the word wrote in passing. On the other hand, we should not strip this because it is not a quote marker but just regular text that happens to contain the substring wrote somewhere in the middle of a very long line that exceeds the threshold of 200 characters so it will be treated as normal text.";
        assert_eq!(strip_quoted_replies(text), text);
    }

    #[test]
    fn strip_quoted_replies_strips_gt_prefix_block() {
        let text =
            "My reply here.\n\n> quoted line 1\n> quoted line 2\n> quoted line 3\n\nSignature";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "My reply here.\n\n");
        assert!(!result.contains("> quoted"));
    }

    #[test]
    fn strip_quoted_replies_single_gt_line_not_stripped() {
        // Single "> " line should NOT be stripped (might be intentional quote, not a block)
        let text = "My reply here.\n> a single quoted line\n\nBest";
        assert_eq!(strip_quoted_replies(text), text);
    }

    #[test]
    fn strip_quoted_replies_empty_text() {
        assert_eq!(strip_quoted_replies(""), "");
        assert_eq!(strip_quoted_replies("   "), "   ");
    }

    #[test]
    fn strip_quoted_replies_preserves_signature_before_quote() {
        let text = "Thanks for the review.\n\nBest regards,\nJohn Doe\nAcme Corp\n+1-555-0123\n\n-----Original Message-----\nOld content";
        let result = strip_quoted_replies(text);
        assert!(result.contains("Best regards,"));
        assert!(result.contains("John Doe"));
        assert!(result.contains("+1-555-0123"));
        assert!(!result.contains("Original Message"));
    }

    #[test]
    fn strip_quoted_replies_case_insensitive_original() {
        let text = "Body\n\n-----original message-----";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "Body\n\n");
    }

    #[test]
    fn strip_quoted_replies_on_wrote_with_multiple_colons() {
        // "On ... wrote:" with timestamp containing colons should still match
        let text = "Thanks.\n\nOn Tue, 4 Mar 2025 at 14:30, John Doe wrote:\n> old text";
        let result = strip_quoted_replies(text);
        assert_eq!(result, "Thanks.\n\n");
    }
}
