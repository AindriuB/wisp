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

/// Why a secret store reports itself unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UnavailableReason {
    /// This build has no store for the platform.
    #[error("no secret store on this platform")]
    Portable,
    /// The store exists but is not set up or configured.
    #[error("secret store not configured")]
    NotConfigured,
    /// The store is not running or cannot be reached.
    #[error("secret store service not running")]
    NoService,
    /// The store did not answer in time.
    #[error("secret store timed out")]
    TimedOut,
}

/// A failure from a [`crate::vault::SecretStore`]. Messages are fixed
/// strings and non-secret codes; they never carry a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SecretStoreError {
    #[error("secret store unavailable: {reason}")]
    Unavailable { reason: UnavailableReason },
    /// The store needs the user to unlock it.
    #[error("secret store is locked")]
    Locked,
    /// The user refused, or the pairing was revoked.
    #[error("secret store access denied")]
    Denied,
    /// The value is over the store's size cap; nothing was stored.
    #[error("secret value too large for the store")]
    TooLarge,
    #[error("secret store error{}", code.map(|c| format!(" (code {c})")).unwrap_or_default())]
    Other { code: Option<i64> },
}
