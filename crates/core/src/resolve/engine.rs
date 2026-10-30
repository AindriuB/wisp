//! The pure `resolve()` implementation. No I/O.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;

use super::{Location, Problem, ResolveError, ResolveWarning, Scope};
use crate::model::{
    ClientSettings, Header, PreparedBody, PreparedPart, PreparedPartValue, PreparedRequest,
};
use crate::request::{ApiKeyLocation, Auth, Body, KeyValue, PartValue, Request, TextBody};
use crate::template::{self, PathToken};

/// Maximum nesting of variables whose values contain further variables.
const MAX_DEPTH: usize = 8;

/// Maximum bytes that variable expansion may add to a single resolved value.
const MAX_OUTPUT: usize = 64 * 1024;

/// Maximum variable expansions in one `resolve()` call.
const MAX_EXPANSIONS: usize = 10_000;

/// Everything except RFC 3986 unreserved characters is percent-encoded.
const ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn encode(s: &str) -> String {
    utf8_percent_encode(s, ENCODE).to_string()
}

fn encode_form(s: &str) -> String {
    encode(s).replace("%20", "+")
}

/// Find a variable by precedence. Borrows from the inputs, so no copy is made.
fn lookup<'s>(req: &'s Request, scope: &'s Scope<'s>, name: &str) -> Option<(&'s str, VarSource)> {
    let find = |vars: &'s [crate::request::Var]| {
        vars.iter()
            .find(|v| v.name == name)
            .map(|v| v.value.as_str())
    };
    find(&req.vars)
        .map(|v| (v, VarSource::Request))
        .or_else(|| {
            scope
                .folders
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, f)| find(&f.vars).map(|v| (v, VarSource::Folder(i))))
        })
        .or_else(|| {
            scope
                .environment
                .and_then(|e| find(&e.secrets))
                .map(|v| (v, VarSource::Secret))
        })
        .or_else(|| {
            scope
                .environment
                .and_then(|e| find(&e.vars))
                .map(|v| (v, VarSource::Environment))
        })
        .or_else(|| find(&scope.collection.vars).map(|v| (v, VarSource::Collection)))
}

/// Where a variable's value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarSource {
    Request,
    /// Index into `Scope::folders` (outermost = 0).
    Folder(usize),
    Environment,
    Secret,
    Collection,
}

/// The result of looking up one variable.
#[derive(Clone, PartialEq, Eq)]
pub enum VarLookup {
    Found {
        /// Fully expanded, with the same bounds as `resolve`.
        value: String,
        source: VarSource,
        /// True when the variable, or any variable its expansion used, is a secret.
        secret: bool,
    },
    /// `$uuid` / `$timestamp`: generated at resolve time.
    Dynamic,
    Unresolved,
    Error(Problem),
}

impl std::fmt::Debug for VarLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VarLookup::Found {
                value,
                source,
                secret,
            } => {
                let shown: &str = if *secret { "<redacted>" } else { value };
                f.debug_struct("Found")
                    .field("value", &shown)
                    .field("source", source)
                    .field("secret", secret)
                    .finish()
            }
            VarLookup::Dynamic => f.write_str("Dynamic"),
            VarLookup::Unresolved => f.write_str("Unresolved"),
            VarLookup::Error(p) => f.debug_tuple("Error").field(p).finish(),
        }
    }
}

/// Look up one variable through the same precedence and bounds as `resolve`.
pub fn lookup_var(request: &Request, scope: &Scope, name: &str) -> VarLookup {
    let name = name.trim();
    if let Some(dynamic) = name.strip_prefix('$') {
        return if matches!(dynamic, "uuid" | "timestamp") {
            VarLookup::Dynamic
        } else {
            VarLookup::Unresolved
        };
    }
    let Some((raw, source)) = lookup(request, scope, name) else {
        return VarLookup::Unresolved;
    };
    let mut r = Resolver {
        req: request,
        scope,
        problems: Vec::new(),
        warnings: Vec::new(),
        expansions: 0,
        stop: false,
        seen: HashSet::new(),
        used_secret: matches!(source, VarSource::Secret),
    };
    let value = r.expand(
        raw,
        &Location::Var(name.to_string()),
        &mut vec![name.to_string()],
    );
    match r.problems.into_iter().next() {
        Some(p) => VarLookup::Error(p),
        None => VarLookup::Found {
            value,
            source,
            secret: r.used_secret,
        },
    }
}

