//! A read-only explanation of the effective request: every value with the layer it came
//! from, and never a secret value. Built by a walker that mirrors `resolve()` (see the
//! parity test) but emits typed parts instead of expanded text.

use std::collections::{HashMap, HashSet};

use regex::Regex;

use super::engine::{
    MAX_DEPTH, MAX_EXPANSIONS, MAX_OUTPUT, VarSource, encode, encode_form, lookup, lookup_all,
};
use super::{Location, Problem, Scope};
use crate::model::{Method, redact_userinfo};
use crate::redact::is_credential_name;
use crate::request::{ApiKeyLocation, Auth, Body, KeyValue, PartValue, Request};
use crate::template::{self, PathToken};

/// Longest text value shown; longer values are `Hidden(TooLong)` (the hover limit).
const MAX_SHOWN_CHARS: usize = 200;

/// Stands in for a non-text part while a string is scanned.
const MARK: char = '\u{E000}';

/// Why a value is not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideReason {
    /// The field or variable name looks credential-like.
    Credential,
    /// URL userinfo (`user:password@`).
    Userinfo,
    /// More than 200 characters.
    TooLong,
}

/// One piece of display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShownPart {
    Text(String),
    /// A secret-sourced variable; its value is never read.
    Secret {
        name: String,
        resolved: bool,
    },
    /// `$uuid` or `$timestamp`; never generated.
    Dynamic {
        name: String,
    },
    Hidden(HideReason),
}

/// Display text as typed parts, so a user value can never imitate a marker.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown(pub Vec<ShownPart>);

impl Shown {
    /// The text when every part is plain text; `None` when any part is secret,
    /// dynamic or hidden.
    pub fn plain_text(&self) -> Option<String> {
        let mut out = String::new();
        for p in &self.0 {
            match p {
                ShownPart::Text(t) => out.push_str(t),
                _ => return None,
            }
        }
        Some(out)
    }
}

/// The layer a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Request,
    /// Index into `Scope::folders` (outermost = 0).
    Folder(usize),
    Collection,
    Environment,
    Secret,
    Dynamic,
    /// Synthesized from the body type (Content-Type).
    BodyType,
    /// Synthesized from the auth scheme.
    Auth,
}

