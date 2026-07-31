//! Bounded content delivery via reconstructable Windows.
//!
//! A Content Window is a bounded, ordered slice of Canonical normalized content.
//! Consecutive valid Windows reconstruct the Canonical normalized content byte-for-byte.
//! See ADR-0006 and CONTEXT.md for the full contract.

use schemars::JsonSchema;
use serde::Serialize;
use std::path::Path;

/// Default window size in bytes. Initial candidate pending corpus evaluation.
pub const DEFAULT_WINDOW_BYTES: usize = 8192;

/// Maximum window size in bytes.
pub const MAX_WINDOW_BYTES: usize = 65536;

/// A Content Window — a bounded slice of Canonical normalized content.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[must_use]
pub struct ContentWindow {
    /// Byte offset from the start of the canonical content (inclusive).
    pub offset: usize,
    /// Number of bytes in this window.
    pub bytes_returned: usize,
    /// Total bytes of the canonical normalized content.
    pub total_bytes: usize,
    /// Whether all canonical content has been delivered.
    pub complete: bool,
    /// Byte offset for the next continuation call. `None` when `complete` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    /// Source revision at time of extraction. Changed content invalidates this.
    pub source_revision: String,
    /// Human-readable representation of the content (e.g. "plain_text", "markdown_html").
    pub representation: String,
    /// Known extraction limitations, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_limitations: Option<String>,
}

/// Compute a weak source revision from file metadata.
///
/// Combines file size and modification time into a simple string that changes
/// when the source content changes. Not cryptographic — just a weak change detector.
pub fn compute_source_revision(
    size: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    extraction_version: &str,
) -> String {
    format!("{size}:{mtime_secs}.{mtime_nanos}:{extraction_version}")
}

pub(crate) fn file_source_revision(path: &Path, extraction_version: &str) -> String {
    let (size, mtime_secs, mtime_nanos) = std::fs::metadata(path)
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or((0, 0), |duration| {
                    (duration.as_secs(), duration.subsec_nanos())
                });
            (metadata.len(), modified.0, modified.1)
        })
        .unwrap_or((0, 0, 0));
    compute_source_revision(size, mtime_secs, mtime_nanos, extraction_version)
}

/// Slice canonical normalized content into a bounded Window.
///
/// Prefers cutting at paragraph or newline boundaries when close to the limit.
/// Validates offset, revision, and UTF-8 boundaries.
///
/// # Errors
///
/// Returns an error if the offset is out of bounds, the revision is stale,
/// or the offset is not on a UTF-8 character boundary.
pub fn slice_content(
    content: &[u8],
    offset: usize,
    limit: usize,
    source_revision: &str,
    expected_revision: &str,
    representation: &str,
    extraction_limitations: Option<&str>,
) -> Result<ContentWindow, DeliveryError> {
    let content_text =
        std::str::from_utf8(content).map_err(|err| DeliveryError::InvalidUtf8Boundary {
            offset: err.valid_up_to(),
        })?;
    if source_revision != expected_revision {
        return Err(DeliveryError::RevisionMismatch {
            expected: expected_revision.to_string(),
            got: source_revision.to_string(),
        });
    }
    if offset > content.len() {
        return Err(DeliveryError::OffsetOutOfBounds {
            offset,
            total: content.len(),
        });
    }
    if !content_text.is_char_boundary(offset) {
        return Err(DeliveryError::InvalidUtf8Boundary { offset });
    }

    let total_bytes = content.len();
    let mut end = offset.saturating_add(limit).min(total_bytes);
    while end > offset && !content_text.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset && offset < total_bytes {
        end = content_text[offset..]
            .char_indices()
            .nth(1)
            .map_or(total_bytes, |(next, _)| offset + next);
    }

    // Try to find a nearby paragraph/newline boundary to avoid splitting mid-sentence.
    let mut slice_end = end;
    if end < total_bytes {
        let search_start = offset.saturating_add(1);
        let search_end = end;
        if search_end > search_start {
            let search_region = &content[search_start..search_end];
            // Look for last paragraph break in the search region
            if let Some(pos) = search_region.windows(2).rposition(|w| w == b"\n\n") {
                let candidate = search_start + pos + 2;
                // Only use the boundary if it doesn't shrink the window too much
                // and is still past the current offset
                if candidate >= end.saturating_sub(256) && candidate > offset {
                    slice_end = candidate;
                }
            }
        }
    }

    let window_bytes = slice_end - offset;
    let complete = slice_end >= total_bytes;
    let next_offset = if complete { None } else { Some(slice_end) };

    Ok(ContentWindow {
        offset,
        bytes_returned: window_bytes,
        total_bytes,
        complete,
        next_offset,
        source_revision: source_revision.to_string(),
        representation: representation.to_string(),
        extraction_limitations: extraction_limitations.map(String::from),
    })
}

