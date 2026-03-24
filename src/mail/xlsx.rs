//! XLSX to CSV converter.
//!
//! Converts Microsoft Excel spreadsheets (.xlsx) to CSV format for LLM consumption.
//! XLSX files are ZIP archives containing XML. This module extracts the first worksheet
//! and converts it to CSV, handling shared strings and various cell types.

use std::io::{Cursor, Read};
use thiserror::Error;

/// Errors that can occur during XLSX processing.
#[derive(Debug, Error)]
pub enum XlsxError {
    #[error("Not a valid ZIP archive")]
    InvalidZip,
    #[error("Missing worksheet: {0}")]
    MissingWorksheet(String),
    #[error("XML parse error: {0}")]
    XmlParse(String),
    #[error("Shared strings error: {0}")]
    SharedStrings(String),
    #[error("UTF-8 decoding error")]
    Utf8Error,
    #[error("Empty worksheet")]
    EmptyWorksheet,
}

/// Convert XLSX bytes to CSV string.
///
/// # Arguments
///
/// * `bytes` - Raw XLSX file bytes
///
/// # Returns
///
/// CSV string on success, XlsxError on failure.
///
/// # Example
///
/// ```rust
/// use rusty_apple_mail_mcp::mail::xlsx::xlsx_to_csv;
///
/// // Assuming you have XLSX bytes
/// // let csv = xlsx_to_csv(&xlsx_bytes)?;
/// ```
pub fn xlsx_to_csv(bytes: &[u8]) -> Result<String, XlsxError> {
    // Unzip the archive
    let cursor = Cursor::new(bytes);
    let mut archive = zip::read::ZipArchive::new(cursor).map_err(|_| XlsxError::InvalidZip)?;

    // Read shared strings (if exists)
    let shared_strings = read_shared_strings(&mut archive)?;

    // Read first worksheet
    let csv = read_worksheet(&mut archive, "xl/worksheets/sheet1.xml", &shared_strings)?;

    if csv.trim().is_empty() {
        return Err(XlsxError::EmptyWorksheet);
    }

    Ok(csv)
}

/// Read shared strings from xl/sharedStrings.xml.
fn read_shared_strings(
    archive: &mut zip::read::ZipArchive<Cursor<&[u8]>>,
) -> Result<Vec<String>, XlsxError> {
    // Check if sharedStrings.xml exists
    if archive.by_name("xl/sharedStrings.xml").is_err() {
        return Ok(Vec::new());
    }

    let mut content = String::new();
    {
        let mut file = archive.by_name("xl/sharedStrings.xml").map_err(|e| {
            XlsxError::SharedStrings(format!("Failed to open sharedStrings.xml: {}", e))
        })?;
        file.read_to_string(&mut content)
            .map_err(|_| XlsxError::Utf8Error)?;
    }

    parse_shared_strings(&content)
}

