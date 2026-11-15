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
use wisp_core::assert::{Assertion, AssertionResult, Verdict};
use wisp_core::redact::{contains_credential_fragment, is_credential_name as is_secret_name};
use wisp_core::request::{ApiKeyLocation, Auth, Body, PartValue, Request};
use wisp_core::{ErrorKind, Header, Method};

use crate::StorageError;
use crate::collection::CollectionId;
use crate::data_dir::{DataDir, ensure_private_dir};
use crate::fs::atomic_write_private;

/// Largest response body stored per entry.
pub const BODY_CAP: usize = 1024 * 1024;
/// The state of one assertion in a history record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssertionState {
    Pass,
    Fail,
    Skipped,
}

/// One assertion as history keeps it: label, definition fingerprint and state. Never an
/// expected or actual value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionRecord {
    pub label: String,
    pub hash: String,
    pub state: AssertionState,
}

impl AssertionRecord {
    pub fn from_result(assertion: &Assertion, result: &AssertionResult) -> Self {
        Self {
            label: result.label.clone(),
            hash: assertion.fingerprint(),
            state: match result.verdict {
                Verdict::Pass => AssertionState::Pass,
                Verdict::Fail { .. } => AssertionState::Fail,
                Verdict::Skipped { .. } => AssertionState::Skipped,
            },
        }
    }
}

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

/// Schemes the WHATWG parser always gives an authority.
const SPECIAL_SCHEMES: [&str; 5] = ["http", "https", "ws", "wss", "ftp"];

fn is_special_scheme(scheme: &str) -> bool {
    SPECIAL_SCHEMES
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
}

/// Offset where the authority begins: after `scheme://`, or after a leading `//` (also `\\`
/// and mixed forms, which the WHATWG parser treats as slashes) for scheme-relative URLs.
fn authority_start(base: &str) -> usize {
    let slashes = |s: &str| s.bytes().take_while(|b| matches!(b, b'/' | b'\\')).count();
    let lead = slashes(base);
    if lead >= 2 {
        return lead;
    }
    if let Some((scheme, rest)) = base.split_once(':')
        && !scheme.is_empty()
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        let n = slashes(rest);
        // Special schemes have an authority after any number of slashes, even none.
        if n >= 2 || is_special_scheme(scheme) {
            return scheme.len() + 1 + n;
        }
    }
    base.find("://").map_or(0, |i| i + 3)
}

/// Redact the value of every credential-named `name=value` pair in a query string.
fn redact_query(q: &str, names: &mut Vec<String>) -> String {
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
    pairs.join("&")
}

/// The WHATWG parser deletes ASCII tab, CR and LF anywhere in a URL, so `https:\t/u:p@h/`
/// names a userinfo. Redact a copy with them removed; if that changed anything, store the
/// redacted copy, else the original unchanged. Every URL redaction path goes through here.
fn redact_ignoring_newlines(
    value: &str,
    names: &mut Vec<String>,
    redact: fn(&str, &mut Vec<String>) -> String,
) -> String {
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut found = Vec::new();
    let redacted = redact(&stripped, &mut found);
    if redacted == stripped {
        return value.to_string();
    }
    for n in found {
        note(names, n);
    }
    redacted
}

/// Redact userinfo and credential-named query values in a URL template.
fn redact_url(url: &str, names: &mut Vec<String>) -> String {
    redact_ignoring_newlines(url, names, redact_url_stripped)
}

/// Redact userinfo at every place a `Refresh` value could start an authority (see
/// [`redact_refresh_stripped`]).
fn redact_refresh(value: &str, found: &mut Vec<String>) -> String {
    redact_ignoring_newlines(value, found, redact_refresh_stripped)
}

/// Redact userinfo and credential-named query values in a URL template.
fn redact_url_stripped(url: &str, names: &mut Vec<String>) -> String {
    let (rest, fragment) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let (base, query) = match rest.find('?') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let auth_start = authority_start(base);
    let auth_end = base[auth_start..]
        .find(['/', '\\'])
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
        out.push_str(&redact_query(q, names));
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
    pub dns_ms: Option<u64>,
    #[serde(default)]
    pub connect_ms: Option<u64>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub body_stored: bool,
    #[serde(default)]
    pub body_truncated: bool,
    /// Names of the fields and headers whose values were emptied.
    #[serde(default)]
    pub redacted: Vec<String>,
    /// Name of the environment the request was sent under; never its values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Assertion label, fingerprint and state; never a value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<AssertionRecord>,
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
    pub dns: Option<Duration>,
    pub connect: Option<Duration>,
    pub size: Option<u64>,
    pub response_headers: Vec<Header>,
    pub environment: Option<String>,
    pub assertions: Vec<AssertionRecord>,
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
        Auth::OAuth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
            ..
        } => vec![
            ("token_url", token_url),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("scope", scope),
        ],
        Auth::OAuth2AuthCode {
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            ..
        } => vec![
            ("auth_url", auth_url),
            ("token_url", token_url),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("scope", scope),
        ],
    }
}

