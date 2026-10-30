//! Storage error type.

use std::path::PathBuf;

/// A failure reading or writing collection data.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("i/o error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// TOML syntax error, including the line.
    #[error("parse error in {}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    /// Wrong type, unknown method or auth type.
    #[error("invalid `{key}` in {}: {message}", path.display())]
    Invalid {
        path: PathBuf,
        key: String,
        message: String,
    },
    /// An operation would overwrite something that already exists.
    #[error("already exists: {}", path.display())]
    Exists { path: PathBuf },
    #[error("unsupported format {found} in {}", path.display())]
    UnsupportedFormat { path: PathBuf, found: i64 },
    #[error("not a collection: {}", path.display())]
    NotACollection { path: PathBuf },
    #[error("watch error: {message}")]
    Watch { message: String },
    #[error("history error: {message}")]
    History { message: String },
}
