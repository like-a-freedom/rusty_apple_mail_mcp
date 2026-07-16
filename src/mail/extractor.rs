//! Extract text content from attachments based on MIME type.
//!
//! This module provides functions to extract LLM-readable text from various
//! attachment formats.

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

#[cfg(test)]
mod tests {
    use super::*;

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
