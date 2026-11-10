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
use wisp_core::redact::{SECRET_FRAGMENTS, is_credential_name as is_secret_name};
use wisp_core::request::{ApiKeyLocation, Auth, Body, PartValue, Request};
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

/// Serialises appends: a store is opened per call site, so the lock is process-wide.
static APPEND_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Advisory cross-process lock, released when dropped.
struct DirLock(fs::File);

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

const INDEX_FILE: &str = "index.jsonl";
const BODIES_DIR: &str = "bodies";

/// Keep only a lone `{{var}}` reference in `value`; true when it changed.
fn scrub(value: &mut String) -> bool {
    let kept = keep_var_ref(value);
    if kept == *value {
        return false;
    }
    *value = kept;
    true
}

fn note(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// Redact userinfo and credential-named query values in a URL template.
fn redact_url(url: &str, names: &mut Vec<String>) -> String {
    let (rest, fragment) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let (base, query) = match rest.find('?') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let auth_start = base.find("://").map_or(0, |i| i + 3);
    let auth_end = base[auth_start..]
        .find('/')
        .map_or(base.len(), |i| auth_start + i);
    let mut out = String::with_capacity(url.len());
    match base[auth_start..auth_end].rfind('@') {
        Some(at) => {
            let at = auth_start + at;
            let info = &base[auth_start..at];
            out.push_str(&base[..auth_start]);
            let (mut user, pass) = match info.split_once(':') {
                Some((u, p)) => (u.to_string(), Some(p.to_string())),
                None => (info.to_string(), None),
            };
            if scrub(&mut user) {
                note(names, "url.username".into());
            }
            out.push_str(&user);
            if let Some(mut p) = pass {
                if scrub(&mut p) {
                    note(names, "url.password".into());
                }
                out.push(':');
                out.push_str(&p);
            }
            out.push_str(&base[at..]);
        }
        None => out.push_str(base),
    }
    if let Some(q) = query {
        out.push('?');
        let pairs: Vec<String> = q
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((n, v)) if is_secret_name(n) => {
                    let mut v = v.to_string();
                    if scrub(&mut v) {
                        note(names, format!("query.{n}"));
                    }
                    format!("{n}={v}")
                }
                _ => pair.to_string(),
            })
            .collect();
        out.push_str(&pairs.join("&"));
    }
    out.push_str(fragment);
    out
}

/// Redact string leaves of a JSON value; `secret` is true under a credential-named key.
fn redact_json(v: &mut serde_json::Value, secret: bool, path: &str, names: &mut Vec<String>) {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            for (k, child) in m.iter_mut() {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                redact_json(child, secret || is_secret_name(k), &p, names);
            }
        }
        Value::Array(a) => {
            for child in a {
                redact_json(child, secret, path, names);
            }
        }
        Value::String(s) if secret => {
            if scrub(s) {
                note(names, format!("body.{path}"));
            }
        }
        Value::Number(_) | Value::Bool(_) if secret => {
            *v = Value::String(String::new());
            note(names, format!("body.{path}"));
        }
        _ => {}
    }
}

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
        if (is_secret_name(&h.name) || api_key_header.as_deref() == Some(&lower))
            && scrub(&mut h.value)
        {
            note(&mut names, h.name.clone());
        }
    }
    out.url = redact_url(&req.url, &mut names);
    for p in &mut out.params {
        if is_secret_name(&p.name) && scrub(&mut p.value) {
            note(&mut names, format!("params.{}", p.name));
        }
    }
    for v in &mut out.vars {
        if is_secret_name(&v.name) && scrub(&mut v.value) {
            note(&mut names, format!("vars.{}", v.name));
        }
    }
    match &mut out.body {
        Body::Form(fields) => {
            for f in fields {
                if is_secret_name(&f.name) && scrub(&mut f.value) {
                    note(&mut names, format!("form.{}", f.name));
                }
            }
        }
        Body::Multipart(parts) => {
            for p in parts {
                if let PartValue::Text(t) = &mut p.value
                    && is_secret_name(&p.name)
                    && scrub(t)
                {
                    note(&mut names, format!("form.{}", p.name));
                }
            }
        }
        Body::Json(b) => redact_json_body(&mut b.content, &mut names),
        Body::Text(b) | Body::Xml(b) => {
            let lower = b.content.to_lowercase();
            if SECRET_FRAGMENTS.iter().any(|f| lower.contains(f)) {
                b.content.clear();
                note(&mut names, "body".into());
            }
        }
        _ => {}
    }
    (out, names)
}

/// Marker for a bare `{{...}}` reference: a JSON string token that serde writes back unchanged.
fn ref_marker(n: usize) -> String {
    format!("\u{1}wispref{n}\u{1}")
}

