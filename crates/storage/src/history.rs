//! Request history store.
//!
//! Lives in the per-user data dir under `history/<collection-id>/`: `index.jsonl` holds one
//! JSON line per finished request, and `bodies/<entry-id>.bin` and `bodies/<entry-id>.req.json`
//! hold the response body (capped) and the stored request. The request is stored with its auth
//! and secret-bearing header values redacted by [`NewEntry`] itself.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use wisp_core::request::{ApiKeyLocation, Auth, Request};
use wisp_core::{ErrorKind, Header, Method};

use crate::StorageError;
use crate::collection::CollectionId;
use crate::data_dir::{DataDir, ensure_private_dir};
use crate::fs::atomic_write_private;

/// Largest response body stored per entry.
pub const BODY_CAP: usize = 1024 * 1024;
/// Entries kept per collection.
pub const MAX_ENTRIES: usize = 500;
/// Bytes kept under `bodies/` per collection.
pub const MAX_BYTES: u64 = 100 * 1024 * 1024;

const INDEX_FILE: &str = "index.jsonl";
const BODIES_DIR: &str = "bodies";

/// Header names whose values are never stored (compared case-insensitively).
const SENSITIVE_HEADERS: [&str; 5] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "x-amz-security-token",
];

/// How a request ended. Carries no error message, since those embed resolved URLs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Completed,
    Failed { kind: ErrorKind },
    Cancelled,
}

/// One index line. Unknown fields are ignored and optional ones default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    /// Unix milliseconds.
    pub at: u64,
    /// Request file path relative to the collection root.
    #[serde(default)]
    pub request: Option<String>,
    pub method: Method,
    /// The URL template before resolution.
    pub url: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub ttfb_ms: Option<u64>,
    #[serde(default)]
    pub total_ms: Option<u64>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub body_stored: bool,
    #[serde(default)]
    pub body_truncated: bool,
    /// Names of the fields and headers whose values were emptied.
    #[serde(default)]
    pub redacted: Vec<String>,
}

/// What the caller hands to [`HistoryStore::append`].
#[derive(Debug, Clone)]
pub struct NewEntry {
    pub at: SystemTime,
    pub request_path: Option<String>,
    /// The unresolved draft; redacted by the store.
    pub request: Request,
    pub outcome: Outcome,
    pub status: Option<u16>,
    pub ttfb: Option<Duration>,
    pub total: Option<Duration>,
    pub size: Option<u64>,
    pub response_headers: Vec<Header>,
}

/// A loaded entry's request, response headers and stored body.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEntry {
    pub request: Request,
    pub response_headers: Vec<Header>,
    pub body: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct ReqFile {
    request: Request,
    #[serde(default)]
    response_headers: Vec<Header>,
}

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn keep_var_ref(s: &str) -> String {
    match (Auth::Bearer { token: s.into() }).redacted() {
        Auth::Bearer { token } => token,
        _ => String::new(),
    }
}

fn auth_fields(a: &Auth) -> Vec<(&'static str, &str)> {
    match a {
        Auth::None | Auth::Inherit => vec![],
        Auth::Basic { username, password } | Auth::Digest { username, password } => {
            vec![("username", username), ("password", password)]
        }
        Auth::Bearer { token } => vec![("token", token)],
        Auth::ApiKey { name, value, .. } => vec![("name", name), ("value", value)],
        Auth::AwsSigV4 {
            access_key,
            secret_key,
            session_token,
            region,
            service,
        } => vec![
            ("access_key", access_key),
            ("secret_key", secret_key),
            ("session_token", session_token),
            ("region", region),
            ("service", service),
        ],
    }
}

/// The request as it is stored, and the names of everything that was emptied.
fn redact_request(req: &Request) -> (Request, Vec<String>) {
    let mut out = req.clone();
    let mut names = Vec::new();
    out.auth = req.auth.redacted();
    for ((n, before), (_, after)) in auth_fields(&req.auth)
        .into_iter()
        .zip(auth_fields(&out.auth))
    {
        if before != after {
            names.push(format!("auth.{n}"));
        }
    }
    let api_key_header = match &req.auth {
        Auth::ApiKey {
            name,
            location: ApiKeyLocation::Header,
            ..
        } if !name.is_empty() => Some(name.to_ascii_lowercase()),
        _ => None,
    };
    for h in &mut out.headers {
        let lower = h.name.to_ascii_lowercase();
        if SENSITIVE_HEADERS.contains(&lower.as_str()) || api_key_header.as_deref() == Some(&lower)
        {
            let kept = keep_var_ref(&h.value);
            if kept != h.value {
                h.value = kept;
                if !names.contains(&h.name) {
                    names.push(h.name.clone());
                }
            }
        }
    }
    (out, names)
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// True when the file exists, is non-empty and does not end in a newline.
fn ends_mid_line(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut last = [0u8; 1];
    f.seek(SeekFrom::End(-1)).is_ok() && f.read_exact(&mut last).is_ok() && last[0] != b'\n'
}

/// Per-collection history in the data dir.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    dir: PathBuf,
    max_entries: usize,
    max_bytes: u64,
}

impl HistoryStore {
    /// Does no I/O.
    pub fn open(data: &DataDir, id: &CollectionId) -> HistoryStore {
        HistoryStore {
            dir: data.history_dir(id),
            max_entries: MAX_ENTRIES,
            max_bytes: MAX_BYTES,
        }
    }

