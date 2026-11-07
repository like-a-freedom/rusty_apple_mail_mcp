//! Bounded reads for entries inside attachment archive formats.
//!
//! Attachment archives (.docx/.xlsx/.pptx) are attacker-controlled content:
//! a crafted ZIP can declare a tiny compressed size while decompressing to
//! gigabytes. Every entry read must go through [`read_zip_entry_to_string`]
//! so a decompression bomb cannot exhaust server memory.

use std::io::Read;

use super::traits::ExtractionError;
use super::traits::MAX_DECOMPRESSED_BYTES;

/// Read a UTF-8 entry from a ZIP archive with a hard cap on decompressed size.
///
/// # Errors
///
/// Returns [`ExtractionError::InvalidFormat`] when the entry is missing,
/// [`ExtractionError::Utf8Error`] when the entry is not valid UTF-8, and
/// [`ExtractionError::Other`] when the decompressed size exceeds `max_bytes`.
pub(crate) fn read_zip_entry_to_string<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    entry_path: &str,
    max_bytes: u64,
) -> Result<String, ExtractionError> {
    let file = archive.by_name(entry_path).map_err(|_| {
        ExtractionError::InvalidFormat(format!("missing file in archive: {entry_path}"))
    })?;
    let mut content = String::new();
    // Read one byte beyond the cap so oversized entries are detected reliably.
    let mut limited = file.take(max_bytes.saturating_add(1));
    limited
        .read_to_string(&mut content)
        .map_err(|_| ExtractionError::Utf8Error)?;
    if content.len() as u64 > max_bytes {
        return Err(ExtractionError::Other(format!(
            "decompressed size of \"{entry_path}\" exceeds the {max_bytes}-byte extraction limit"
        )));
    }
    Ok(content)
}

/// [`read_zip_entry_to_string`] with the default [`MAX_DECOMPRESSED_BYTES`] cap.
pub(crate) fn read_zip_entry_bounded<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    entry_path: &str,
) -> Result<String, ExtractionError> {
    read_zip_entry_to_string(archive, entry_path, MAX_DECOMPRESSED_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::Write;

    fn zip_with_entry(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options: zip::write::SimpleFileOptions = Default::default();
        zip.start_file(name, options).expect("start file");
        zip.write_all(contents).expect("write entry");
        zip.finish().expect("finish zip").into_inner()
    }

    #[test]
    fn read_zip_entry_to_string_returns_entry_within_limit() {
        let bytes = zip_with_entry("a.xml", b"<xml/>");
        let mut archive = zip::read::ZipArchive::new(Cursor::new(bytes)).expect("archive");
        let content = read_zip_entry_to_string(&mut archive, "a.xml", 16).expect("entry");
        assert_eq!(content, "<xml/>");
    }

    #[test]
    fn read_zip_entry_to_string_rejects_entry_larger_than_limit() {
        let big = "A".repeat(1024);
        let bytes = zip_with_entry("bomb.xml", big.as_bytes());
        let mut archive = zip::read::ZipArchive::new(Cursor::new(bytes)).expect("archive");
        let err = read_zip_entry_to_string(&mut archive, "bomb.xml", 16).expect_err("too large");
        assert!(
            matches!(err, ExtractionError::Other(ref msg) if msg.contains("limit")),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn read_zip_entry_to_string_rejects_missing_entry() {
        let bytes = zip_with_entry("a.xml", b"<xml/>");
        let mut archive = zip::read::ZipArchive::new(Cursor::new(bytes)).expect("archive");
        let err = read_zip_entry_to_string(&mut archive, "missing.xml", 16).expect_err("missing");
        assert!(matches!(err, ExtractionError::InvalidFormat(_)));
    }

    #[test]
    fn read_zip_entry_bounded_uses_default_limit() {
        let bytes = zip_with_entry("small.xml", b"<xml/>");
        let mut archive = zip::read::ZipArchive::new(Cursor::new(bytes)).expect("archive");
        let content = read_zip_entry_bounded(&mut archive, "small.xml").expect("entry");
        assert_eq!(content, "<xml/>");
    }
}
