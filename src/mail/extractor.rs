//! Extract text content from attachments based on MIME type.
//!
//! This module provides functions to extract LLM-readable text from various
//! attachment formats. The goal is to provide meaningful text content when
//! possible, and clear guidance when extraction is not supported.

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
/// ExtractionResult with either extracted text or a reason why extraction is not supported.
pub fn extract_text(bytes: &[u8], mime_type: &str) -> ExtractionResult {
    let mime_lower = mime_type.to_lowercase();

    // Text formats - return as-is
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
    if mime_lower == "text/markdown" || mime_lower.ends_with(".md") {
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
    if mime_lower == "text/html" || mime_lower.ends_with(".html") {
        return extract_text_from_html(bytes);
    }

    // PDF - not supported in v1.0 (requires PDF parsing library)
    if mime_lower == "application/pdf" {
        return ExtractionResult::NotSupported {
            reason: "PDF text extraction requires external library, not in v1.0 scope",
        };
    }

    // Office documents - not supported in v1.0
    if mime_lower.contains("wordprocessingml")
        || mime_lower.contains("spreadsheetml")
        || mime_lower.contains("presentationml")
        || mime_lower == "application/msword"
        || mime_lower == "application/vnd.ms-excel"
        || mime_lower == "application/vnd.ms-powerpoint"
    {
        return ExtractionResult::NotSupported {
            reason: "Office document text extraction not in v1.0 scope",
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

    // Default: binary format not supported
    ExtractionResult::NotSupported {
        reason: "binary format text extraction not supported",
    }
}

/// Extract text content from HTML bytes.
fn extract_text_from_html(bytes: &[u8]) -> ExtractionResult {
    let html = match String::from_utf8(bytes.to_vec()) {
        Ok(h) => h,
        Err(_) => {
            return ExtractionResult::NotSupported {
                reason: "HTML with invalid UTF-8 encoding",
            };
        }
    };

    // Simple HTML to text conversion
    // Strip tags and decode common HTML entities
    let text = strip_html_tags(&html);

    ExtractionResult::Text {
        content: text,
        method: "html_tag_stripping",
    }
}

/// Strip HTML tags from a string and decode common entities.
fn strip_html_tags(html: &str) -> String {
    // Remove script and style elements
    let mut result = html.to_string();
    result = regex_replace(&result, r"(?s)<script[^>]*>.*?</script>", "");
    result = regex_replace(&result, r"(?s)<style[^>]*>.*?</style>", "");

    // Remove all other tags
    result = regex_replace(&result, r"<[^>]*>", "");

    // Decode common HTML entities
    result = result.replace("&nbsp;", " ");
    result = result.replace("&amp;", "&");
    result = result.replace("&lt;", "<");
    result = result.replace("&gt;", ">");
    result = result.replace("&quot;", "\"");
    result = result.replace("&#39;", "'");
    result = result.replace("&apos;", "'");

    // Normalize whitespace
    result
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Simple regex-like replacement (handles basic patterns).
fn regex_replace(text: &str, pattern: &str, replacement: &str) -> String {
    // This is a very simplified replacement - for production use,
    // consider adding the `regex` crate
    if pattern.contains("(?s)") {
        // Multiline mode - handle .*? matching
        let pattern = pattern.replace("(?s)", "");
        if pattern.contains(".*?") {
            // Non-greedy match - simple implementation
            let parts: Vec<&str> = pattern.split(".*?").collect();
            if parts.len() == 2 {
                return remove_between(text, parts[0], parts[1], replacement);
            }
        }
    }

    if pattern.contains("<[^>]*>") {
        // Tag removal - character by character
        let mut result = String::new();
        let mut in_tag = false;
        for c in text.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag {
                result.push(c);
            }
        }
        return result;
    }

    text.to_string()
}

/// Remove content between start and end markers.
fn remove_between(text: &str, start: &str, end: &str, replacement: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;

    while let Some(start_pos) = remaining.find(start) {
        result.push_str(&remaining[..start_pos + start.len()]);
        remaining = &remaining[start_pos + start.len()..];

        if let Some(end_pos) = remaining.find(end) {
            remaining = &remaining[end_pos + end.len()..];
        } else {
            break;
        }
    }

    result.push_str(remaining);
    result.replace(&format!("{start}{end}"), replacement)
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
}
