//! TextExtractor trait and unified extraction error.

use std::fmt;

/// Errors that can occur during attachment text extraction.
#[derive(Debug)]
pub enum ExtractionError {
    InvalidZip,
    InvalidFormat(String),
    XmlParse(String),
    NoTextLayer,
    EmptyDocument,
    Utf8Error,
    UnsupportedMime(String),
    Other(String),
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidZip => write!(f, "Not a valid ZIP archive"),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {msg}"),
            Self::XmlParse(msg) => write!(f, "XML parse error: {msg}"),
            Self::NoTextLayer => write!(f, "No extractable text layer (possibly scanned)"),
            Self::EmptyDocument => write!(f, "Empty document"),
            Self::Utf8Error => write!(f, "UTF-8 decoding error"),
            Self::UnsupportedMime(mime) => write!(f, "Unsupported MIME type: {mime}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ExtractionError {}

/// Text extraction strategy for a specific attachment format.
pub trait TextExtractor: Send + Sync {
    /// Extract text from raw attachment bytes.
    fn extract(&self, bytes: &[u8]) -> Result<String, ExtractionError>;
}
