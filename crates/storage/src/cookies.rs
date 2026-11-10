//! Persisted cookie file for one collection.
//!
//! Lives in the per-user data dir at `cookies/<collection-id>.json` as
//! `{"version":1,"cookies":[...]}`. Persistence is on exactly when the file exists (an empty
//! list counts) and off otherwise. Session cookies are never stored: `expires` is required,
//! so the caller leaves them out. Cookie values are secrets and are never logged.

use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::StorageError;
use crate::collection::CollectionId;
use crate::data_dir::{DataDir, ensure_private_dir};
use crate::fs::atomic_write_force_private;

/// Largest cookie file `load` will read.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Cookies kept per file.
pub const MAX_COOKIES: usize = 3_000;
/// Largest `name` + `value` length (bytes) of one cookie.
pub const MAX_PAIR_BYTES: usize = 4_096;

const VERSION: u32 = 1;
/// Slack for the `{"version":1,"cookies":[]}` wrapper.
const HEADER_BYTES: usize = 64;

/// Serialises writers in this process; the file lock covers other processes.
static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Advisory cross-process lock, released when dropped.
struct FileLock(std::fs::File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// One persisted cookie. Plain data; the app maps it to and from the core jar type.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires: SystemTime,
}

impl std::fmt::Debug for StoredCookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredCookie")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("domain", &self.domain)
            .field("host_only", &self.host_only)
            .field("path", &self.path)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("expires", &self.expires)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    value: String,
    domain: String,
    host_only: bool,
    path: String,
    secure: bool,
    http_only: bool,
    expires: u64,
}

#[derive(Serialize, Deserialize)]
struct FileDoc {
    version: u32,
    cookies: Vec<Entry>,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// True when the cookie may be stored: non-empty domain, name+value within the cap.
fn acceptable(name: &str, value: &str, domain: &str) -> bool {
    !domain.is_empty() && name.len().saturating_add(value.len()) <= MAX_PAIR_BYTES
}

/// Largest `domain` (bytes) `save` writes.
pub const MAX_DOMAIN_BYTES: usize = 253;
/// Largest `path` (bytes) `save` writes.
pub const MAX_PATH_BYTES: usize = 1_024;

/// The persisted cookie file of one collection.
#[derive(Debug, Clone)]
pub struct CookieFile {
    path: PathBuf,
}

impl CookieFile {
    pub fn new(dir: &DataDir, id: &CollectionId) -> CookieFile {
        CookieFile {
            path: dir.cookies_file(id),
        }
    }

    fn io(&self, source: std::io::Error) -> StorageError {
        StorageError::Io {
            path: self.path.clone(),
            source,
        }
    }

    fn invalid(&self, message: &str) -> StorageError {
        StorageError::Invalid {
            path: self.path.clone(),
            key: "cookies".to_string(),
            message: message.to_string(),
        }
    }

    /// Err when the file path is a symlink.
    fn refuse_symlink(&self) -> Result<(), StorageError> {
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.file_type().is_symlink() => Err(self.invalid("is a symlink")),
            _ => Ok(()),
        }
    }

    /// Take the process-wide mutex and the advisory file lock (`<id>.lock` beside the cookie
    /// file). Always taken in that order; never held across caller code.
    fn lock(&self) -> Result<(std::sync::MutexGuard<'static, ()>, FileLock), StorageError> {
        let guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.path.with_extension("lock");
        if let Some(parent) = path.parent() {
            ensure_private_dir(parent)?;
        }
        let mut opts = OpenOptions::new();
        // Windows `File::lock` fails on append-only handles.
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let f = opts.open(&path).map_err(|e| StorageError::Io {
            path: path.clone(),
            source: e,
        })?;
        f.lock().map_err(|e| StorageError::Io { path, source: e })?;
        Ok((guard, FileLock(f)))
    }

    /// Persistence is on when the file exists.
    pub fn is_enabled(&self) -> bool {
        std::fs::symlink_metadata(&self.path).is_ok()
    }

    /// Turn persistence on, writing an empty list if the file is absent.
    pub fn enable(&self) -> Result<(), StorageError> {
        self.refuse_symlink()?;
        let _locks = self.lock()?;
        if self.is_enabled() {
            return Ok(());
        }
        self.write(&[], SystemTime::now())
    }

