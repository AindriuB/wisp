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

/// A file path from foreign input that is safe to keep: relative, with no
/// `~` and no `..`, on any platform's syntax. `None` means refuse it.
pub(crate) fn safe_file_path(p: &str) -> Option<std::path::PathBuf> {
    let b = p.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    if p.is_empty()
        || p.starts_with(['/', '\\', '~'])
        || drive
        || p.split(['/', '\\']).any(|c| c == "..")
    {
        return None;
    }
    Some(std::path::PathBuf::from(p))
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

    #[test]
    fn safe_paths() {
        for bad in [
            "",
            "/etc/passwd",
            "~/.ssh/id_rsa",
            "~",
            "../x",
            "a/../b",
            "a\\..\\b",
            "C:\\x",
            "c:/x",
            "\\\\h\\s\\x",
        ] {
            assert!(safe_file_path(bad).is_none(), "{bad}");
        }
        for ok in ["a.png", "assets/a.png", "a..b/c", "x\\y"] {
            assert!(safe_file_path(ok).is_some(), "{ok}");
        }
    }
}
