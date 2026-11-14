//! Importers for foreign collection formats.
//!
//! Each importer turns hostile input into one [`ImportResult`]; [`write`] puts
//! a result into an open collection through the ordinary storage write paths.
//! Nothing is dropped silently: every unsupported construct is a warning.

pub mod curl;
pub mod openapi;
pub mod postman;
pub mod write;
pub mod yaml;

use wisp_core::request::{KeyValue, Request, Var};

pub use write::{WriteReport, write};

/// Largest input accepted, in bytes.
pub const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;
/// Deepest nesting accepted in JSON or YAML input.
pub const MAX_DEPTH: usize = 64;
/// Most requests one import may produce.
pub const MAX_REQUESTS: usize = 5_000;
/// Name an importer gives the environment that holds secrets; [`write`]
/// replaces it with the caller's `secrets_env`.
pub const SECRETS_ENV_PLACEHOLDER: &str = "<secrets>";

/// A failure importing a foreign document.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("input too large: {0}")]
    TooLarge(String),
    #[error("input nested too deeply (more than {MAX_DEPTH} levels)")]
    TooDeep,
    #[error("cannot parse input: {0}")]
    Parse(String),
    #[error("unsupported input: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Storage(#[from] crate::StorageError),
}

/// Recognised input formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Curl,
    Postman,
    OpenApi,
}

/// A variable set to create as an environment.
#[derive(Clone, PartialEq, Default)]
pub struct ImportedEnv {
    pub name: String,
    pub vars: Vec<Var>,
    pub secrets: Vec<Var>,
}

impl std::fmt::Debug for ImportedEnv {
    /// Secret values never print; their names do.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct Redacted<'a>(&'a [Var]);
        impl std::fmt::Debug for Redacted<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_list()
                    .entries(self.0.iter().map(|v| (&v.name, "<redacted>")))
                    .finish()
            }
        }
        f.debug_struct("ImportedEnv")
            .field("name", &self.name)
            .field("vars", &self.vars)
            .field("secrets", &Redacted(&self.secrets))
            .finish()
    }
}

/// Most warnings kept; one summary warning stands in for the rest.
pub const MAX_WARNINGS: usize = 1_000;
const MAX_WARNING_BYTES: usize = 512;

fn truncate_bytes(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

/// A construct that was not imported, and where it was.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportWarning {
    /// Item path, folders and request joined by ` / `.
    pub at: String,
    pub message: String,
}

/// One node of the imported tree.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ImportItem {
    Folder {
        name: String,
        items: Vec<ImportItem>,
    },
    Request {
        name: String,
        request: Request,
    },
}

/// What an importer produced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportResult {
    pub name: String,
    pub items: Vec<ImportItem>,
    pub collection_vars: Vec<Var>,
    pub environments: Vec<ImportedEnv>,
    pub warnings: Vec<ImportWarning>,
}

impl ImportResult {
    pub(crate) fn warn(&mut self, at: &str, message: impl Into<String>) {
        if self.warnings.len() > MAX_WARNINGS {
            return;
        }
        if self.warnings.len() == MAX_WARNINGS {
            self.warnings.push(ImportWarning {
                at: String::new(),
                message: format!("more than {MAX_WARNINGS} warnings; the rest are not shown"),
            });
            return;
        }
        self.warnings.push(ImportWarning {
            at: truncate_bytes(at.to_string(), MAX_WARNING_BYTES),
            message: truncate_bytes(message.into(), MAX_WARNING_BYTES),
        });
    }
}