    /// Lower the caps. For tests.
    #[doc(hidden)]
    pub fn with_caps(mut self, max_entries: usize, max_bytes: u64) -> HistoryStore {
        self.max_entries = max_entries;
        self.max_bytes = max_bytes;
        self
    }

    fn index(&self) -> PathBuf {
        self.dir.join(INDEX_FILE)
    }

    fn bodies(&self) -> PathBuf {
        self.dir.join(BODIES_DIR)
    }

    fn body_files(&self, id: &str) -> [PathBuf; 2] {
        let b = self.bodies();
        [
            b.join(format!("{id}.bin")),
            b.join(format!("{id}.req.json")),
        ]
    }

    /// Store one finished request. `body` is cut to [`BODY_CAP`] bytes.
    pub fn append(&self, new: NewEntry, body: Option<&[u8]>) -> Result<HistoryEntry, StorageError> {
        ensure_private_dir(&self.bodies())?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (request, redacted) = redact_request(&new.request);
        let (bin, req) = {
            let [a, b] = self.body_files(&id);
            (a, b)
        };
        let stored = body.map(|b| &b[..b.len().min(BODY_CAP)]);
        let truncated = body.is_some_and(|b| b.len() > BODY_CAP);
        if let Some(b) = stored {
            atomic_write_private(&bin, b)?;
        }
        let file = ReqFile {
            request,
            response_headers: new.response_headers,
        };
        let json = serde_json::to_vec(&file).map_err(|e| StorageError::History {
            message: e.to_string(),
        })?;
        atomic_write_private(&req, &json)?;

        let entry = HistoryEntry {
            id,
            at: millis(new.at.duration_since(UNIX_EPOCH).unwrap_or_default()),
            request: new.request_path,
            method: new.request.method,
            url: new.request.url,
            outcome: new.outcome,
            status: new.status,
            ttfb_ms: new.ttfb.map(millis),
            total_ms: new.total.map(millis),
            size: new.size,
            body_stored: stored.is_some(),
            body_truncated: truncated,
            redacted,
        };
        let mut line = serde_json::to_string(&entry).map_err(|e| StorageError::History {
            message: e.to_string(),
        })?;
        line.push('\n');
        let index = self.index();
        if ends_mid_line(&index) {
            line.insert(0, '\n');
        }
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&index).map_err(|e| io(&index, e))?;
        f.write_all(line.as_bytes()).map_err(|e| io(&index, e))?;
        drop(f);
        self.evict()?;
        Ok(entry)
    }

    /// Entries in file order (oldest first), skipping malformed lines.
    fn read_index(&self) -> Result<Vec<HistoryEntry>, StorageError> {
        let index = self.index();
        let bytes = match fs::read(&index) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&index, e)),
        };
        Ok(bytes
            .split(|&b| b == b'\n')
            .filter_map(|l| serde_json::from_slice::<HistoryEntry>(l).ok())
            .filter(|e| valid_id(&e.id))
            .collect())
    }

    fn entry_bytes(&self, id: &str) -> u64 {
        self.body_files(id)
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    fn evict(&self) -> Result<(), StorageError> {
        let mut entries = self.read_index()?;
        let mut sizes: Vec<u64> = entries.iter().map(|e| self.entry_bytes(&e.id)).collect();
        let mut total: u64 = sizes.iter().sum();
        let mut drop_n = entries.len().saturating_sub(self.max_entries);
        for s in sizes.iter().take(drop_n) {
            total -= s;
        }
        while total > self.max_bytes && drop_n + 1 < entries.len() {
            total -= sizes[drop_n];
            drop_n += 1;
        }
        if drop_n == 0 {
            return Ok(());
        }
        for e in &entries[..drop_n] {
            for p in self.body_files(&e.id) {
                match fs::remove_file(&p) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(io(&p, err)),
                }
            }
        }
        entries.drain(..drop_n);
        sizes.drain(..drop_n);
        let mut text = String::new();
        for e in &entries {
            text.push_str(
                &serde_json::to_string(e).map_err(|e| StorageError::History {
                    message: e.to_string(),
                })?,
            );
            text.push('\n');
        }
        atomic_write_private(&self.index(), text.as_bytes())
    }

    /// All entries, newest first.
    pub fn list(&self) -> Result<Vec<HistoryEntry>, StorageError> {
        let mut v = self.read_index()?;
        v.reverse();
        Ok(v)
    }

    /// The stored request, response headers and body of one entry.
    pub fn load(&self, entry_id: &str) -> Result<StoredEntry, StorageError> {
        if !valid_id(entry_id) {
            return Err(StorageError::History {
                message: format!("invalid entry id `{entry_id}`"),
            });
        }
        let [bin, req] = self.body_files(entry_id);
        let raw = fs::read(&req).map_err(|e| io(&req, e))?;
        let file: ReqFile = serde_json::from_slice(&raw).map_err(|e| StorageError::Parse {
            path: req.clone(),
            message: e.to_string(),
        })?;
        let body = match fs::read(&bin) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(io(&bin, e)),
        };
        Ok(StoredEntry {
            request: file.request,
            response_headers: file.response_headers,
            body,
        })
    }

    /// Remove this collection's history.
    pub fn clear(&self) -> Result<(), StorageError> {
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(&self.dir, e)),
        }
    }
}
