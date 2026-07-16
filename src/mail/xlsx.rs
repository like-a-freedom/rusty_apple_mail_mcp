//! XLSX to CSV converter.
//!
//! Converts Microsoft Excel spreadsheets (.xlsx) to CSV format for LLM consumption.
//! XLSX files are ZIP archives containing XML. This module extracts the first worksheet
//! and converts it to CSV, handling shared strings and various cell types.

use std::io::{Cursor, Read};

use crate::mail::extract::ExtractionError;

/// Convert XLSX bytes to CSV string.
///
/// # Arguments
///
/// * `bytes` - Raw XLSX file bytes
///
/// # Returns
///
/// CSV string on success, `ExtractionError` on failure.
///
/// # Example
///
/// ```rust
/// use rusty_apple_mail_mcp::mail::xlsx::xlsx_to_csv;
///
/// // Assuming you have XLSX bytes
/// // let csv = xlsx_to_csv(&xlsx_bytes)?;
/// ```
///
/// # Errors
///
/// Returns [`ExtractionError`] if the XLSX cannot be parsed or has no worksheets.
pub fn xlsx_to_csv(bytes: &[u8]) -> Result<String, ExtractionError> {
    // Unzip the archive
    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::read::ZipArchive::new(cursor).map_err(|_| ExtractionError::InvalidZip)?;

    // Read shared strings (if exists)
    let shared_strings = read_shared_strings(&mut archive)?;

    // Read first worksheet
    let csv = read_worksheet(&mut archive, "xl/worksheets/sheet1.xml", &shared_strings)?;

    if csv.trim().is_empty() {
        return Err(ExtractionError::EmptyDocument);
    }

    Ok(csv)
}

/// Read shared strings from xl/sharedStrings.xml.
fn read_shared_strings(
    archive: &mut zip::read::ZipArchive<Cursor<&[u8]>>,
) -> Result<Vec<String>, ExtractionError> {
    // Check if sharedStrings.xml exists
    if archive.by_name("xl/sharedStrings.xml").is_err() {
        return Ok(Vec::new());
    }

    let mut content = String::new();
    {
        let mut file = archive.by_name("xl/sharedStrings.xml").map_err(|e| {
            ExtractionError::Other(format!("Failed to open sharedStrings.xml: {e}"))
        })?;
        file.read_to_string(&mut content)
            .map_err(|_| ExtractionError::Utf8Error)?;
    }

    parse_shared_strings(&content)
}

/// Parse shared strings XML.
fn parse_shared_strings(xml: &str) -> Result<Vec<String>, ExtractionError> {
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
                    "t" if in_si => {
                        in_t = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_t => {
                current_text.push_str(&String::from_utf8_lossy(e.as_ref()));
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
                return Err(ExtractionError::XmlParse(format!(
                    "Shared strings parse error: {e}"
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
) -> Result<String, ExtractionError> {
    let mut content = String::new();
    {
        let mut file = archive.by_name(sheet_path).map_err(|_| {
            ExtractionError::InvalidFormat(format!("missing worksheet: {sheet_path}"))
        })?;
        file.read_to_string(&mut content)
            .map_err(|_| ExtractionError::Utf8Error)?;
    }

    parse_worksheet_to_csv(&content, shared_strings)
}

/// Parse worksheet XML and convert to CSV.
fn parse_worksheet_to_csv(xml: &str, shared_strings: &[String]) -> Result<String, ExtractionError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);

    let mut csv = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
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
                            if key.ends_with('t') {
                                let value = String::from_utf8_lossy(&attr.value);
                                cell_type = Some(value.to_string());
                            }
                        }
                    }
                    "v" if in_cell => {
                        in_value = true;
                    }
                    "is" if in_cell => {
                        // Inline string - treat as text
                        cell_type = Some("str".to_string());
                    }
                    "t" if in_cell => {
                        // Text within inline string
                        in_value = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_value => {
                current_cell.push_str(&String::from_utf8_lossy(e.as_ref()));
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
                    "row" if !current_row.is_empty() => {
                        csv.push(escape_csv_row(&current_row));
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ExtractionError::XmlParse(format!(
                    "Worksheet parse error: {e}"
                )));
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
        format!("\"{escaped}\"")
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
        assert!(matches!(result, Err(ExtractionError::InvalidZip)));
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
        assert!(matches!(result, Err(ExtractionError::InvalidFormat(_))));
    }

    #[test]
    fn test_xlsx_csv_escaping_through_public_api() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

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
<c r="A1" t="str"><v>simple</v></c>
<c r="B1" t="str"><v>with,comma</v></c>
<c r="C1" t="str"><v>with"quote</v></c>
<c r="D1"><v>123</v></c>
<c r="E1" t="b"><v>1</v></c>
</row>
</sheetData>
</worksheet>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner()).unwrap();
        assert!(result.contains("simple"));
        assert!(result.contains("\"with,comma\""));
        assert!(result.contains("\"with\"\"quote\""));
        assert!(result.contains("123"));
        assert!(result.contains("TRUE"));
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
        assert!(matches!(result, Err(ExtractionError::EmptyDocument)));
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
        assert!(matches!(result, Err(ExtractionError::XmlParse(_))));
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
    fn test_xlsx_with_shared_strings() {
        use std::io::Write;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::write::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
            )
            .unwrap();

            zip.start_file("xl/sharedStrings.xml", options).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">
  <si><t>Name</t></si>
  <si><t>Value</t></si>
  <si><t>Item, with comma</t></si>
</sst>"#,
            )
            .unwrap();

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
<c r="A2" t="s"><v>2</v></c>
</row>
</sheetData>
</worksheet>"#,
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let result = xlsx_to_csv(&buf.into_inner()).unwrap();
        assert!(result.contains("Name,Value"));
        assert!(result.contains("\"Item, with comma\""));
    }
}