impl From<VarSource> for Origin {
    fn from(s: VarSource) -> Self {
        match s {
            VarSource::Request => Origin::Request,
            VarSource::Folder(i) => Origin::Folder(i),
            VarSource::Environment => Origin::Environment,
            VarSource::Secret => Origin::Secret,
            VarSource::Collection => Origin::Collection,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretStatus {
    pub name: String,
    /// The secret has a non-empty value.
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvReport {
    pub name: String,
    pub secrets: Vec<SecretStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Query,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamReport {
    pub kind: ParamKind,
    pub name: Shown,
    pub value: Shown,
    pub origin: Origin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    None,
    Json,
    Text,
    Xml,
    Form,
    Multipart,
    File,
}

impl BodyKind {
    pub fn label(self) -> &'static str {
        match self {
            BodyKind::None => "none",
            BodyKind::Json => "JSON",
            BodyKind::Text => "text",
            BodyKind::Xml => "XML",
            BodyKind::Form => "form",
            BodyKind::Multipart => "multipart",
            BodyKind::File => "file",
        }
    }

    fn of(b: &Body) -> Self {
        match b {
            Body::None => BodyKind::None,
            Body::Json(_) => BodyKind::Json,
            Body::Text(_) => BodyKind::Text,
            Body::Xml(_) => BodyKind::Xml,
            Body::Form(_) => BodyKind::Form,
            Body::Multipart(_) => BodyKind::Multipart,
            Body::File(_) => BodyKind::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    None,
    Basic,
    Bearer,
    ApiKey,
    Digest,
    AwsSigV4,
    OAuth2ClientCredentials,
    OAuth2AuthCode,
}

impl AuthKind {
    pub fn label(self) -> &'static str {
        match self {
            AuthKind::None => "none",
            AuthKind::Basic => "Basic",
            AuthKind::Bearer => "Bearer",
            AuthKind::ApiKey => "API key",
            AuthKind::Digest => "Digest",
            AuthKind::AwsSigV4 => "AWS SigV4",
            AuthKind::OAuth2ClientCredentials => "OAuth2 client credentials",
            AuthKind::OAuth2AuthCode => "OAuth2 authorization code",
        }
    }

    fn of(a: &Auth) -> Self {
        match a {
            Auth::None | Auth::Inherit => AuthKind::None,
            Auth::Basic { .. } => AuthKind::Basic,
            Auth::Bearer { .. } => AuthKind::Bearer,
            Auth::ApiKey { .. } => AuthKind::ApiKey,
            Auth::Digest { .. } => AuthKind::Digest,
            Auth::AwsSigV4 { .. } => AuthKind::AwsSigV4,
            Auth::OAuth2ClientCredentials { .. } => AuthKind::OAuth2ClientCredentials,
            Auth::OAuth2AuthCode { .. } => AuthKind::OAuth2AuthCode,
        }
    }
}

/// What a synthetic header was made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synthetic {
    BodyType(BodyKind),
    Auth(AuthKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderReport {
    pub name: Shown,
    pub value: Shown,
    pub origin: Origin,
    /// Layers whose header of the same name was replaced, lowest first.
    pub overridden: Vec<Origin>,
    pub synthetic: Option<Synthetic>,
}

/// How the effective auth reaches the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthApplication {
    None,
    /// As this header.
    Header(Shown),
    /// As this query parameter.
    Query(Shown),
    /// By the engine at send time (Digest, SigV4, OAuth2).
    AtSend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthField {
    pub name: &'static str,
    /// Secret-bearing fields are `Hidden(Credential)`.
    pub value: Shown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthReport {
    pub kind: AuthKind,
    /// `None` when no layer sets auth.
    pub origin: Option<Origin>,
    pub application: AuthApplication,
    pub fields: Vec<AuthField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingSource {
    /// Differs from the default.
    Request,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub value: String,
    pub source: SettingSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsReport {
    pub timeout: Setting,
    pub follow_redirects: Setting,
    pub verify_tls: Setting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyReport {
    pub kind: BodyKind,
    /// Resolved byte length. `None` when it depends on a secret or dynamic value,
    /// or on a file. Multipart counts text parts only.
    pub bytes: Option<usize>,
    /// Variable names used by the body.
    pub vars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarReport {
    pub name: String,
    pub origin: Origin,
    pub value: Shown,
    /// Layers that define the same name and lose, in precedence order.
    pub shadowed: Vec<Origin>,
    pub locations: Vec<Location>,
}

/// A non-fatal finding, without the offending value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportWarning {
    PatternMismatch {
        name: String,
        pattern: String,
    },
    InvalidPattern {
        name: String,
        pattern: String,
        message: String,
    },
}

/// The explanation of one request. Not serializable on purpose.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionReport {
    pub environment: Option<EnvReport>,
    pub method: Method,
    pub url: Shown,
    pub params: Vec<ParamReport>,
    pub headers: Vec<HeaderReport>,
    pub auth: AuthReport,
    pub settings: SettingsReport,
    pub body: BodyReport,
    pub variables: Vec<VarReport>,
    pub problems: Vec<Problem>,
    pub warnings: Vec<ReportWarning>,
}

/// Explain `req` in `scope`.
pub fn explain(req: &Request, scope: &Scope) -> ResolutionReport {
    let mut w = Walker::new(req, scope);
    let mut params = Vec::new();
    let url = w.build_url(&mut params);
    let mut headers = w.merged_headers();
    let (auth, effect) = w.auth();
    if let Some((name, value)) = effect.header {
        set_header(
            &mut headers,
            &name,
            value,
            Origin::Auth,
            Some(Synthetic::Auth(auth.kind)),
        );
    }
    let query_extra: Vec<_> = effect.query.into_iter().collect();
    let url = finish_url(&append_query(url, &query_extra).parts);
    let body = w.build_body(&mut headers);
    let s = &req.settings;
    let settings = SettingsReport {
        timeout: match s.timeout {
            Some(d) => Setting {
                value: format!("{d:?}"),
                source: SettingSource::Request,
            },
            None => Setting {
                value: "default".into(),
                source: SettingSource::Default,
            },
        },
        follow_redirects: bool_setting(s.follow_redirects, true),
        verify_tls: bool_setting(s.verify_tls, true),
    };
    let variables = w.variables();
    ResolutionReport {
        environment: scope.environment.map(|e| EnvReport {
            name: e.name.clone(),
            secrets: e
                .secrets
                .iter()
                .map(|s| SecretStatus {
                    name: s.name.clone(),
                    present: !s.value.is_empty(),
                })
                .collect(),
        }),
        method: req.method,
        url,
        params,
        headers: headers.into_iter().map(|h| h.rep).collect(),
        auth,
        settings,
        body,
        variables,
        problems: w.problems,
        warnings: w.warnings,
    }
}

/// Explain only the effective auth. Expands the auth fields and nothing else.
pub fn explain_auth(req: &Request, scope: &Scope) -> AuthReport {
    Walker::new(req, scope).auth().0
}

fn bool_setting(v: bool, default: bool) -> Setting {
    Setting {
        value: v.to_string(),
        source: if v == default {
            SettingSource::Default
        } else {
            SettingSource::Request
        },
    }
}

// ---- parts ----

/// Parts under construction, with the byte length of what the engine would have built.
#[derive(Default, Clone)]
struct Acc {
    parts: Vec<ShownPart>,
    len: usize,
    /// For each `Hidden` part in order, what it stands for (a variable name), so two
    /// different hidden names do not look the same.
    hidden_ids: Vec<String>,
}

impl Acc {
    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.len += s.len();
        match self.parts.last_mut() {
            Some(ShownPart::Text(t)) => t.push_str(s),
            _ => self.parts.push(ShownPart::Text(s.to_string())),
        }
    }

    fn part(&mut self, p: ShownPart) {
        match p {
            ShownPart::Text(t) => self.text(&t),
            other => self.parts.push(other),
        }
    }

    fn append(&mut self, o: &Acc) {
        let before = self.len;
        for p in &o.parts {
            self.part(p.clone());
        }
        self.len = before + o.len;
        self.hidden_ids.extend(o.hidden_ids.iter().cloned());
    }

    /// Stand in for `o` with a hidden marker, keeping its size for the bounds.
    fn hide(&mut self, o: &Acc, id: &str) {
        self.parts.push(ShownPart::Hidden(HideReason::Credential));
        self.hidden_ids.push(id.to_string());
        self.len += o.len;
    }

    fn has_non_text(&self) -> bool {
        self.parts.iter().any(|p| !matches!(p, ShownPart::Text(_)))
    }

    fn bytes(&self) -> Option<usize> {
        (!self.has_non_text()).then_some(self.len)
    }

    /// The text parts only, for name checks.
    fn text_only(&self) -> String {
        text_only(&self.parts)
    }

    /// Identity for header-name matching.
    fn key(&self) -> String {
        let mut k = String::new();
        let mut hidden = 0usize;
        for p in &self.parts {
            match p {
                ShownPart::Text(t) => k.push_str(&t.to_ascii_lowercase()),
                ShownPart::Secret { name, .. } => {
                    k.push(MARK);
                    k.push_str("s:");
                    k.push_str(name);
                }
                ShownPart::Dynamic { name } => {
                    k.push(MARK);
                    k.push_str("d:");
                    k.push_str(name);
                }
                ShownPart::Hidden(_) => {
                    k.push(MARK);
                    k.push_str("h:");
                    k.push_str(self.hidden_ids.get(hidden).map_or("", String::as_str));
                    hidden += 1;
                }
            }
        }
        k
    }
}

fn text_only(parts: &[ShownPart]) -> String {
    parts
        .iter()
        .filter_map(|p| match p {
            ShownPart::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// Apply the display rules: credential-named values and over-long values are hidden.
fn finish(parts: &[ShownPart], name: Option<&str>) -> Shown {
    if parts.is_empty() {
        return Shown::default();
    }
    if name.is_some_and(is_credential_name) {
        return Shown(vec![ShownPart::Hidden(HideReason::Credential)]);
    }
    let chars: usize = parts
        .iter()
        .map(|p| match p {
            ShownPart::Text(t) => t.chars().count(),
            _ => 0,
        })
        .sum();
    if chars > MAX_SHOWN_CHARS {
        return Shown(vec![ShownPart::Hidden(HideReason::TooLong)]);
    }
    Shown(parts.to_vec())
}

/// `finish` for a value whose name is `name`: hidden when the name is credential-like
/// or holds any hidden or secret part (its text cannot be checked).
fn finish_for(parts: &[ShownPart], name: &[ShownPart]) -> Shown {
    let opaque = name
        .iter()
        .any(|p| matches!(p, ShownPart::Hidden(_) | ShownPart::Secret { .. }));
    if opaque && !parts.is_empty() {
        return hidden_credential();
    }
    finish(parts, Some(&text_only(name)))
}

/// Hide the value of every credential-named (or opaquely named) pair in the query.
fn hide_query_values(parts: &[ShownPart]) -> Vec<ShownPart> {
    let (flat, rest) = flatten(parts);
    let Some(q) = flat.find('?') else {
        return parts.to_vec();
    };
    let qend = flat[q..].find('#').map_or(flat.len(), |i| q + i);
    let mut cur = Cursor {
        parts: rest.into_iter(),
    };
    let mut out = Acc::default();
    cur.append(&mut out, &flat[..=q]);
    for (i, pair) in flat[q + 1..qend].split('&').enumerate() {
        if i > 0 {
            out.text("&");
        }
        match pair.split_once('=') {
            Some((n, v)) if is_credential_name(n) || n.contains(MARK) => {
                cur.append(&mut out, n);
                out.text("=");
                cur.skip(v);
                if !v.is_empty() {
                    out.part(ShownPart::Hidden(HideReason::Credential));
                }
            }
            _ => cur.append(&mut out, pair),
        }
    }
    cur.append(&mut out, &flat[qend..]);
    out.parts
}

/// `finish`, plus hiding URL userinfo and credential-named query values.
fn finish_url(parts: &[ShownPart]) -> Shown {
    let parts = &hide_query_values(parts);
    let (flat, _) = flatten(parts);
    if redact_userinfo(&flat) == flat {
        return finish(parts, None);
    }
    let Some(cut) = userinfo_cut(&flat) else {
        return finish(parts, None);
    };
    let mut out = vec![ShownPart::Hidden(HideReason::Userinfo)];
    let mut pos = 0usize;
    for p in parts {
        match p {
            ShownPart::Text(t) => {
                let end = pos + t.len();
                if pos >= cut {
                    out.push(p.clone());
                } else if end > cut {
                    out.push(ShownPart::Text(t[cut - pos..].to_string()));
                }
                pos = end;
            }
            other => {
                if pos >= cut {
                    out.push(other.clone());
                }
                pos += MARK.len_utf8();
            }
        }
    }
    finish(&out, None)
}

/// Byte offset just past the `@` that ends the userinfo of `flat`, if any.
fn userinfo_cut(flat: &str) -> Option<usize> {
    let start = flat.find("://").map_or(0, |i| i + 3);
    let rest = &flat[start..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    rest[..end].rfind('@').map(|a| start + a + 1)
}

/// Text with a `MARK` per non-text part, and those parts in order. A literal `MARK`
/// in text becomes U+FFFD (the same width).
fn flatten(parts: &[ShownPart]) -> (String, Vec<ShownPart>) {
    let mut s = String::new();
    let mut rest = Vec::new();
    for p in parts {
        match p {
            ShownPart::Text(t) => s.push_str(&t.replace(MARK, "\u{FFFD}")),
            other => {
                s.push(MARK);
                rest.push(other.clone());
            }
        }
    }
    (s, rest)
}

/// Rebuilds parts from slices of a flattened string.
struct Cursor {
    parts: std::vec::IntoIter<ShownPart>,
}

impl Cursor {
    fn append(&mut self, out: &mut Acc, text: &str) {
        for (i, seg) in text.split(MARK).enumerate() {
            if i > 0
                && let Some(p) = self.parts.next()
            {
                out.part(p);
            }
            out.text(seg);
        }
    }

    fn skip(&mut self, text: &str) {
        for _ in text.matches(MARK) {
            self.parts.next();
        }
    }
}

type Pair = (Vec<ShownPart>, Vec<ShownPart>);

fn encode_into(out: &mut Acc, parts: &[ShownPart], f: fn(&str) -> String) {
    for p in parts {
        match p {
            ShownPart::Text(t) => out.text(&f(t)),
            other => out.part(other.clone()),
        }
    }
}

/// Append encoded pairs to the query, keeping any fragment last (mirrors the engine).
fn append_query(url: Acc, pairs: &[Pair]) -> Acc {
    if pairs.is_empty() {
        return url;
    }
    let mut base = Vec::new();
    let mut frag = Vec::new();
    for p in url.parts {
        if !frag.is_empty() {
            frag.push(p);
            continue;
        }
        match &p {
            ShownPart::Text(t) if t.contains('#') => {
                let i = t.find('#').unwrap_or(0);
                if i > 0 {
                    base.push(ShownPart::Text(t[..i].to_string()));
                }
                frag.push(ShownPart::Text(t[i..].to_string()));
            }
            _ => base.push(p),
        }
    }
    let has_q = base
        .iter()
        .any(|p| matches!(p, ShownPart::Text(t) if t.contains('?')));
    let ends =
        matches!(base.last(), Some(ShownPart::Text(t)) if t.ends_with('?') || t.ends_with('&'));
    let mut out = Acc::default();
    for p in base {
        out.part(p);
    }
    out.text(if !has_q {
        "?"
    } else if ends {
        ""
    } else {
        "&"
    });
    for (i, (n, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.text("&");
        }
        encode_into(&mut out, n, encode);
        out.text("=");
        encode_into(&mut out, v, encode);
    }
    for p in frag {
        out.part(p);
    }
    out
}

// ---- headers ----

struct HeaderBuild {
    key: String,
    rep: HeaderReport,
}

/// Insert a header, replacing any with the same name (case-insensitive) and recording it.
fn set_header(
    out: &mut Vec<HeaderBuild>,
    name: &Acc,
    value: Shown,
    origin: Origin,
    synthetic: Option<Synthetic>,
) {
    let key = name.key();
    let mut rep = HeaderReport {
        name: finish(&name.parts, None),
        value,
        origin,
        overridden: Vec::new(),
        synthetic,
    };
    match out.iter_mut().find(|h| h.key == key) {
        Some(slot) => {
            let mut ov = std::mem::take(&mut slot.rep.overridden);
            ov.push(slot.rep.origin);
            rep.overridden = ov;
            *slot = HeaderBuild { key, rep };
        }
        None => out.push(HeaderBuild { key, rep }),
    }
}

// ---- walker ----

struct VarUse {
    name: String,
    locations: Vec<Location>,
}

/// What the effective auth adds to the request.
#[derive(Default)]
struct AuthEffect {
    header: Option<(Acc, Shown)>,
    query: Option<Pair>,
}

#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    Plain,
    Sensitive,
    Username,
    Url,
}

struct Walker<'a> {
    req: &'a Request,
    scope: &'a Scope<'a>,
    problems: Vec<Problem>,
    warnings: Vec<ReportWarning>,
    expansions: usize,
    stop: bool,
    seen: HashSet<String>,
    uses: Vec<VarUse>,
    use_idx: HashMap<String, usize>,
}

struct CachedParam {
    shown: Vec<ShownPart>,
    plain: Option<String>,
}

impl<'a> Walker<'a> {
    fn new(req: &'a Request, scope: &'a Scope<'a>) -> Self {
        Walker {
            req,
            scope,
            problems: Vec::new(),
            warnings: Vec::new(),
            expansions: 0,
            stop: false,
            seen: HashSet::new(),
            uses: Vec::new(),
            use_idx: HashMap::new(),
        }
    }

    fn problem(&mut self, p: Problem) {
        if self.seen.insert(format!("{p:?}")) {
            self.problems.push(p);
        }
    }

    fn note_use(&mut self, name: &str, loc: &Location) {
        let i = match self.use_idx.get(name) {
            Some(i) => *i,
            None => {
                self.uses.push(VarUse {
                    name: name.to_string(),
                    locations: Vec::new(),
                });
                self.use_idx.insert(name.to_string(), self.uses.len() - 1);
                self.uses.len() - 1
            }
        };
        if !self.uses[i].locations.contains(loc) {
            self.uses[i].locations.push(loc.clone());
        }
    }

    fn subst(&mut self, s: &str, loc: &Location) -> Acc {
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

    /// Mirrors `Resolver::expand`, except that secret values are never read and dynamic
    /// values are never generated.
    fn expand(&mut self, s: &str, loc: &Location, stack: &mut Vec<String>) -> Acc {
        let top = stack.is_empty();
        let mut out = Acc::default();
        let mut grown = 0usize;
        let mut rest = 0usize;
        for var in template::var_refs(s) {
            if self.stop {
                return out;
            }
            out.text(&s[rest..var.range.start]);
            if !var.terminated {
                self.problem(Problem::Syntax {
                    location: loc.clone(),
                    message: "unclosed `{{`".into(),
                });
                out.text(&s[var.range.start..]);
                return out;
            }
            let name = var.name.as_str();
            rest = var.range.end;
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
            let before = out.len;
            if let Some(dynamic) = name.strip_prefix('$') {
                if matches!(dynamic, "uuid" | "timestamp") {
                    self.note_use(name, loc);
                    out.part(ShownPart::Dynamic {
                        name: name.to_string(),
                    });
                } else {
                    self.problem(Problem::UnresolvedVar {
                        name: name.to_string(),
                        location: loc.clone(),
                    });
                }
            } else if let Some((value, source)) = lookup(self.req, self.scope, name) {
                self.note_use(name, loc);
                if matches!(source, VarSource::Secret) {
                    out.part(ShownPart::Secret {
                        name: name.to_string(),
                        resolved: !value.is_empty(),
                    });
                } else if stack.iter().any(|n| n == name) {
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
                    if is_credential_name(name) {
                        out.hide(&v, name);
                    } else {
                        out.append(&v);
                    }
                }
            } else {
                self.problem(Problem::UnresolvedVar {
                    name: name.to_string(),
                    location: loc.clone(),
                });
            }
            grown += out.len - before;
            if (if top { grown } else { out.len }) > MAX_OUTPUT || self.stop {
                self.too_large(loc);
                return out;
            }
        }
        out.text(&s[rest..]);
        if !top && out.len > MAX_OUTPUT {
            self.too_large(loc);
        }
        out
    }

    // ---- URL ----

    fn build_url(&mut self, params: &mut Vec<ParamReport>) -> Acc {
        let req = self.req;
        let url = self.subst(&req.url, &Location::Url);
        let url = self.apply_path_params(url, params);
        let mut pairs: Vec<Pair> = Vec::new();
        for p in req.params.iter().filter(|p| p.enabled) {
            let loc = Location::Param(p.name.clone());
            let n = self.subst(&p.name, &loc);
            let v = self.subst(&p.value, &loc);
            let name = finish(&n.parts, None);
            let value = finish_for(&v.parts, &n.parts);
            params.push(ParamReport {
                kind: ParamKind::Query,
                name: name.clone(),
                value: value.clone(),
                origin: Origin::Request,
            });
            pairs.push((name.0, value.0));
        }
        append_query(url, &pairs)
    }

    fn apply_path_params(&mut self, url: Acc, params: &mut Vec<ParamReport>) -> Acc {
        let (flat, parts) = flatten(&url.parts);
        let pr = template::path_range_plain(&flat);
        let mut cur = Cursor {
            parts: parts.into_iter(),
        };
        let mut out = Acc::default();
        cur.append(&mut out, &flat[..pr.start]);
        self.expand_path(&flat[pr.clone()], &mut cur, &mut out, params);
        cur.append(&mut out, &flat[pr.end..]);
        out
    }

    fn expand_path(
        &mut self,
        path: &str,
        cur: &mut Cursor,
        out: &mut Acc,
        params: &mut Vec<ParamReport>,
    ) {
        let mut cache: HashMap<String, Option<CachedParam>> = HashMap::new();
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
                    cur.skip(&path[last..]);
                    return;
                }
                PathToken::Param { range, inner } => {
                    cur.append(out, &path[last..range.start]);
                    cur.skip(&path[range.clone()]);
                    last = range.end;
                    let v = self.path_param(inner, &mut cache, params);
                    out.append(&v);
                }
            }
        }
        cur.append(out, &path[last..]);
    }

    fn path_param(
        &mut self,
        inner: &str,
        cache: &mut HashMap<String, Option<CachedParam>>,
        params: &mut Vec<ParamReport>,
    ) -> Acc {
        let mut out = Acc::default();
        let Some((name, pattern, catch_all)) = template::parse_param(inner) else {
            self.problem(Problem::Syntax {
                location: Location::Url,
                message: format!("invalid path parameter `{{{inner}}}`"),
            });
            return out;
        };
        let name = name.as_str();
        if !cache.contains_key(name) {
            let req = self.req;
            let v = req.path.iter().find(|v| v.name == name).map(|v| {
                let acc = self.subst(&v.value, &Location::PathParam(name.to_string()));
                let shown = finish(&acc.parts, Some(name));
                params.push(ParamReport {
                    kind: ParamKind::Path,
                    name: Shown(vec![ShownPart::Text(name.to_string())]),
                    value: shown.clone(),
                    origin: Origin::Request,
                });
                CachedParam {
                    shown: shown.0,
                    plain: (!acc.has_non_text()).then(|| text_only(&acc.parts)),
                }
            });
            cache.insert(name.to_string(), v);
        }
        let Some(Some(value)) = cache.get(name) else {
            self.problem(Problem::MissingPathParam {
                name: name.to_string(),
            });
            return out;
        };
        if let (Some(pat), Some(plain)) = (pattern.as_deref(), value.plain.as_deref()) {
            let compiled = Regex::new(pat).and_then(|_| Regex::new(&format!("^(?:{pat})$")));
            match compiled {
                Ok(re) => {
                    if !re.is_match(plain) {
                        self.warnings.push(ReportWarning::PatternMismatch {
                            name: name.to_string(),
                            pattern: pat.to_string(),
                        });
                    }
                }
                Err(e) => self.warnings.push(ReportWarning::InvalidPattern {
                    name: name.to_string(),
                    pattern: pat.to_string(),
                    message: e.to_string(),
                }),
            }
        }
        for p in &value.shown {
            match p {
                ShownPart::Text(t) if catch_all => {
                    let enc = t.split('/').map(encode).collect::<Vec<_>>().join("/");
                    out.text(&enc);
                }
                ShownPart::Text(t) => out.text(&encode(t)),
                other => out.part(other.clone()),
            }
        }
        out
    }

    // ---- headers ----

    fn merged_headers(&mut self) -> Vec<HeaderBuild> {
        let mut out: Vec<HeaderBuild> = Vec::new();
        let scope = self.scope;
        let mut levels: Vec<(Origin, &[KeyValue])> =
            vec![(Origin::Collection, scope.collection.headers.as_slice())];
        for (i, f) in scope.folders.iter().enumerate() {
            levels.push((Origin::Folder(i), f.headers.as_slice()));
        }
        levels.push((Origin::Request, self.req.headers.as_slice()));
        for (origin, level) in levels {
            for h in level.iter().filter(|h| h.enabled) {
                let loc = Location::Header(h.name.clone());
                let name = self.subst(&h.name, &loc);
                let value = self.subst(&h.value, &loc);
                let shown = finish_for(&value.parts, &name.parts);
                set_header(&mut out, &name, shown, origin, None);
            }
        }
        out
    }

    // ---- auth ----

    fn effective_auth(&self) -> (Auth, Option<Origin>) {
        let found = std::iter::once((&self.req.auth, Origin::Request))
            .chain(
                self.scope
                    .folders
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, f)| (&f.auth, Origin::Folder(i))),
            )
            .chain(std::iter::once((
                &self.scope.collection.auth,
                Origin::Collection,
            )))
            .find(|(a, _)| !matches!(a, Auth::Inherit));
        match found {
            Some((a, o)) => (a.clone(), Some(o)),
            None => (Auth::None, None),
        }
    }

    fn field(
        &mut self,
        fields: &mut Vec<AuthField>,
        name: &'static str,
        raw: &str,
        kind: FieldKind,
    ) -> Acc {
        let acc = self.subst(raw, &Location::Auth);
        let value = match kind {
            FieldKind::Plain => finish(&acc.parts, None),
            FieldKind::Url => finish_url(&acc.parts),
            FieldKind::Sensitive | FieldKind::Username if acc.parts.is_empty() => Shown::default(),
            FieldKind::Username => Shown(vec![ShownPart::Hidden(HideReason::Userinfo)]),
            FieldKind::Sensitive => Shown(vec![ShownPart::Hidden(HideReason::Credential)]),
        };
        fields.push(AuthField { name, value });
        acc
    }

    fn auth(&mut self) -> (AuthReport, AuthEffect) {
        use FieldKind::{Plain, Sensitive, Url, Username};
        let (auth, origin) = self.effective_auth();
        let kind = AuthKind::of(&auth);
        let mut f = Vec::new();
        let mut effect = AuthEffect::default();
        let text = |s: &str| {
            let mut a = Acc::default();
            a.text(s);
            a
        };
        let application = match &auth {
            Auth::None | Auth::Inherit => AuthApplication::None,
            Auth::Basic { username, password } => {
                self.field(&mut f, "username", username, Username);
                self.field(&mut f, "password", password, Sensitive);
                let name = text("Authorization");
                let app = AuthApplication::Header(finish(&name.parts, None));
                effect.header = Some((name, hidden_credential()));
                app
            }
            Auth::Bearer { token } => {
                self.field(&mut f, "token", token, Sensitive);
                let name = text("Authorization");
                let app = AuthApplication::Header(finish(&name.parts, None));
                effect.header = Some((name, hidden_credential()));
                app
            }
            Auth::ApiKey {
                name,
                value,
                location,
            } => {
                let n = self.field(&mut f, "name", name, Plain);
                let v = self.field(&mut f, "value", value, Sensitive);
                let nshown = finish(&n.parts, None);
                let vshown = if v.parts.is_empty() {
                    Shown::default()
                } else {
                    hidden_credential()
                };
                match location {
                    ApiKeyLocation::Header => {
                        effect.header = Some((n, vshown));
                        AuthApplication::Header(nshown)
                    }
                    ApiKeyLocation::Query => {
                        effect.query = Some((nshown.0.clone(), vshown.0));
                        AuthApplication::Query(nshown)
                    }
                }
            }
            Auth::Digest { username, password } => {
                self.field(&mut f, "username", username, Username);
                self.field(&mut f, "password", password, Sensitive);
                AuthApplication::AtSend
            }
            Auth::AwsSigV4 {
                access_key,
                secret_key,
                session_token,
                region,
                service,
            } => {
                self.field(&mut f, "access_key", access_key, Sensitive);
                self.field(&mut f, "secret_key", secret_key, Sensitive);
                self.field(&mut f, "session_token", session_token, Sensitive);
                self.field(&mut f, "region", region, Plain);
                self.field(&mut f, "service", service, Plain);
                AuthApplication::AtSend
            }
            Auth::OAuth2ClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
                ..
            } => {
                self.field(&mut f, "token_url", token_url, Url);
                self.field(&mut f, "client_id", client_id, Plain);
                self.field(&mut f, "client_secret", client_secret, Sensitive);
                self.field(&mut f, "scope", scope, Plain);
                AuthApplication::AtSend
            }
            Auth::OAuth2AuthCode {
                auth_url,
                token_url,
                client_id,
                client_secret,
                scope,
                ..
            } => {
                self.field(&mut f, "auth_url", auth_url, Url);
                self.field(&mut f, "token_url", token_url, Url);
                self.field(&mut f, "client_id", client_id, Plain);
                self.field(&mut f, "client_secret", client_secret, Sensitive);
                self.field(&mut f, "scope", scope, Plain);
                AuthApplication::AtSend
            }
        };
        (
            AuthReport {
                kind,
                origin,
                application,
                fields: f,
            },
            effect,
        )
    }

    // ---- body ----

    fn build_body(&mut self, headers: &mut Vec<HeaderBuild>) -> BodyReport {
        let req = self.req;
        let loc = Location::Body;
        let (bytes, content_type) = match &req.body {
            Body::None => (Some(0), None),
            Body::Json(t) => (
                self.subst(&t.content, &loc).bytes(),
                Some("application/json"),
            ),
            Body::Text(t) => (
                self.subst(&t.content, &loc).bytes(),
                Some("text/plain; charset=utf-8"),
            ),
            Body::Xml(t) => (
                self.subst(&t.content, &loc).bytes(),
                Some("application/xml"),
            ),
            Body::Form(fields) => {
                let mut total = Some(0usize);
                let mut count = 0usize;
                for f in fields.iter().filter(|f| f.enabled) {
                    let n = self.subst(&f.name, &loc);
                    let v = self.subst(&f.value, &loc);
                    count += 1;
                    total = match (total, n.bytes(), v.bytes()) {
                        (Some(t), Some(_), Some(_)) => Some(
                            t + encode_form(&n.text_only()).len()
                                + 1
                                + encode_form(&v.text_only()).len(),
                        ),
                        _ => None,
                    };
                }
                let total = total.map(|t| t + count.saturating_sub(1));
                (total, Some("application/x-www-form-urlencoded"))
            }
            Body::Multipart(parts) => {
                let mut total = Some(0usize);
                for p in parts.iter().filter(|p| p.enabled) {
                    if let PartValue::Text(t) = &p.value {
                        let v = self.subst(t, &loc);
                        total = total.zip(v.bytes()).map(|(a, b)| a + b);
                    }
                }
                (total, None)
            }
            Body::File(_) => (None, None),
        };
        if let Some(ct) = content_type
            && !headers.iter().any(|h| h.key == "content-type")
        {
            let mut name = Acc::default();
            name.text("Content-Type");
            let mut value = Acc::default();
            value.text(ct);
            set_header(
                headers,
                &name,
                finish(&value.parts, None),
                Origin::BodyType,
                Some(Synthetic::BodyType(BodyKind::of(&req.body))),
            );
        }
        BodyReport {
            kind: BodyKind::of(&req.body),
            bytes,
            vars: self
                .uses
                .iter()
                .filter(|u| u.locations.contains(&Location::Body))
                .map(|u| u.name.clone())
                .collect(),
        }
    }

    // ---- variables ----

    fn variables(&self) -> Vec<VarReport> {
        let mut out = Vec::new();
        for u in &self.uses {
            if u.name.starts_with('$') {
                out.push(VarReport {
                    name: u.name.clone(),
                    origin: Origin::Dynamic,
                    value: Shown(vec![ShownPart::Dynamic {
                        name: u.name.clone(),
                    }]),
                    shadowed: Vec::new(),
                    locations: u.locations.clone(),
                });
                continue;
            }
            let all = lookup_all(self.req, self.scope, &u.name);
            let Some((raw, source)) = all.first().cloned() else {
                continue;
            };
            let value = if matches!(source, VarSource::Secret) {
                Shown(vec![ShownPart::Secret {
                    name: u.name.clone(),
                    resolved: !raw.is_empty(),
                }])
            } else {
                // A throwaway walker: its problems are already reported at the use sites.
                let mut w = Walker::new(self.req, self.scope);
                let acc = w.expand(
                    raw,
                    &Location::Var(u.name.clone()),
                    &mut vec![u.name.clone()],
                );
                finish(&acc.parts, Some(&u.name))
            };
            out.push(VarReport {
                name: u.name.clone(),
                origin: source.into(),
                value,
                shadowed: all.into_iter().skip(1).map(|(_, s)| s.into()).collect(),
                locations: u.locations.clone(),
            });
        }
        out
    }
}

fn hidden_credential() -> Shown {
    Shown(vec![ShownPart::Hidden(HideReason::Credential)])
}