/// True when `inner` is a plain variable name, as `template::var_refs` would report it.
fn is_var_grammar(inner: &str) -> bool {
    let name = inner.trim();
    let refs = wisp_core::template::var_refs(&format!("{{{{{inner}}}}}"));
    refs.len() == 1
        && refs[0].terminated
        && refs[0].name == name
        && !name.strip_prefix('$').unwrap_or(name).is_empty()
        && name
            .strip_prefix('$')
            .unwrap_or(name)
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Swap each `{{...}}` outside a JSON string for a marker string. Inside strings the text
/// is already valid JSON and is left alone. Returns the new text and the originals.
fn mask_bare_refs(text: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut refs = Vec::new();
    let (mut in_str, mut esc) = (false, false);
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if rest.starts_with("{{")
            && let Some(end) = rest.find("}}")
            && is_var_grammar(&rest[2..end])
        {
            // The marker holds raw control characters, so escape it by hand.
            out.push_str(&format!("\"\\u0001wispref{}\\u0001\"", refs.len()));
            refs.push(rest[..end + 2].to_string());
            rest = &rest[end + 2..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    (out, refs)
}

/// Redact credential-named keys in a JSON body template. Bodies are text templates, so
/// `{{ref}}` in value position is masked for the parse and restored unquoted. A body that
/// still does not parse is dropped (fail closed) and `body` is recorded.
fn redact_json_body(content: &mut String, names: &mut Vec<String>) {
    if content.trim().is_empty() {
        return;
    }
    let (masked, refs) = mask_bare_refs(content);
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&masked) else {
        content.clear();
        note(names, "body".into());
        return;
    };
    let mut found = Vec::new();
    redact_json(&mut v, false, "", &mut found);
    if found.is_empty() {
        return;
    }
    let Ok(mut text) = serde_json::to_string(&v) else {
        content.clear();
        note(names, "body".into());
        return;
    };
    for (i, original) in refs.iter().enumerate() {
        text = text.replace(
            &format!("\"{}\"", ref_marker(i).replace('\u{1}', "\\u0001")),
            original,
        );
    }
    *content = text;
    for n in found {
        note(names, n);
    }
}

/// Empty the value of every `Set-Cookie` header; true when any value changed.
fn redact_set_cookies(headers: &mut [Header]) -> bool {
    let mut changed = false;
    for h in headers {
        if !h.name.eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let Some(eq) = h.value.find('=') else {
            continue;
        };
        let end = h.value.find(';').unwrap_or(h.value.len());
        if end <= eq + 1 {
            continue;
        }
        h.value = format!("{}{}", &h.value[..=eq], &h.value[end..]);
        changed = true;
    }
    changed
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

    /// Take the advisory file lock for this collection. It sits beside the collection's
    /// directory (`<id>.lock`) so that [`HistoryStore::clear`] can remove the directory.
    fn lock_file(&self) -> Result<DirLock, StorageError> {
        let path = self.dir.with_extension("lock");
        if let Some(parent) = path.parent() {
            ensure_private_dir(parent)?;
        }
        let mut opts = OpenOptions::new();
        // Windows `File::lock` fails on append-only handles; std wants read, read+append
        // or write access.
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let f = opts.open(&path).map_err(|e| io(&path, e))?;
        f.lock().map_err(|e| io(&path, e))?;
        Ok(DirLock(f))
    }

    /// Store one finished request. `body` is cut to [`BODY_CAP`] bytes.
    pub fn append(&self, new: NewEntry, body: Option<&[u8]>) -> Result<HistoryEntry, StorageError> {
        let _guard = APPEND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _file_lock = self.lock_file()?;
        ensure_private_dir(&self.bodies())?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (request, mut redacted) = redact_request(&new.request);
        let mut response_headers = new.response_headers;
        if redact_set_cookies(&mut response_headers) {
            redacted.push("Set-Cookie".into());
        }
        let (bin, req) = {
            let [a, b] = self.body_files(&id);
            (a, b)
        };
        let stored = body.map(|b| &b[..b.len().min(BODY_CAP)]);
        let truncated = body.is_some_and(|b| b.len() > BODY_CAP);
        if let Some(b) = stored {
            atomic_write_private(&bin, b)?;
        }
        let url = request.url.clone();
        let file = ReqFile {
            request,
            response_headers,
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
            url,
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
        // Best effort: the entry is stored, so a failed sweep must not report it dropped.
        // Leftover files are swept as orphans by the next append.
        if let Err(e) = self.evict() {
            tracing::warn!(error = %e, "history eviction failed");
        }
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

    /// Index lines this build can attribute to an entry: full parses and lines with only a
    /// valid `id` (a newer build's shape). Each is `(id, raw line)`, in file order.
    fn read_index_lines(&self) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        #[derive(Deserialize)]
        struct IdOnly {
            id: String,
        }
        let index = self.index();
        let bytes = match fs::read(&index) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&index, e)),
        };
        Ok(bytes
            .split(|&b| b == b'\n')
            .filter_map(|l| {
                let id = serde_json::from_slice::<IdOnly>(l).ok()?.id;
                valid_id(&id).then(|| (id, l.to_vec()))
            })
            .collect())
    }

    fn entry_bytes(&self, id: &str) -> u64 {
        self.body_files(id)
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Drop the oldest entries over the caps and sweep body files the index does not list.
    /// The index is rewritten before any file is deleted, so a failure leaves orphans, not
    /// dangling index lines.
    fn evict(&self) -> Result<(), StorageError> {
        let mut entries = self.read_index_lines()?;
        let known: std::collections::HashSet<&str> =
            entries.iter().map(|(id, _)| id.as_str()).collect();
        let mut orphans: Vec<PathBuf> = Vec::new();
        let mut orphan_bytes: u64 = 0;
        if let Ok(rd) = fs::read_dir(self.bodies()) {
            for f in rd.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let stem = name.split('.').next().unwrap_or_default();
                if valid_id(stem) && !known.contains(stem) {
                    orphan_bytes += f.metadata().map_or(0, |m| m.len());
                    orphans.push(f.path());
                }
            }
        }
        let sizes: Vec<u64> = entries.iter().map(|(id, _)| self.entry_bytes(id)).collect();
        let mut total: u64 = sizes.iter().sum::<u64>() + orphan_bytes;
        let mut drop_n = entries.len().saturating_sub(self.max_entries);
        for s in sizes.iter().take(drop_n) {
            total -= s;
        }
        // Orphans are always removed, so they leave the total too.
        total -= orphan_bytes;
        while total > self.max_bytes && drop_n + 1 < entries.len() {
            total -= sizes[drop_n];
            drop_n += 1;
        }
        let dropped: Vec<(String, Vec<u8>)> = entries.drain(..drop_n).collect();
        if drop_n > 0 {
            let mut text = Vec::new();
            for (_, line) in &entries {
                text.extend_from_slice(line);
                text.push(b'\n');
            }
            atomic_write_private(&self.index(), &text)?;
        }
        let doomed = dropped
            .iter()
            .flat_map(|(id, _)| self.body_files(id))
            .chain(orphans);
        for p in doomed {
            // A file still open elsewhere (Windows pending delete) is left as an
            // orphan; the next sweep removes it.
            if let Err(err) = fs::remove_file(&p)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!(path = %p.display(), error = %err, "history body left for the next sweep");
            }
        }
        Ok(())
    }

    /// All entries, newest first.
    pub fn list(&self) -> Result<Vec<HistoryEntry>, StorageError> {
        let mut v = self.read_index()?;
        v.reverse();
        Ok(v)
    }

    /// The stored request, response headers and body of one entry.
    pub fn load(&self, entry_id: &str) -> Result<StoredEntry, StorageError> {
        let mut stored = self.load_without_body(entry_id)?;
        let [bin, _] = self.body_files(entry_id);
        stored.body = match fs::read(&bin) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(io(&bin, e)),
        };
        Ok(stored)
    }

    /// Like [`HistoryStore::load`] but leaves `body` empty; see [`HistoryStore::body_path`].
    pub fn load_without_body(&self, entry_id: &str) -> Result<StoredEntry, StorageError> {
        if !valid_id(entry_id) {
            return Err(StorageError::History {
                message: format!("invalid entry id `{entry_id}`"),
            });
        }
        let [_, req] = self.body_files(entry_id);
        let raw = fs::read(&req).map_err(|e| io(&req, e))?;
        let file: ReqFile = serde_json::from_slice(&raw).map_err(|e| StorageError::Parse {
            path: req.clone(),
            message: e.to_string(),
        })?;
        Ok(StoredEntry {
            request: file.request,
            response_headers: file.response_headers,
            body: Vec::new(),
        })
    }

    /// Path of an entry's stored body file, if one exists.
    pub fn body_path(&self, entry_id: &str) -> Option<PathBuf> {
        if !valid_id(entry_id) {
            return None;
        }
        let [bin, _] = self.body_files(entry_id);
        bin.is_file().then_some(bin)
    }

    /// Remove this collection's history.
    pub fn clear(&self) -> Result<(), StorageError> {
        let _guard = APPEND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _file_lock = self.lock_file()?;
        // The index goes first: without it no entry lists, so any body file that
        // cannot be deleted now (open or pending delete) is an orphan the next
        // sweep removes, and clearing does not fail because of it.
        match fs::remove_file(self.index()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(&self.index(), e)),
        }
        if let Ok(rd) = fs::read_dir(self.bodies()) {
            for f in rd.flatten() {
                let _ = fs::remove_file(f.path());
            }
        }
        // Best effort: the directory may still hold the lock file or a leftover.
        let _ = fs::remove_dir_all(&self.dir);
        Ok(())
    }
}