/// Parse shared strings XML.
fn parse_shared_strings(xml: &str) -> Result<Vec<String>, XlsxError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);

    let mut strings = Vec::new();
    let mut in_si = false;
    let mut in_t = false;
    let mut current_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let binding = e.name();
                let name = binding.as_ref();
                let local_name = String::from_utf8_lossy(name);
                let local_name = local_name.split(':').next_back().unwrap_or(&local_name);

                match local_name {
                    "si" => {
                        in_si = true;
                        current_text.clear();
                    }
                    "t" => {
                        if in_si {
                            in_t = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if in_t {
                    current_text.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::End(e)) => {
                let binding = e.name();
                let name = binding.as_ref();
                let local_name = String::from_utf8_lossy(name);
                let local_name = local_name.split(':').next_back().unwrap_or(&local_name);

                match local_name {
                    "t" => {
                        in_t = false;
                    }
                    "si" => {
                        in_si = false;
                        strings.push(current_text.clone());
                        current_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(XlsxError::XmlParse(format!(
                    "Shared strings parse error: {}",
                    e
                )));
            }
            _ => {}
        }
    }

    Ok(strings)
}

/// Read and convert a worksheet to CSV.
fn read_worksheet(
    archive: &mut zip::read::ZipArchive<Cursor<&[u8]>>,
    sheet_path: &str,
    shared_strings: &[String],
) -> Result<String, XlsxError> {
    let mut content = String::new();
    {
        let mut file = archive
            .by_name(sheet_path)
            .map_err(|_| XlsxError::MissingWorksheet(sheet_path.to_string()))?;
        file.read_to_string(&mut content)
            .map_err(|_| XlsxError::Utf8Error)?;
    }

    parse_worksheet_to_csv(&content, shared_strings)
}

/// Parse worksheet XML and convert to CSV.
fn parse_worksheet_to_csv(xml: &str, shared_strings: &[String]) -> Result<String, XlsxError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);

    let mut csv = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let _in_row = false;
    let mut in_cell = false;
    let mut in_value = false;
    let mut cell_type: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let binding = e.name();
                let name = binding.as_ref();
                let local_name = String::from_utf8_lossy(name);
                let local_name = local_name.split(':').next_back().unwrap_or(&local_name);

                match local_name {
                    "row" => {
                        current_row.clear();
                    }
                    "c" => {
                        in_cell = true;
                        current_cell.clear();
                        cell_type = None;

                        // Check for cell type attribute
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref());
                            if key.ends_with("t") {
                                let value = String::from_utf8_lossy(&attr.value);
                                cell_type = Some(value.to_string());
                            }
                        }
                    }
                    "v" => {
                        if in_cell {
                            in_value = true;
                        }
                    }
                    "is" => {
                        // Inline string - treat as text
                        if in_cell {
                            cell_type = Some("str".to_string());
                        }
                    }
                    "t" => {
                        // Text within inline string
                        if in_cell {
                            in_value = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if in_value {
                    current_cell.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::End(e)) => {
                let binding = e.name();
                let name = binding.as_ref();
                let local_name = String::from_utf8_lossy(name);
                let local_name = local_name.split(':').next_back().unwrap_or(&local_name);

                match local_name {
                    "v" | "t" => {
                        in_value = false;
                    }
                    "c" => {
                        in_cell = false;
                        // Resolve cell value based on type
                        let resolved =
                            resolve_cell_value(&current_cell, cell_type.as_deref(), shared_strings);
                        current_row.push(resolved);
                    }
                    "row" => {
                        if !current_row.is_empty() {
                            csv.push(escape_csv_row(&current_row));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(XlsxError::XmlParse(format!("Worksheet parse error: {}", e)));
            }
            _ => {}
        }
    }

    Ok(csv.join("\n"))
}

/// Resolve cell value based on type.
fn resolve_cell_value(value: &str, cell_type: Option<&str>, shared_strings: &[String]) -> String {
    match cell_type {
        Some("s") => {
            // Shared string - lookup by index
            if let Ok(index) = value.parse::<usize>() {
                shared_strings.get(index).cloned().unwrap_or_default()
            } else {
                value.to_string()
            }
        }
        Some("b") => {
            // Boolean
            if value == "1" {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Some("str") => {
            // Inline string - already collected
            value.to_string()
        }
        _ => {
            // Numeric or default
            value.to_string()
        }
    }
}

/// Escape a CSV row following RFC 4180.
fn escape_csv_row(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| escape_csv_cell(cell))
        .collect::<Vec<_>>()
        .join(",")
}

/// Escape a single CSV cell.
fn escape_csv_cell(cell: &str) -> String {
    // Check if escaping is needed
    if cell.contains(',') || cell.contains('"') || cell.contains('\n') || cell.contains('\r') {
        // Escape double quotes by doubling them
        let escaped = cell.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        cell.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_minimal_xlsx() -> Vec<u8> {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            // [Content_Types].xml
            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
            )
            .unwrap();

            // _rels/.rels
            zip.start_file("_rels/.rels", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
            )
            .unwrap();

            // xl/_rels/workbook.xml.rels
            zip.start_file("xl/_rels/workbook.xml.rels", options)
                .unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#,
            )
            .unwrap();

            // xl/workbook.xml
            zip.start_file("xl/workbook.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>
  </sheets>
</workbook>"#,
            )
            .unwrap();

            // xl/worksheets/sheet1.xml
            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="s"><v>0</v></c>
      <c r="B1" t="s"><v>1</v></c>
    </row>
    <row r="2">
      <c r="A2"><v>100</v></c>
      <c r="B2" t="str"><v>Text</v></c>
    </row>
  </sheetData>
</worksheet>"#,
            )
            .unwrap();

            // xl/sharedStrings.xml
            zip.start_file("xl/sharedStrings.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>Name</t></si>
  <si><t>Value</t></si>
</sst>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        buf.into_inner()
    }

    #[test]
    fn test_xlsx_to_csv_basic() {
        let xlsx = create_minimal_xlsx();
        let result = xlsx_to_csv(&xlsx).unwrap();
        assert!(result.contains("Name,Value"));
        assert!(result.contains("100,Text"));
    }

    #[test]
    fn test_xlsx_invalid_zip() {
        let result = xlsx_to_csv(b"not a zip file");
        assert!(matches!(result, Err(XlsxError::InvalidZip)));
    }

    #[test]
    fn test_xlsx_missing_worksheet() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("other.txt", options).unwrap();
            zip.write_all(b"content").unwrap();
            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner());
        assert!(matches!(result, Err(XlsxError::MissingWorksheet(_))));
    }

    #[test]
    fn test_escape_csv_cell() {
        assert_eq!(escape_csv_cell("simple"), "simple");
        assert_eq!(escape_csv_cell("with,comma"), "\"with,comma\"");
        assert_eq!(escape_csv_cell("with\"quote"), "\"with\"\"quote\"");
        assert_eq!(escape_csv_cell("with\nnewline"), "\"with\nnewline\"");
    }

    #[test]
    fn test_escape_csv_row() {
        let cells = vec!["Name".to_string(), "Value".to_string()];
        assert_eq!(escape_csv_row(&cells), "Name,Value");

        let cells = vec!["Name".to_string(), "With, Comma".to_string()];
        assert_eq!(escape_csv_row(&cells), "Name,\"With, Comma\"");
    }

    #[test]
    fn test_resolve_cell_value() {
        let shared = vec!["Header1".to_string(), "Header2".to_string()];

        // Shared string
        assert_eq!(resolve_cell_value("0", Some("s"), &shared), "Header1");
        assert_eq!(resolve_cell_value("1", Some("s"), &shared), "Header2");

        // Inline string
        assert_eq!(resolve_cell_value("Direct", Some("str"), &shared), "Direct");

        // Numeric
        assert_eq!(resolve_cell_value("100", None, &shared), "100");

        // Boolean
        assert_eq!(resolve_cell_value("1", Some("b"), &[]), "TRUE");
        assert_eq!(resolve_cell_value("0", Some("b"), &[]), "FALSE");
    }

    #[test]
    fn test_parse_shared_strings() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>Header1</t></si>
  <si><t>Header2</t></si>
</sst>"#;

        let strings = parse_shared_strings(xml).unwrap();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0], "Header1");
        assert_eq!(strings[1], "Header2");
    }

    #[test]
    fn test_xlsx_without_shared_strings() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            // Minimal XLSX without shared strings (all inline)
            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
            )
            .unwrap();

            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="str"><v>Direct</v></c>
      <c r="B1"><v>123</v></c>
    </row>
  </sheetData>