    /// Turn persistence off by removing the file. Succeeds if it is already absent.
    pub fn disable(&self) -> Result<(), StorageError> {
        if !self.is_enabled() {
            return Ok(());
        }
        let _locks = self.lock()?;
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(self.io(e)),
        }
    }

    /// Read the stored cookies. An absent file gives an empty list.
    ///
    /// A symlink, a file over [`MAX_FILE_BYTES`] (not read past the cap), unparseable JSON or a
    /// `version` other than 1 is an error and the file is left untouched. Expired entries, entries
    /// with an empty domain or a name+value over [`MAX_PAIR_BYTES`] are skipped, and at most
    /// [`MAX_COOKIES`] cookies are kept.
    pub fn load(&self, now: SystemTime) -> Result<Vec<StoredCookie>, StorageError> {
        self.refuse_symlink()?;
        let file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(self.io(e)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| self.io(e))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(self.invalid("file is over 4 MiB"));
        }
        let doc: FileDoc = serde_json::from_slice(&bytes).map_err(|e| StorageError::Parse {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        if doc.version != VERSION {
            return Err(StorageError::UnsupportedFormat {
                path: self.path.clone(),
                found: i64::from(doc.version),
            });
        }
        let now_s = secs(now);
        Ok(doc
            .cookies
            .into_iter()
            .filter(|e| e.expires > now_s && acceptable(&e.name, &e.value, &e.domain))
            .filter_map(|e| {
                let expires = UNIX_EPOCH.checked_add(Duration::from_secs(e.expires))?;
                Some((e, expires))
            })
            .take(MAX_COOKIES)
            .map(|(e, expires)| StoredCookie {
                name: e.name,
                value: e.value,
                domain: e.domain,
                host_only: e.host_only,
                path: e.path,
                secure: e.secure,
                http_only: e.http_only,
                expires,
            })
            .collect())
    }

    /// Write the cookies, dropping expired and over-limit ones (same rules as [`load`]).
    /// A no-op while persistence is off; refuses a symlinked file.
    ///
    /// [`load`]: CookieFile::load
    pub fn save(&self, cookies: &[StoredCookie], now: SystemTime) -> Result<(), StorageError> {
        self.refuse_symlink()?;
        // Off: nothing to write, and no lock file or directory to create. Re-checked under
        // the lock, since another writer may disable in between.
        if !self.is_enabled() {
            return Ok(());
        }
        let _locks = self.lock()?;
        if !self.is_enabled() {
            return Ok(());
        }
        self.write(cookies, now)
    }

    fn write(&self, cookies: &[StoredCookie], now: SystemTime) -> Result<(), StorageError> {
        let now_s = secs(now);
        let mut entries: Vec<Entry> = cookies
            .iter()
            .filter(|c| {
                secs(c.expires) > now_s
                    && acceptable(&c.name, &c.value, &c.domain)
                    && c.domain.len() <= MAX_DOMAIN_BYTES
                    && c.path.len() <= MAX_PATH_BYTES
            })
            .take(MAX_COOKIES)
            .map(|c| Entry {
                name: c.name.clone(),
                value: c.value.clone(),
                domain: c.domain.clone(),
                host_only: c.host_only,
                path: c.path.clone(),
                secure: c.secure,
                http_only: c.http_only,
                expires: secs(c.expires),
            })
            .collect();
        // Keep the file within what `load` accepts: drop the soonest-expiring cookies first.
        let sizes = entries
            .iter()
            .map(|e| serde_json::to_vec(e).map(|v| v.len() + 1))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.invalid(&e.to_string()))?;
        let budget = (MAX_FILE_BYTES as usize).saturating_sub(HEADER_BYTES);
        if sizes.iter().sum::<usize>() > budget {
            let mut order: Vec<usize> = (0..entries.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(entries[i].expires));
            let mut keep = vec![false; entries.len()];
            let mut used = 0usize;
            for i in order {
                if used + sizes[i] > budget {
                    break;
                }
                used += sizes[i];
                keep[i] = true;
            }
            let mut it = keep.into_iter();
            entries.retain(|_| it.next().unwrap_or(false));
        }
        let doc = FileDoc {
            version: VERSION,
            cookies: entries,
        };
        let bytes = serde_json::to_vec(&doc).map_err(|e| self.invalid(&e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent)?;
        }
        atomic_write_force_private(&self.path, &bytes)
    }
}