/// The request as it is stored, and the names of everything that was emptied.
fn redact_request(req: &Request) -> (Request, Vec<String>) {
    let mut out = req.clone();
    let mut names = Vec::new();
    out.auth = req.auth.redacted();
    // Expected values are authored literals that may be sensitive; the copy carries none.
    out.assertions.clear();
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
    for v in &mut out.path {
        if is_secret_name(&v.name) && scrub(&mut v.value) {
            note(&mut names, format!("path.{}", v.name));
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
        Body::Text(b) | Body::Xml(b) if contains_credential_fragment(&b.content) => {
            b.content.clear();
            note(&mut names, "body".into());
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

/// Parses any JSON, failing on a duplicate key in any object (serde_json's `Value` keeps the last).
struct NoDupKeys;

impl<'de> Deserialize<'de> for NoDupKeys {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{MapAccess, SeqAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NoDupKeys;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E>(self, _: bool) -> Result<NoDupKeys, E> {
                Ok(NoDupKeys)
            }
            fn visit_i64<E>(self, _: i64) -> Result<NoDupKeys, E> {
                Ok(NoDupKeys)
            }
            fn visit_u64<E>(self, _: u64) -> Result<NoDupKeys, E> {
                Ok(NoDupKeys)
            }
            fn visit_f64<E>(self, _: f64) -> Result<NoDupKeys, E> {
                Ok(NoDupKeys)
            }
            fn visit_str<E>(self, _: &str) -> Result<NoDupKeys, E> {
                Ok(NoDupKeys)
            }
            fn visit_unit<E>(self) -> Result<NoDupKeys, E> {
                Ok(NoDupKeys)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<NoDupKeys, A::Error> {
                while a.next_element::<NoDupKeys>()?.is_some() {}
                Ok(NoDupKeys)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<NoDupKeys, A::Error> {
                let mut seen = std::collections::HashSet::new();
                while let Some(k) = a.next_key::<String>()? {
                    if !seen.insert(k) {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                    a.next_value::<NoDupKeys>()?;
                }
                Ok(NoDupKeys)
            }
        }
        d.deserialize_any(V)
    }
}

/// Redact credential-named keys in a JSON body template. Bodies are text templates, so
/// `{{ref}}` in value position is masked for the parse and restored unquoted. A body that
/// still does not parse is dropped (fail closed) and `body` is recorded.
fn redact_json_body(content: &mut String, names: &mut Vec<String>) {
    if content.trim().is_empty() {
        return;
    }
    let (masked, refs) = mask_bare_refs(content);
    if serde_json::from_str::<NoDupKeys>(&masked).is_err() {
        content.clear();
        note(names, "body".into());
        return;
    }
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

/// Empty the value of every `Set-Cookie` header; true when any value changed. `name=value`
/// keeps the name; a nameless `token; Path=/` has the part before the first `;` emptied.
fn redact_set_cookies(headers: &mut [Header]) -> bool {
    let mut changed = false;
    for h in headers {
        if !h.name.eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let end = h.value.find(';').unwrap_or(h.value.len());
        let head = &h.value[..end];
        let keep = match head.find('=') {
            Some(eq) => eq + 1,
            None => 0,
        };
        if end <= keep {
            continue;
        }
        h.value = format!("{}{}", &h.value[..keep], &h.value[end..]);
        changed = true;
    }
    changed
}

/// Empty the value of every non-cookie response header with a credential-like name.
fn redact_credential_headers(headers: &mut [Header], names: &mut Vec<String>) {
    for h in headers {
        if !h.name.eq_ignore_ascii_case("set-cookie")
            && is_secret_name(&h.name)
            && !h.value.is_empty()
        {
            h.value.clear();
            note(names, h.name.clone());
        }
    }
}

/// Redact userinfo at every place a `Refresh` value could start an authority. The value is
/// not parsed down to one target URL: that is how a second `url=`, text after a closing
/// quote or a whitespace separator slipped past. Over-redacting is safe; credential-free
/// values come back unchanged.
fn redact_refresh_stripped(value: &str, found: &mut Vec<String>) -> String {
    let b = value.as_bytes();
    let is_slash = |c: u8| matches!(c, b'/' | b'\\');
    let skip_ws = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    };
    let skip_slashes = |mut i: usize| {
        while i < b.len() && is_slash(b[i]) {
            i += 1;
        }
        i
    };
    let mut starts: Vec<usize> = Vec::new();
    // Every run of two or more slashes (`://`, `//`, `\\`, mixed) opens an authority.
    let mut i = 0;
    while i < b.len() {
        if is_slash(b[i]) {
            let end = skip_slashes(i);
            if end - i >= 2 {
                starts.push(end);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    // Anything after `url`, optional whitespace and `=`, one optional quote, an optional
    // scheme and at least one slash.
    for i in 0..b.len().saturating_sub(2) {
        if !b[i..i + 3].eq_ignore_ascii_case(b"url") {
            continue;
        }
        let mut j = skip_ws(i + 3);
        if j < b.len() && b[j] == b'=' {
            j = skip_ws(j + 1);
        }
        if j < b.len() && matches!(b[j], b'\'' | b'"') {
            j += 1;
        }
        let mut k = j;
        if k < b.len() && b[k].is_ascii_alphabetic() {
            while k < b.len()
                && (b[k].is_ascii_alphanumeric() || matches!(b[k], b'+' | b'-' | b'.'))
            {
                k += 1;
            }
            if k < b.len() && b[k] == b':' {
                j = k + 1;
            }
        }
        let end = skip_slashes(j);
        if end > j {
            starts.push(end);
        }
    }
    // A special-scheme name and `:` opens an authority after any slashes, even none.
    for i in 0..b.len() {
        for s in SPECIAL_SCHEMES {
            let end = i + s.len();
            if end < b.len() && b[i..end].eq_ignore_ascii_case(s.as_bytes()) && b[end] == b':' {
                starts.push(skip_slashes(end + 1));
            }
        }
    }
    starts.sort_unstable();
    starts.dedup();
    let mut out = value.to_string();
    // Back to front, so an edit never moves a start still to be processed.
    for &p in starts.iter().rev() {
        let span_end = out[p..]
            .find(['/', '\\', '?', '#'])
            .map_or(out.len(), |i| p + i);
        let Some(at) = out[p..span_end].rfind('@').map(|i| p + i) else {
            continue;
        };
        let info = &out[p..at];
        let (mut user, pass) = match info.split_once(':') {
            Some((u, pw)) => (u.to_string(), Some(pw.to_string())),
            None => (info.to_string(), None),
        };
        if scrub(&mut user) {
            note(found, "url.username".into());
        }
        if let Some(mut pw) = pass {
            if scrub(&mut pw) {
                note(found, "url.password".into());
            }
            user.push(':');
            user.push_str(&pw);
        }
        out.replace_range(p..at, &user);
    }
    // Every `?` opens a query that runs to `#` or the end: an unquoted URL may hold
    // quotes, `;` and whitespace, so none of them end it.
    let mut pos = out.len();
    while let Some(q) = out[..pos].rfind('?') {
        let end = out[q + 1..].find('#').map_or(out.len(), |i| q + 1 + i);
        let redacted = redact_query(&out[q + 1..end], found);
        out.replace_range(q + 1..end, &redacted);
        pos = q;
    }
    out
}

/// Run every response header that can carry a URL through [`redact_url`].
fn redact_url_headers(headers: &mut [Header], names: &mut Vec<String>) {
    for h in headers {
        let n = &h.name;
        if n.eq_ignore_ascii_case("location")
            || n.eq_ignore_ascii_case("content-location")
            || n.eq_ignore_ascii_case("refresh")
        {
            let mut found = Vec::new();
            let redacted = if n.eq_ignore_ascii_case("refresh") {
                redact_refresh(&h.value, &mut found)
            } else {
                redact_url(&h.value, &mut found)
            };
            if redacted != h.value {
                h.value = redacted;
                note(names, h.name.clone());
            }
        }
    }
}

/// Redact a response header list as it is stored.
fn redact_response_headers(headers: &mut [Header], names: &mut Vec<String>) {
    if redact_set_cookies(headers) {
        note(names, "Set-Cookie".into());
    }
    redact_credential_headers(headers, names);
    redact_url_headers(headers, names);
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
    lock_wait: Duration,
}

/// Longest an append or clear waits for the cross-process lock.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// Present in a history dir once entries from older builds have been re-redacted.
const SCRUB_MARKER: &str = ".scrubbed-1";

impl HistoryStore {
    /// Re-redacts an existing history dir once (see [`SCRUB_MARKER`]); a dir that does not
    /// exist yet is left alone. Failures are logged and retried on the next open.
    pub fn open(data: &DataDir, id: &CollectionId) -> HistoryStore {
        let store = HistoryStore {
            dir: data.history_dir(id),
            max_entries: MAX_ENTRIES,
            max_bytes: MAX_BYTES,
            lock_wait: LOCK_WAIT,
        };
        if store.dir.is_dir()
            && !store.dir.join(SCRUB_MARKER).exists()
            && let Err(e) = store.scrub_legacy()
        {
            tracing::warn!(error = %e, "history scrub failed");
        }
        store
    }

    /// Shorten the lock wait. For tests.
    #[doc(hidden)]
    pub fn with_lock_wait(mut self, wait: Duration) -> HistoryStore {
        self.lock_wait = wait;
        self
    }

    /// Re-run the current redaction over entries older builds wrote, then drop the marker.
    fn scrub_legacy(&self) -> Result<(), StorageError> {
        let _guard = APPEND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _file_lock = self.lock_file()?;
        let index = self.index();
        let bytes = match fs::read(&index) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(io(&index, e)),
        };
        let mut changed = false;
        let mut text = Vec::with_capacity(bytes.len());
        for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            let mut out = line.to_vec();
            if let Ok(mut entry) = serde_json::from_slice::<HistoryEntry>(line) {
                let mut names = Vec::new();
                let url = redact_url(&entry.url, &mut names);
                if url != entry.url {
                    entry.url = url;
                    for n in names {
                        note(&mut entry.redacted, n);
                    }
                    if let Ok(v) = serde_json::to_vec(&entry) {
                        out = v;
                        changed = true;
                    }
                }
            }
            text.extend_from_slice(&out);
            text.push(b'\n');
        }
        let rd = match fs::read_dir(self.bodies()) {
            Ok(rd) => Some(rd),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io(&self.bodies(), e)),
        };
        if let Some(rd) = rd {
            for f in rd {
                let f = f.map_err(|e| io(&self.bodies(), e))?;
                let path = f.path();
                let name = f.file_name().to_string_lossy().into_owned();
                let Some(stem) = name.strip_suffix(".req.json") else {
                    continue;
                };
                if !valid_id(stem) {
                    continue;
                }
                let raw = fs::read(&path).map_err(|e| io(&path, e))?;
                let Ok(mut file) = serde_json::from_slice::<ReqFile>(&raw) else {
                    continue;
                };
                let mut names = Vec::new();
                let (request, req_names) = redact_request(&file.request);
                file.request = request;
                names.extend(req_names);
                redact_response_headers(&mut file.response_headers, &mut names);
                let json = serde_json::to_vec(&file).map_err(|e| StorageError::History {
                    message: e.to_string(),
                })?;
                let before = serde_json::from_slice::<ReqFile>(&raw)
                    .ok()
                    .and_then(|f| serde_json::to_vec(&f).ok());
                if before.as_deref() != Some(json.as_slice()) {
                    atomic_write_private(&path, &json)?;
                }
            }
        }
        if changed {
            atomic_write_private(&index, &text)?;
        }
        atomic_write_private(&self.dir.join(SCRUB_MARKER), b"1")
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
        // Never follow a symlink planted at the lock path.
        if let Ok(meta) = fs::symlink_metadata(&path)
            && meta.file_type().is_symlink()
        {
            return Err(StorageError::Invalid {
                path,
                key: "path".to_string(),
                message: "is a symlink".to_string(),
            });
        }
        let f = opts.open(&path).map_err(|e| io(&path, e))?;
        let start = std::time::Instant::now();
        loop {
            match f.try_lock() {
                Ok(()) => return Ok(DirLock(f)),
                Err(fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= self.lock_wait {
                        return Err(StorageError::History {
                            message: format!("timed out waiting for lock {}", path.display()),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(fs::TryLockError::Error(e)) => return Err(io(&path, e)),
            }
        }
    }

    /// Store one finished request. `body` is cut to [`BODY_CAP`] bytes.
    pub fn append(&self, new: NewEntry, body: Option<&[u8]>) -> Result<HistoryEntry, StorageError> {
        let _guard = APPEND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _file_lock = self.lock_file()?;
        ensure_private_dir(&self.bodies())?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (request, mut redacted) = redact_request(&new.request);
        let mut response_headers = new.response_headers;
        redact_response_headers(&mut response_headers, &mut redacted);
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
            dns_ms: new.dns.map(millis),
            connect_ms: new.connect.map(millis),
            size: new.size,
            body_stored: stored.is_some(),
            body_truncated: truncated,
            redacted,
            environment: new.environment,
            assertions: new.assertions,
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
        // A kept line may repeat a dropped id; its files stay.
        let kept: std::collections::HashSet<&str> =
            entries.iter().map(|(id, _)| id.as_str()).collect();
        let doomed = dropped
            .iter()
            .filter(|(id, _)| !kept.contains(id.as_str()))
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