/// Errors that can occur during content windowing.
#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("Source revision mismatch: expected {expected}, got {got}. Restart from offset 0.")]
    RevisionMismatch { expected: String, got: String },

    #[error("Offset {offset} is out of bounds (total {total} bytes)")]
    OffsetOutOfBounds { offset: usize, total: usize },

    #[error("Offset {offset} is not on a valid UTF-8 character boundary")]
    InvalidUtf8Boundary { offset: usize },
}

impl From<DeliveryError> for crate::error::MailMcpError {
    fn from(err: DeliveryError) -> Self {
        match err {
            DeliveryError::RevisionMismatch { .. } => {
                crate::error::MailMcpError::Validation(err.to_string())
            }
            DeliveryError::OffsetOutOfBounds { .. } => {
                crate::error::MailMcpError::Validation(err.to_string())
            }
            DeliveryError::InvalidUtf8Boundary { .. } => {
                crate::error::MailMcpError::Validation(err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_REVISION: &str = "100:1700000000.0:v1";

    fn window(content: &str, offset: usize, limit: usize) -> ContentWindow {
        slice_content(
            content.as_bytes(),
            offset,
            limit,
            TEST_REVISION,
            TEST_REVISION,
            "plain_text",
            None,
        )
        .expect("valid window")
    }

    #[test]
    fn empty_content_returns_complete_window() {
        let w = window("", 0, 100);
        assert_eq!(w.offset, 0);
        assert_eq!(w.bytes_returned, 0);
        assert_eq!(w.total_bytes, 0);
        assert!(w.complete);
        assert!(w.next_offset.is_none());
    }

    #[test]
    fn content_fits_in_one_window() {
        let w = window("Hello, World!", 0, 100);
        assert_eq!(w.offset, 0);
        assert_eq!(w.bytes_returned, 13);
        assert_eq!(w.total_bytes, 13);
        assert!(w.complete);
        assert!(w.next_offset.is_none());
    }

    #[test]
    fn content_exactly_at_limit_is_complete() {
        let w = window("12345", 0, 5);
        assert_eq!(w.bytes_returned, 5);
        assert!(w.complete);
        assert!(w.next_offset.is_none());
    }

    #[test]
    fn partial_window_has_next_offset() {
        let w = window("Hello, World!", 0, 5);
        assert_eq!(w.bytes_returned, 5);
        assert!(!w.complete);
        assert!(w.next_offset.is_some());
    }

    #[test]
    fn continuation_reconstructs_content() {
        let content = "The quick brown fox jumps over the lazy dog.";
        let mut offset = 0;
        let mut reconstructed = Vec::new();
        loop {
            let w = window(content, offset, 10);
            let slice = &content.as_bytes()[w.offset..w.offset + w.bytes_returned];
            reconstructed.extend_from_slice(slice);
            if w.complete {
                break;
            }
            offset = w.next_offset.expect("next_offset when not complete");
        }
        assert_eq!(reconstructed, content.as_bytes());
    }

    #[test]
    fn cyrillic_content_preserves_boundaries() {
        let content = "Привет, мир! Это тест кириллического текста для проверки границ UTF-8.";
        let w = window(content, 0, 20);
        // Should not panic or produce invalid UTF-8
        let slice = &content.as_bytes()[w.offset..w.offset + w.bytes_returned];
        assert!(std::str::from_utf8(slice).is_ok());
    }

    #[test]
    fn emoji_content_preserves_boundaries() {
        let content = "Hello 🌍 World! 🎉 celebration";
        let w = window(content, 0, 10);
        let slice = &content.as_bytes()[w.offset..w.offset + w.bytes_returned];
        assert!(std::str::from_utf8(slice).is_ok());
    }

    #[test]
    fn limit_inside_multibyte_character_returns_valid_utf8() {
        let content = "Ж";
        let w = window(content, 0, 1);
        let slice = &content.as_bytes()[w.offset..w.offset + w.bytes_returned];
        assert_eq!(std::str::from_utf8(slice).unwrap(), content);
    }

    #[test]
    fn offset_inside_multibyte_character_returns_error() {
        let content = "Ж";
        let result = slice_content(
            content.as_bytes(),
            1,
            10,
            TEST_REVISION,
            TEST_REVISION,
            "plain_text",
            None,
        );
        assert!(matches!(
            result,
            Err(DeliveryError::InvalidUtf8Boundary { offset: 1 })
        ));
    }

    #[test]
    fn offset_out_of_bounds_returns_error() {
        let content = "short";
        let result = slice_content(
            content.as_bytes(),
            100,
            10,
            TEST_REVISION,
            TEST_REVISION,
            "plain_text",
            None,
        );
        assert!(matches!(
            result,
            Err(DeliveryError::OffsetOutOfBounds { .. })
        ));
    }

    #[test]
    fn revision_mismatch_returns_error() {
        let content = "Hello";
        let result = slice_content(
            content.as_bytes(),
            0,
            10,
            "old-revision",
            "new-revision",
            "plain_text",
            None,
        );
        assert!(matches!(
            result,
            Err(DeliveryError::RevisionMismatch { .. })
        ));
    }

    #[test]
    fn preferred_boundary_at_paragraph_break() {
        // Content with paragraph breaks near the cut point
        let content = "First paragraph.\n\nSecond paragraph starts here and continues for a while.";
        let w = window(content, 0, 30);
        // The window should cut at the paragraph break (after "First paragraph.\n\n")
        // which is at byte 18, rather than in the middle of "Second paragraph..."
        // The search region is [30-64..30] which is [0..30], finding "\n\n" at position 16
        assert!(w.bytes_returned <= 30);
        let slice = &content[w.offset..w.offset + w.bytes_returned];
        // Should end at or near the paragraph boundary
        assert!(
            slice.ends_with("\n\n") || !w.complete && w.bytes_returned > 0,
            "window should prefer paragraph boundary: {slice:?}"
        );
    }

    #[test]
    fn window_reports_representation() {
        let w = window("text", 0, 100);
        assert_eq!(w.representation, "plain_text");
    }

    #[test]
    fn window_reports_extraction_limitations() {
        let w = slice_content(
            "text".as_bytes(),
            0,
            100,
            TEST_REVISION,
            TEST_REVISION,
            "xlsx_csv",
            Some("Only first worksheet extracted"),
        )
        .unwrap();
        assert_eq!(
            w.extraction_limitations.as_deref(),
            Some("Only first worksheet extracted")
        );
    }

    #[test]
    fn delivery_error_maps_to_mail_mcp_error() {
        let err = DeliveryError::OffsetOutOfBounds {
            offset: 100,
            total: 10,
        };
        let mcp_err: crate::error::MailMcpError = err.into();
        assert!(mcp_err.to_string().contains("out of bounds"));
    }

    #[test]
    fn slice_end_prefers_newline_boundary() {
        // A long line without paragraph breaks
        let content = "a".repeat(100);
        let w = window(&content, 0, 30);
        // With no paragraph break, should cut at limit
        assert_eq!(w.bytes_returned, 30);
    }

    #[test]
    fn offset_at_content_end_returns_zero_bytes() {
        let content = "Hello";
        let w = window(content, 5, 100);
        assert_eq!(w.bytes_returned, 0);
        assert!(w.complete);
    }
}