</worksheet>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner()).unwrap();
        assert!(result.contains("Direct,123"));
    }

    #[test]
    fn test_xlsx_empty_worksheet() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:worksheet xmlns:w="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
</sheetData>
</w:worksheet>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner());
        assert!(matches!(result, Err(XlsxError::EmptyWorksheet)));
    }

    #[test]
    fn test_xlsx_xml_parse_error() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(b"<invalid xml without closing").unwrap();
            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner());
        assert!(matches!(result, Err(XlsxError::XmlParse(_))));
    }

    #[test]
    fn test_xlsx_with_boolean_cells() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1">
<c r="A1" t="b"><v>1</v></c>
<c r="B1" t="b"><v>0</v></c>
</row>
</sheetData>
</worksheet>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner()).unwrap();
        assert!(result.contains("TRUE,FALSE"));
    }

    #[test]
    fn test_xlsx_with_inline_string() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1">
<c r="A1"><is><t>Inline Text</t></is></c>
</row>
</sheetData>
</worksheet>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner()).unwrap();
        assert!(result.contains("Inline Text"));
    }

    #[test]
    fn test_resolve_cell_value_invalid_shared_string_index() {
        let shared: Vec<String> = vec![];
        // Invalid index - should return empty string (no shared string at that index)
        assert_eq!(resolve_cell_value("999", Some("s"), &shared), "");
        // Non-numeric value with 's' type - returns the value as-is since parsing fails
        assert_eq!(resolve_cell_value("abc", Some("s"), &shared), "abc");
    }

    #[test]
    fn test_escape_csv_cell_with_carriage_return() {
        assert_eq!(escape_csv_cell("with\rCR"), "\"with\rCR\"");
    }

    #[test]
    fn test_xlsx_error_display() {
        let err = XlsxError::InvalidZip;
        assert_eq!(format!("{}", err), "Not a valid ZIP archive");

        let err = XlsxError::MissingWorksheet("sheet1.xml".to_string());
        assert_eq!(format!("{}", err), "Missing worksheet: sheet1.xml");

        let err = XlsxError::XmlParse("test error".to_string());
        assert_eq!(format!("{}", err), "XML parse error: test error");

        let err = XlsxError::SharedStrings("test".to_string());
        assert_eq!(format!("{}", err), "Shared strings error: test");

        let err = XlsxError::Utf8Error;
        assert_eq!(format!("{}", err), "UTF-8 decoding error");

        let err = XlsxError::EmptyWorksheet;
        assert_eq!(format!("{}", err), "Empty worksheet");
    }
}