/// Guess the format of `input`. `None` when it is none of the three.
pub fn detect(input: &[u8]) -> Option<Format> {
    if input.len() > MAX_INPUT_BYTES {
        return None;
    }
    let head = &input[..input.len().min(64 * 1024)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if trimmed.starts_with('{') {
        let lower = trimmed.to_ascii_lowercase();
        // Postman collections carry a schema URL or `_postman_id` in `info`.
        if lower.contains("getpostman.com") || lower.contains("_postman_id") {
            return Some(Format::Postman);
        }
        if lower.contains("\"openapi\"") || lower.contains("\"swagger\"") {
            return Some(Format::OpenApi);
        }
        return None;
    }
    if trimmed.starts_with("curl ") || trimmed.starts_with("curl\t") || trimmed == "curl" {
        return Some(Format::Curl);
    }
    let yaml = trimmed.lines().any(|l| {
        let l = l.trim_start_matches(['"', '\'']);
        l.starts_with("openapi:") || l.starts_with("swagger:")
    });
    yaml.then_some(Format::OpenApi)
}

pub(crate) fn check_size(len: usize) -> Result<(), ImportError> {
    if len > MAX_INPUT_BYTES {
        return Err(ImportError::TooLarge(format!(
            "{len} bytes (limit {MAX_INPUT_BYTES})"
        )));
    }
    Ok(())
}

/// Reject JSON nested deeper than [`MAX_DEPTH`] without recursing.
pub(crate) fn check_json_depth(bytes: &[u8]) -> Result<(), ImportError> {
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for &b in bytes {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(ImportError::TooDeep);
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Size and depth checked JSON parse.
pub(crate) fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, ImportError> {
    check_size(bytes.len())?;
    check_json_depth(bytes)?;
    serde_json::from_slice(bytes).map_err(|e| ImportError::Parse(e.to_string()))
}

/// Counts requests against [`MAX_REQUESTS`].
#[derive(Default)]
pub(crate) struct RequestCounter(usize);

impl RequestCounter {
    pub(crate) fn add(&mut self) -> Result<(), ImportError> {
        self.0 += 1;
        if self.0 > MAX_REQUESTS {
            return Err(ImportError::TooLarge(format!(
                "more than {MAX_REQUESTS} requests"
            )));
        }
        Ok(())
    }
}

/// Decode `%XX` escapes; invalid escapes stay as written.
pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Decode form-urlencoded text: `+` is a space, then `%XX` escapes.
pub(crate) fn form_decode(s: &str) -> String {
    percent_decode(&s.replace('+', " "))
}

/// Longest request-name prefix used as the base of a secret name, so a huge
/// name times many credential headers stays cheap.
const MAX_SECRET_BASE_BYTES: usize = 64;

/// Byte offset where the authority of `[scheme://]host...` starts.
fn authority_start(url: &str) -> usize {
    // `://` first, whatever the scheme text (a template like `{{protocol}}`
    // included), as long as nothing that ends an authority comes before it.
    if let Some(i) = url.find("://")
        && !url[..i].contains(['/', '?', '#', '@'])
    {
        let after = &url[i + 3..];
        return i + 3 + after.len() - after.trim_start_matches('/').len();
    }
    // Lenient forms (`//h`, `http:/h`, `https:///h`) only when there is no
    // `://`; a bare scheme is letters then `:` and at least one `/`.
    if url.starts_with("//") {
        2
    } else {
        match url.find(':') {
            Some(i)
                if i > 0
                    && url[..i]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
                    && url[i + 1..].starts_with('/') =>
            {
                i + 1 + url[i + 1..].len() - url[i + 1..].trim_start_matches('/').len()
            }
            _ => 0,
        }
    }
}

/// A non-empty all-digit port. A templated port (`{{port}}`) is not one: a
/// template is plain host:port and never takes part in the ambiguity checks.
fn is_port_like(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())
}

/// Split `user:pass@` off the authority of `[scheme://]host...`. Returns the URL
/// without it and the percent-decoded `(user, password)`.
pub(crate) fn split_userinfo(url: &str) -> (String, Option<(String, String)>) {
    let scheme_end = authority_start(url);
    let rest = &url[scheme_end..];
    let first_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let at = if let Some(at) = rest[..first_end].rfind('@') {
        at
    } else {
        // A raw `/`, `?` or `#` inside the password moves the apparent end of
        // the authority. A leading `user:pass` segment (not `host:port`)
        // followed by a later `@` is treated as userinfo.
        let seg = &rest[..first_end];
        // An empty port can never be a real port, so it counts as userinfo.
        // `http:h/x@y` is a scheme with no slash (host `h`), not userinfo.
        let looks_like_userinfo = seg.split_once(':').is_some_and(|(h, p)| {
            (!h.is_empty() || scheme_end > 0)
                && !h.starts_with('[')
                && !["http", "https", "ws", "wss"]
                    .iter()
                    .any(|s| h.eq_ignore_ascii_case(s))
                && (p.is_empty() || !is_port_like(p))
        });
        // A template is not a password: never userinfo when the segment or the
        // text before the authority holds `{{`.
        let templated = seg.contains("{{") || url[..scheme_end].contains("{{");
        let Some(first_at) = rest.find('@').filter(|_| looks_like_userinfo && !templated) else {
            return (url.to_string(), None);
        };
        let limit = rest[first_at..]
            .find(['/', '?', '#'])
            .map_or(rest.len(), |i| first_at + i);
        match rest[..limit].rfind('@') {
            Some(at) => at,
            None => return (url.to_string(), None),
        }
    };
    let info = &rest[..at];
    let (user, pass) = info.split_once(':').unwrap_or((info, ""));
    (
        format!("{}{}", &url[..scheme_end], &rest[at + 1..]),
        Some((percent_decode(user), percent_decode(pass))),
    )
}

/// True for `name:<digits>` then `/`, `?` or `#` and a later `@`: kept as
/// host:port, but the digits may be a password. Importers warn without naming
/// any part of the URL.
pub(crate) fn may_hold_credentials(url: &str) -> bool {
    let rest = &url[authority_start(url)..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let seg = &rest[..end];
    let Some((h, p)) = seg.split_once(':') else {
        return false;
    };
    !seg.contains('@')
        && (!h.is_empty() || authority_start(url) > 0)
        && !seg.starts_with('[')
        && is_port_like(p)
        && rest[end..].contains('@')
}

/// Literal credentials gathered by an importer; they go to the secrets
/// environment and the request keeps `{{name}}` references.
#[derive(Default)]
pub(crate) struct SecretSink {
    secrets: Vec<Var>,
    /// Secret names in use, and the next suffix to try for each base.
    names: std::collections::HashSet<String>,
    next: std::collections::HashMap<String, usize>,
}

/// Lowercase ASCII alphanumerics and `_`; every other run becomes one `_`.
/// Bounded in input scanned and output length.
fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    // Bound the input scanned too: punctuation produces no output.
    for c in raw.chars().take(MAX_SECRET_BASE_BYTES * 4) {
        if out.len() >= MAX_SECRET_BASE_BYTES {
            break;
        }
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out
}

/// True when the whole trimmed value is exactly one `{{name}}` reference.
fn is_single_ref(value: &str) -> bool {
    value
        .trim()
        .strip_prefix("{{")
        .and_then(|v| v.strip_suffix("}}"))
        .is_some_and(|name| !name.trim().is_empty() && !name.contains(['{', '}']))
}

impl SecretSink {
    /// Keep `{{refs}}` and empty values; move a literal into the secrets
    /// environment and return a reference to it.
    pub(crate) fn secret_ref(&mut self, request: &str, field: &str, value: String) -> String {
        if value.is_empty() || is_single_ref(&value) {
            return value;
        }
        let base = sanitize_name(request);
        let field = {
            let f = sanitize_name(field);
            let f = f.trim_matches('_');
            if f.is_empty() { "value" } else { f }.to_string()
        };
        let base = format!(
            "{}{field}",
            if base.is_empty() {
                "request_".to_string()
            } else if base.ends_with('_') {
                base
            } else {
                format!("{base}_")
            }
        );
        let mut name = base.clone();
        if self.names.contains(&name) {
            let next = self.next.entry(base.clone()).or_insert(2);
            loop {
                name = format!("{base}_{next}");
                *next += 1;
                if !self.names.contains(&name) {
                    break;
                }
            }
        }
        self.names.insert(name.clone());
        self.secrets.push(Var {
            name: name.clone(),
            value,
        });
        format!("{{{{{name}}}}}")
    }

    /// The environment holding what was gathered, if anything.
    pub(crate) fn into_env(self) -> Option<ImportedEnv> {
        (!self.secrets.is_empty()).then(|| ImportedEnv {
            name: SECRETS_ENV_PLACEHOLDER.into(),
            vars: Vec::new(),
            secrets: self.secrets,
        })
    }
}

/// Headers whose literal values are credentials.
pub(crate) fn is_credential_header(name: &str) -> bool {
    wisp_core::redact::is_credential_name(name)
}

/// Percent-encode everything but unreserved characters.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Split `a=1&b=2` into enabled key/values, decoding each side.
pub(crate) fn parse_query(q: &str) -> Vec<KeyValue> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            KeyValue {
                name: form_decode(k),
                value: form_decode(v),
                enabled: true,
            }
        })
        .collect()
}

