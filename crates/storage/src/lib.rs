//! Wisp storage: on-disk collections and settings.

pub mod codec;
pub mod collection;
pub mod environment;
pub mod error;
pub mod fs;
pub mod history;
pub mod request;
pub mod watcher;

pub use error::StorageError;

/// Prefix of atomic-write temp files, which the watcher ignores.
pub const TEMP_PREFIX: &str = ".wisp-tmp-";
/// Name of the per-collection state (history) directory.
pub const STATE_DIR: &str = ".wisp";