/// Resolve an authored request against its scope. Pure: no I/O.
pub fn resolve(req: &Request, scope: &Scope) -> Result<PreparedRequest, ResolveError> {
    let mut r = Resolver {
        req,
        scope,
        problems: Vec::new(),
        warnings: Vec::new(),
        expansions: 0,
        stop: false,
        seen: HashSet::new(),
        used_secret: false,
    };
    let prepared = r.run();
    if r.problems.is_empty() {
        Ok(prepared)
    } else {
        Err(ResolveError {
            problems: r.problems,
        })
    }
}

struct Resolver<'a> {
    req: &'a Request,
    scope: &'a Scope<'a>,
    problems: Vec<Problem>,
    warnings: Vec<ResolveWarning>,
    /// Variable expansions performed so far in this call.
    expansions: usize,
    /// Set when the current value hit the size cap or the work budget.
    stop: bool,
    /// Debug renderings of problems already recorded, for O(1) dedupe.
    seen: HashSet<String>,
    /// Set when any expanded variable came from `Environment::secrets`.
    used_secret: bool,
}

impl Resolver<'_> {
    fn problem(&mut self, p: Problem) {
        if self.seen.insert(format!("{p:?}")) {
            self.problems.push(p);
        }
    }

    fn subst(&mut self, s: &str, loc: &Location) -> String {
        self.stop = false;
        self.expand(s, loc, &mut Vec::new())
    }

    fn too_large(&mut self, loc: &Location) {
        self.stop = true;
        self.problem(Problem::Syntax {
            location: loc.clone(),
            message: "variable expansion too large".into(),
        });
    }

    fn expand(&mut self, s: &str, loc: &Location, stack: &mut Vec<String>) -> String {
        let top = stack.is_empty();
        let mut out = String::new();
        // Bytes added by expansion. At the top level the literal text does not
        // count; inside a variable's value everything is expansion growth.
        let mut grown = 0usize;
        let mut rest = 0usize;
        for var in template::var_refs(s) {
            if self.stop {
                return out;
            }
            out.push_str(&s[rest..var.range.start]);
            if !var.terminated {
                self.problem(Problem::Syntax {
                    location: loc.clone(),
                    message: "unclosed `{{`".into(),
                });
                out.push_str(&s[var.range.start..]);
                return out;
            }
            let name = var.name.as_str();
            rest = var.range.end;
            // Every placeholder processed (empty ones included) is charged, whether it resolves or not.
            self.expansions += 1;
            if self.expansions > MAX_EXPANSIONS {
                self.too_large(loc);
                return out;
            }
            if name.is_empty() {
                self.problem(Problem::Syntax {
                    location: loc.clone(),
                    message: "empty variable name".into(),
                });
                continue;
            }
            let before = out.len();
            if let Some(dynamic) = name.strip_prefix('$') {
                match dynamic {
                    "uuid" => out.push_str(&uuid::Uuid::new_v4().to_string()),
                    "timestamp" => {
                        let secs = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs());
                        out.push_str(&secs.to_string());
                    }
                    _ => self.problem(Problem::UnresolvedVar {
                        name: name.to_string(),
                        location: loc.clone(),
                    }),
                }
            } else if let Some((value, source)) = lookup(self.req, self.scope, name) {
                if matches!(source, VarSource::Secret) {
                    self.used_secret = true;
                }
                if stack.iter().any(|n| n == name) {
                    self.problem(Problem::VarCycle {
                        name: name.to_string(),
                    });
                } else if stack.len() >= MAX_DEPTH {
                    self.problem(Problem::Syntax {
                        location: Location::Var(name.to_string()),
                        message: format!("variables nested deeper than {MAX_DEPTH}"),
                    });
                } else {
                    stack.push(name.to_string());
                    let v = self.expand(value, loc, stack);
                    stack.pop();
                    out.push_str(&v);
                }
            } else {
                self.problem(Problem::UnresolvedVar {
                    name: name.to_string(),
                    location: loc.clone(),
                });
            }
            grown += out.len() - before;
            if (if top { grown } else { out.len() }) > MAX_OUTPUT || self.stop {
                self.too_large(loc);
                return out;
            }
        }
        out.push_str(&s[rest..]);
        if !top && out.len() > MAX_OUTPUT {
            self.too_large(loc);
        }
        out
    }

    fn run(&mut self) -> PreparedRequest {
        let url = self.build_url();
        let mut headers = self.merged_headers();
        let mut query_extra = Vec::new();
        self.apply_auth(&mut headers, &mut query_extra);
        let url = append_query(url, &query_extra);
        let body = self.build_body(&mut headers);
        let s = &self.req.settings;
        PreparedRequest {
            method: self.req.method,
            url,
            headers,
            body,
            timeout: s.timeout,
            client: ClientSettings {
                verify_tls: s.verify_tls,
                follow_redirects: s.follow_redirects,
                proxy: None,
            },
            warnings: std::mem::take(&mut self.warnings),
        }
    }

    // ---- URL ----

    fn build_url(&mut self) -> String {
        let url = self.subst(&self.req.url.clone(), &Location::Url);
        let url = self.apply_path_params(&url);
        let mut pairs = Vec::new();
        for p in self.req.params.iter().filter(|p| p.enabled) {
            let loc = Location::Param(p.name.clone());
            let n = self.subst(&p.name, &loc);
            let v = self.subst(&p.value, &loc);
            pairs.push((n, v));
        }
        append_query(url, &pairs)
    }

    fn apply_path_params(&mut self, url: &str) -> String {
        let pr = template::path_range_plain(url);
        let new_path = self.expand_path(&url[pr.clone()]);
        format!("{}{}{}", &url[..pr.start], new_path, &url[pr.end..])
    }

    fn expand_path(&mut self, path: &str) -> String {
        let mut out = String::new();
        let mut cache: HashMap<String, Option<String>> = HashMap::new();
        let mut last = 0usize;
        for tok in template::scan_path(path, false) {
            match tok {
                PathToken::StrayClose => self.problem(Problem::Syntax {
                    location: Location::Url,
                    message: "unbalanced `}` in path".into(),
                }),
                PathToken::Unclosed => {
                    self.problem(Problem::Syntax {
                        location: Location::Url,
                        message: "unbalanced `{` in path".into(),
                    });
                    return out;
                }
                PathToken::Param { range, inner } => {
                    out.push_str(&path[last..range.start]);
                    last = range.end;
                    out.push_str(&self.path_param(inner, &mut cache));
                }
            }
        }
        out.push_str(&path[last..]);
        out
    }

    fn path_param(&mut self, inner: &str, cache: &mut HashMap<String, Option<String>>) -> String {
        let Some((name, pattern, catch_all)) = template::parse_param(inner) else {
            self.problem(Problem::Syntax {
                location: Location::Url,
                message: format!("invalid path parameter `{{{inner}}}`"),
            });
            return String::new();
        };
        let name = name.as_str();
        let pattern = pattern.as_deref();
        let value = match cache.get(name) {
            Some(v) => v.clone(),
            None => {
                let raw = self
                    .req
                    .path
                    .iter()
                    .find(|v| v.name == name)
                    .map(|v| v.value.clone());
                let v = raw.map(|raw| self.subst(&raw, &Location::PathParam(name.to_string())));
                cache.insert(name.to_string(), v.clone());
                v
            }
        };
        let Some(value) = value else {
            self.problem(Problem::MissingPathParam {
                name: name.to_string(),
            });
            return String::new();
        };
        if let Some(pat) = pattern {
            // Validate the bare pattern first so stray parentheses cannot
            // break out of the anchoring group below.
            let compiled = Regex::new(pat).and_then(|_| Regex::new(&format!("^(?:{pat})$")));
            match compiled {
                Ok(re) => {
                    if !re.is_match(&value) {
                        self.warnings.push(ResolveWarning::PatternMismatch {
                            name: name.to_string(),
                            pattern: pat.to_string(),
                            value: value.clone(),
                        });
                    }
                }
                Err(e) => self.warnings.push(ResolveWarning::InvalidPattern {
                    name: name.to_string(),
                    pattern: pat.to_string(),
                    message: e.to_string(),
                }),
            }
        }
        if catch_all {
            value.split('/').map(encode).collect::<Vec<_>>().join("/")
        } else {
            encode(&value)
        }
    }

    // ---- headers ----

    fn merged_headers(&mut self) -> Vec<Header> {
        let mut out: Vec<Header> = Vec::new();
        let scope = self.scope;
        let levels: Vec<&[KeyValue]> = std::iter::once(scope.collection.headers.as_slice())
            .chain(scope.folders.iter().map(|f| f.headers.as_slice()))
            .chain(std::iter::once(self.req.headers.as_slice()))
            .collect();
        for level in levels {
            for h in level.iter().filter(|h| h.enabled) {
                let loc = Location::Header(h.name.clone());
                let name = self.subst(&h.name, &loc);
                let value = self.subst(&h.value, &loc);
                set_header(&mut out, name, value);
            }
        }
        out
    }

    // ---- auth ----

    fn apply_auth(&mut self, headers: &mut Vec<Header>, query: &mut Vec<(String, String)>) {
        let scope = self.scope;
        let auth = std::iter::once(&self.req.auth)
            .chain(scope.folders.iter().rev().map(|f| &f.auth))
            .chain(std::iter::once(&scope.collection.auth))
            .find(|a| !matches!(a, Auth::Inherit))
            .cloned()
            .unwrap_or(Auth::None);
        match auth {
            Auth::None | Auth::Inherit => {}
            Auth::Basic { username, password } => {
                let u = self.subst(&username, &Location::Auth);
                let p = self.subst(&password, &Location::Auth);
                let enc = STANDARD.encode(format!("{u}:{p}"));
                set_header(headers, "Authorization".into(), format!("Basic {enc}"));
            }
            Auth::Bearer { token } => {
                let t = self.subst(&token, &Location::Auth);
                set_header(headers, "Authorization".into(), format!("Bearer {t}"));
            }
            Auth::ApiKey {
                name,
                value,
                location,
            } => {
                let n = self.subst(&name, &Location::Auth);
                let v = self.subst(&value, &Location::Auth);
                match location {
                    ApiKeyLocation::Header => set_header(headers, n, v),
                    ApiKeyLocation::Query => query.push((n, v)),
                }
            }
        }
    }

    // ---- body ----

    fn text(&mut self, t: &TextBody) -> Vec<u8> {
        self.subst(&t.content, &Location::Body).into_bytes()
    }

    fn build_body(&mut self, headers: &mut Vec<Header>) -> PreparedBody {
        let (body, content_type) = match &self.req.body {
            Body::None => (PreparedBody::None, None),
            Body::Json(t) => (PreparedBody::Bytes(self.text(t)), Some("application/json")),
            Body::Text(t) => (
                PreparedBody::Bytes(self.text(t)),
                Some("text/plain; charset=utf-8"),
            ),
            Body::Xml(t) => (PreparedBody::Bytes(self.text(t)), Some("application/xml")),
            Body::Form(fields) => {
                let mut pairs = Vec::new();
                for f in fields.iter().filter(|f| f.enabled) {
                    let n = self.subst(&f.name, &Location::Body);
                    let v = self.subst(&f.value, &Location::Body);
                    pairs.push(format!("{}={}", encode_form(&n), encode_form(&v)));
                }
                (
                    PreparedBody::Bytes(pairs.join("&").into_bytes()),
                    Some("application/x-www-form-urlencoded"),
                )
            }
            Body::Multipart(parts) => {
                let mut out = Vec::new();
                for p in parts.iter().filter(|p| p.enabled) {
                    let value = match &p.value {
                        PartValue::Text(t) => {
                            PreparedPartValue::Text(self.subst(t, &Location::Body))
                        }
                        PartValue::File(f) => PreparedPartValue::File(f.clone()),
                    };
                    out.push(PreparedPart {
                        name: p.name.clone(),
                        value,
                    });
                }
                (PreparedBody::Multipart(out), None)
            }
            Body::File(p) => (PreparedBody::File(p.clone()), None),
        };
        if let Some(ct) = content_type
            && !headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("content-type"))
        {
            headers.push(Header {
                name: "Content-Type".into(),
                value: ct.into(),
            });
        }
        body
    }
}

/// Insert a header, replacing any existing one with the same name (case-insensitive).
fn set_header(headers: &mut Vec<Header>, name: String, value: String) {
    let new = Header { name, value };
    match headers
        .iter_mut()
        .find(|h| h.name.eq_ignore_ascii_case(&new.name))
    {
        Some(slot) => *slot = new,
        None => headers.push(new),
    }
}

/// Append percent-encoded pairs to the query, keeping any fragment last.
fn append_query(url: String, pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return url;
    }
    let (base, fragment) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url.as_str(), ""),
    };
    let joined = pairs
        .iter()
        .map(|(n, v)| format!("{}={}", encode(n), encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let sep = if !base.contains('?') {
        "?"
    } else if base.ends_with('?') || base.ends_with('&') {
        ""
    } else {
        "&"
    };
    format!("{base}{sep}{joined}{fragment}")
}