/// Split a URL into the part before `?` and its decoded query params.
pub(crate) fn split_query(url: &str) -> (String, Vec<KeyValue>) {
    match url.split_once('?') {
        Some((base, q)) => (base.to_string(), parse_query(q)),
        None => (url.to_string(), Vec::new()),
    }
}

/// Request name from a method and URL: `GET /users/1`.
pub(crate) fn request_name(method: &str, url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or("");
    let path = if let Some((_, rest)) = no_query.split_once("://") {
        rest.find('/').map_or("/", |i| &rest[i..])
    } else if no_query.starts_with("{{") {
        no_query.split_once("}}").map_or("/", |(_, p)| p)
    } else {
        no_query
    };
    let path = if path.is_empty() { "/" } else { path };
    format!("{method} {path}")
}

/// Parse an HTTP method name, ignoring case.
pub(crate) fn parse_method(s: &str) -> Option<wisp_core::Method> {
    use wisp_core::Method;
    Some(match s.to_ascii_uppercase().as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => return None,
    })
}

/// Upper-case name of a method.
pub(crate) fn method_name(m: wisp_core::Method) -> &'static str {
    use wisp_core::Method;
    match m {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn split_userinfo_ignores_at_after_authority() {
        assert_eq!(super::split_userinfo("http://h/a@b").0, "http://h/a@b");
        assert_eq!(super::split_userinfo("h/a@b").1, None);
        assert_eq!(super::split_userinfo("h?x=a@b").1, None);
        let (u, i) = super::split_userinfo("//bob:SECRETPW@h/x");
        assert_eq!(u, "//h/x");
        assert_eq!(i, Some(("bob".into(), "SECRETPW".into())));
        assert_eq!(super::split_userinfo("//h/a@b").1, None);
        assert!(super::is_credential_header(" X-API-Key "));
        assert!(super::is_credential_header("Private-Token"));
        assert!(!super::is_credential_header("Accept"));
        let (u, i) = super::split_userinfo("bob:pw@h/x");
        assert_eq!(u, "h/x");
        assert_eq!(i, Some(("bob".into(), "pw".into())));
    }

    use super::*;

    #[test]
    fn debug_redacts_imported_env() {
        let env = ImportedEnv {
            name: "prod".into(),
            vars: vec![Var {
                name: "host".into(),
                value: "example.com".into(),
            }],
            secrets: vec![Var {
                name: "token".into(),
                value: "hunter2-secret".into(),
            }],
        };
        for out in [format!("{env:?}"), format!("{env:#?}")] {
            assert!(!out.contains("hunter2-secret"), "{out}");
            assert!(out.contains("token") && out.contains("<redacted>"), "{out}");
            assert!(out.contains("example.com") && out.contains("prod"));
        }
        let r = ImportResult {
            environments: vec![env],
            ..Default::default()
        };
        assert!(!format!("{r:?}").contains("hunter2-secret"));
    }

    #[test]
    fn form_decode_plus_and_percent() {
        assert_eq!(form_decode("a+b"), "a b");
        assert_eq!(form_decode("a%2Bb"), "a+b");
    }
}
