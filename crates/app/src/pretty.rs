//! Content-type detection and JSON pretty-printing.

use wisp_core::Header;

pub const PRETTY_LIMIT: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Json,
    Xml,
    Html,
    Text,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrettyError {
    pub offset: usize,
    pub message: String,
}

pub fn content_kind(_headers: &[Header], _sniff: &[u8]) -> ContentKind {
    ContentKind::Text
}

pub fn pretty_json(input: &[u8]) -> Result<String, PrettyError> {
    Ok(String::from_utf8_lossy(input).into_owned())
}
