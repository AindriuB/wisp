//! Declarative, report-only response assertions and their pure evaluator.
//!
//! Expected values are literals (no `{{var}}` expansion). Results never leave the process
//! except as a label, a definition fingerprint and a state; the actual value is for the
//! results pane only, so every type that can hold one has a hand-written `Debug`.

use std::fmt;

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Header;

/// Largest body a JSON assertion will parse (1 MiB; storage pins it equal to its `BODY_CAP`).
pub const MAX_JSON_BODY: usize = 1024 * 1024;
/// Assertions evaluated per request; later entries are skipped.
pub const MAX_ASSERTIONS: usize = 50;
/// Longest regex pattern, in characters.
pub const MAX_REGEX_LEN: usize = 256;
/// Compiled-size limit for a regex, in bytes.
const REGEX_SIZE_LIMIT: usize = 256 * 1024;
/// Longest JSON path, in characters.
pub const MAX_PATH_LEN: usize = 256;
/// Most segments in a JSON path.
pub const MAX_PATH_SEGMENTS: usize = 16;
/// Longest actual value kept in a failure, in characters.
pub const MAX_ACTUAL_LEN: usize = 80;
/// Longest header name or path shown in a label, in characters.
const MAX_LABEL_TARGET: usize = 200;

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Equals,
    Contains,
    Matches,
    Exists,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Equals => "equals",
            Op::Contains => "contains",
            Op::Matches => "matches",
            Op::Exists => "exists",
        }
    }

    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "equals" => Op::Equals,
            "contains" => Op::Contains,
            "matches" => Op::Matches,
            "exists" => Op::Exists,
            _ => return None,
        })
    }
}

/// An expected value: a typed literal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expected {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// An authored assertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Assertion {
    Status {
        op: Op,
        expected: Expected,
    },
    Header {
        name: String,
        op: Op,
        expected: Expected,
    },
    Json {
        path: String,
        op: Op,
        expected: Expected,
    },
    /// An entry this version cannot use, kept as written. `reason` is `"not supported"` or
    /// `"malformed"`.
    Unrecognised {
        raw: String,
        reason: String,
    },
}

/// Why an `[[assert]]` entry could not become a typed [`Assertion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    NotSupported,
    Malformed,
}

impl ParseError {
    pub fn reason(self) -> &'static str {
        match self {
            ParseError::NotSupported => "not supported",
            ParseError::Malformed => "malformed",
        }
    }
}

impl Assertion {
    /// Build a typed assertion from file parts, applying the validity matrix.
    pub fn from_parts(
        kind: &str,
        target: Option<&str>,
        op: &str,
        value: Option<Expected>,
    ) -> Result<Assertion, ParseError> {
        if !matches!(kind, "status" | "header" | "json") {
            return Err(ParseError::NotSupported);
        }
        let op = Op::parse(op).ok_or(ParseError::NotSupported)?;
        let bad = ParseError::Malformed;
        let exists_value = |v: Option<Expected>| match v {
            None => Ok(Expected::Bool(true)),
            Some(Expected::Bool(b)) => Ok(Expected::Bool(b)),
            Some(_) => Err(bad),
        };
        let string_value = |v: Option<Expected>| match v {
            Some(e @ Expected::Str(_)) => Ok(e),
            _ => Err(bad),
        };
        match kind {
            "status" => {
                if target.is_some() {
                    return Err(bad);
                }
                let expected = match op {
                    Op::Equals => match value {
                        Some(e @ Expected::Int(n)) if (100..=599).contains(&n) => e,
                        _ => return Err(bad),
                    },
                    Op::Matches => string_value(value)?,
                    _ => return Err(bad),
                };
                Ok(Assertion::Status { op, expected })
            }
            "header" => {
                let name = target.filter(|n| !n.is_empty()).ok_or(bad)?.to_string();
                let expected = match op {
                    Op::Exists => exists_value(value)?,
                    _ => string_value(value)?,
                };
                Ok(Assertion::Header { name, op, expected })
            }
            _ => {
                let path = target.ok_or(bad)?.to_string();
                let expected = match op {
                    Op::Exists => exists_value(value)?,
                    Op::Matches => string_value(value)?,
                    Op::Equals | Op::Contains => value.ok_or(bad)?,
                };
                Ok(Assertion::Json { path, op, expected })
            }
        }
    }

    /// The `kind` key as written in a file; `None` for an unrecognised entry.
    pub fn kind(&self) -> Option<&'static str> {
        match self {
            Assertion::Status { .. } => Some("status"),
            Assertion::Header { .. } => Some("header"),
            Assertion::Json { .. } => Some("json"),
            Assertion::Unrecognised { .. } => None,
        }
    }

    /// A short label such as "status equals" or "header content-type contains".
    pub fn label(&self) -> String {
        match self {
            Assertion::Status { op, .. } => format!("status {}", op.as_str()),
            Assertion::Header { name, op, .. } => {
                format!("header {} {}", cap(name, MAX_LABEL_TARGET), op.as_str())
            }
            Assertion::Json { path, op, .. } => {
                format!("json {} {}", cap(path, MAX_LABEL_TARGET), op.as_str())
            }
            Assertion::Unrecognised { .. } => "unrecognised assertion".to_string(),
        }
    }

    /// First 8 bytes of SHA-256 over kind, target and operator only (never the expected
    /// value, so it cannot be a hash of a response value), as 16 lowercase hex characters.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        let mut field = |s: &str| {
            h.update((s.len() as u64).to_le_bytes());
            h.update(s.as_bytes());
        };
        match self {
            Assertion::Status { op, .. } => {
                field("status");
                field("");
                field(op.as_str());
            }
            Assertion::Header { name, op, .. } => {
                field("header");
                field(&name.to_ascii_lowercase());
                field(op.as_str());
            }
            Assertion::Json { path, op, .. } => {
                field("json");
                field(path);
                field(op.as_str());
            }
            Assertion::Unrecognised { raw, .. } => {
                field("unrecognised");
                field(raw);
            }
        }
        let digest = h.finalize();
        digest[..8].iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn cap(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The outcome of one assertion.
#[derive(Clone, PartialEq)]
pub enum Verdict {
    Pass,
    /// `actual` is at most 80 characters, for the results pane only.
    Fail {
        actual: Option<String>,
    },
    Skipped {
        reason: String,
    },
}

impl fmt::Debug for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Pass => f.write_str("Pass"),
            Verdict::Fail { actual } => f
                .debug_struct("Fail")
                .field(
                    "actual",
                    &if actual.is_some() {
                        "<redacted>"
                    } else {
                        "<none>"
                    },
                )
                .finish(),
            Verdict::Skipped { reason } => {
                f.debug_struct("Skipped").field("reason", reason).finish()
            }
        }
    }
}

/// One assertion's label and verdict.
#[derive(Clone, PartialEq)]
pub struct AssertionResult {
    pub label: String,
    pub verdict: Verdict,
}

impl fmt::Debug for AssertionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AssertionResult")
            .field("label", &self.label)
            .field("verdict", &self.verdict)
            .finish()
    }
}

/// What a response offers to assertions.
#[derive(Clone, Copy)]
pub struct Facts<'a> {
    pub status: u16,
    pub headers: &'a [Header],
    pub body: Option<&'a [u8]>,
}

impl fmt::Debug for Facts<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Facts")
            .field("status", &self.status)
            .field("headers", &self.headers.len())
            .field("body", &self.body.map(<[u8]>::len))
            .finish()
    }
}

fn skipped(reason: &str) -> Verdict {
    Verdict::Skipped {
        reason: reason.to_string(),
    }
}

fn fail(actual: Option<String>) -> Verdict {
    Verdict::Fail {
        actual: actual.map(|a| cap(&a, MAX_ACTUAL_LEN)),
    }
}

fn verdict(pass: bool, actual: impl FnOnce() -> Option<String>) -> Verdict {
    if pass { Verdict::Pass } else { fail(actual()) }
}

/// Patterns compiled during one `evaluate`, rejected ones included.
#[derive(Default)]
struct Regexes(std::collections::HashMap<String, Result<regex::Regex, String>>);

impl Regexes {
    fn get(&mut self, pattern: &str) -> &Result<regex::Regex, String> {
        self.0
            .entry(pattern.to_string())
            .or_insert_with(|| check_regex(pattern))
    }
}

enum Body {
    Untried,
    Json(serde_json::Value),
    Unusable(&'static str),
}

/// Evaluate every assertion against the response. The body is parsed at most once.
pub fn evaluate(assertions: &[Assertion], facts: &Facts<'_>) -> Vec<AssertionResult> {
    let mut body = Body::Untried;
    let mut res = Regexes::default();
    assertions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let verdict = if i >= MAX_ASSERTIONS {
                skipped("limit: only the first 50 assertions run")
            } else {
                eval_one(a, facts, &mut body, &mut res)
            };
            AssertionResult {
                label: a.label(),
                verdict,
            }
        })
        .collect()
}

fn eval_one(a: &Assertion, facts: &Facts<'_>, body: &mut Body, res: &mut Regexes) -> Verdict {
    match a {
        Assertion::Unrecognised { reason, .. } => {
            if reason == "malformed" {
                skipped("malformed")
            } else {
                skipped("not supported by this version")
            }
        }
        Assertion::Status { op, expected } => eval_status(*op, expected, facts.status, res),
        Assertion::Header { name, op, expected } => eval_header(name, *op, expected, facts, res),
        Assertion::Json { path, op, expected } => {
            let segments = match parse_path(path) {
                Ok(s) => s,
                Err(e) => return skipped(&format!("invalid path: {e}")),
            };
            if matches!(body, Body::Untried) {
                *body = parse_body(facts.body);
            }
            match body {
                Body::Json(v) => eval_json(&segments, *op, expected, v, res),
                Body::Unusable(reason) => skipped(reason),
                Body::Untried => skipped("malformed"),
            }
        }
    }
}

fn parse_body(body: Option<&[u8]>) -> Body {
    match body {
        None => Body::Unusable("no response body"),
        Some(b) if b.len() > MAX_JSON_BODY => Body::Unusable("body over 1 MiB"),
        Some(b) => match serde_json::from_slice(b) {
            Ok(v) => Body::Json(v),
            Err(_) => Body::Unusable("body is not JSON"),
        },
    }
}

#[cfg(test)]
thread_local! {
    static REGEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Compile a pattern within the limits; the error is a short reason.
pub fn check_regex(pattern: &str) -> Result<regex::Regex, String> {
    #[cfg(test)]
    REGEX_BUILDS.with(|c| c.set(c.get() + 1));
    if pattern.chars().count() > MAX_REGEX_LEN {
        return Err("regex over 256 characters".to_string());
    }
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|_| "invalid regex".to_string())
}

fn regex_matches(pattern: &Expected, text: &str, res: &mut Regexes) -> Result<bool, Verdict> {
    let Expected::Str(p) = pattern else {
        return Err(skipped("malformed"));
    };
    match res.get(p) {
        Ok(re) => Ok(re.is_match(text)),
        Err(e) => Err(skipped(e)),
    }
}

fn exists_flag(e: &Expected) -> bool {
    !matches!(e, Expected::Bool(false))
}

fn eval_status(op: Op, expected: &Expected, status: u16, res: &mut Regexes) -> Verdict {
    let actual = || Some(status.to_string());
    match (op, expected) {
        (Op::Equals, Expected::Int(n)) => verdict(i64::from(status) == *n, actual),
        (Op::Matches, p) => match regex_matches(p, &format!("{status:03}"), res) {
            Ok(m) => verdict(m, actual),
            Err(v) => v,
        },
        _ => skipped("malformed"),
    }
}

fn eval_header(
    name: &str,
    op: Op,
    expected: &Expected,
    facts: &Facts<'_>,
    res: &mut Regexes,
) -> Verdict {
    let values: Vec<&str> = facts
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
        .collect();
    let first = || values.first().map(|v| (*v).to_string());
    match (op, expected) {
        (Op::Exists, e) => verdict(values.is_empty() != exists_flag(e), || {
            Some(
                if values.is_empty() {
                    "absent"
                } else {
                    "present"
                }
                .to_string(),
            )
        }),
        (Op::Equals, Expected::Str(s)) => verdict(values.iter().any(|v| v == s), first),
        (Op::Contains, Expected::Str(s)) => {
            verdict(values.iter().any(|v| v.contains(s.as_str())), first)
        }
        (Op::Matches, p) => {
            let mut any = false;
            for v in &values {
                match regex_matches(p, v, res) {
                    Ok(true) => {
                        any = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(e) => return e,
                }
            }
            if values.is_empty() {
                // Still validate the pattern so a bad one is reported either way.
                if let Err(e) = regex_matches(p, "", res) {
                    return e;
                }
            }
            verdict(any, first)
        }
        _ => skipped("malformed"),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Segment {
    Key(String),
    Index(usize),
}

/// Check a JSON path against the supported subset; the error is a short reason.
pub fn check_path(path: &str) -> Result<(), String> {
    parse_path(path).map(|_| ())
}

fn parse_path(path: &str) -> Result<Vec<Segment>, String> {
    if path.chars().count() > MAX_PATH_LEN {
        return Err("over 256 characters".to_string());
    }
    let rest = path.strip_prefix('$').ok_or("must start with $")?;
    let b = rest.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if out.len() >= MAX_PATH_SEGMENTS {
            return Err("over 16 segments".to_string());
        }
        match b[i] {
            b'.' => {
                let start = i + 1;
                let mut end = start;
                while end < b.len() && b[end] != b'.' && b[end] != b'[' {
                    end += 1;
                }
                let name = &rest[start..end];
                if name.is_empty() {
                    return Err("recursion or empty name not supported".to_string());
                }
                if !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    return Err("wildcard or unsupported name syntax".to_string());
                }
                out.push(Segment::Key(name.to_string()));
                i = end;
            }
            b'[' => {
                let close = rest[i..]
                    .find(']')
                    .map(|p| p + i)
                    .ok_or("unclosed bracket")?;
                let inner = &rest[i + 1..close];
                if let Some(q) = inner.strip_prefix('\'') {
                    let name = q.strip_suffix('\'').ok_or("unclosed quote")?;
                    if name.contains('\'') {
                        return Err("unsupported quoting".to_string());
                    }
                    out.push(Segment::Key(name.to_string()));
                } else if !inner.is_empty() && inner.bytes().all(|c| c.is_ascii_digit()) {
                    let n = inner.parse::<usize>().map_err(|_| "index too large")?;
                    out.push(Segment::Index(n));
                } else {
                    return Err(
                        "only [n] and ['name'] are supported (no wildcard, filter, slice or negative index)"
                            .to_string(),
                    );
                }
                i = close + 1;
            }
            _ => return Err("unexpected character".to_string()),
        }
    }
    Ok(out)
}

fn lookup<'a>(root: &'a serde_json::Value, segments: &[Segment]) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for s in segments {
        cur = match s {
            Segment::Key(k) => cur.as_object()?.get(k)?,
            Segment::Index(n) => cur.as_array()?.get(*n)?,
        };
    }
    Some(cur)
}

fn scalar_equals(v: &serde_json::Value, e: &Expected) -> bool {
    match (v, e) {
        (serde_json::Value::String(s), Expected::Str(x)) => s == x,
        (serde_json::Value::Bool(b), Expected::Bool(x)) => b == x,
        (serde_json::Value::Number(n), Expected::Int(x)) => {
            (n.is_i64() || n.is_u64()) && n.as_i64() == Some(*x)
        }
        (serde_json::Value::Number(n), Expected::Float(x)) => n.is_f64() && n.as_f64() == Some(*x),
        _ => false,
    }
}

fn render(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn eval_json(
    segments: &[Segment],
    op: Op,
    expected: &Expected,
    root: &serde_json::Value,
    res: &mut Regexes,
) -> Verdict {
    let node = lookup(root, segments);
    let actual = || node.map(render);
    match op {
        Op::Exists => verdict(node.is_some() == exists_flag(expected), || {
            Some(if node.is_some() { "present" } else { "absent" }.to_string())
        }),
        Op::Equals => verdict(node.is_some_and(|v| scalar_equals(v, expected)), actual),
        Op::Contains => {
            let pass = match (node, expected) {
                (Some(serde_json::Value::String(s)), Expected::Str(x)) => s.contains(x.as_str()),
                (Some(serde_json::Value::Array(items)), e) => {
                    items.iter().any(|i| scalar_equals(i, e))
                }
                _ => false,
            };
            verdict(pass, actual)
        }
        Op::Matches => match (node, expected) {
            (Some(serde_json::Value::String(s)), p) => match regex_matches(p, s, res) {
                Ok(m) => verdict(m, actual),
                Err(v) => v,
            },
            (_, p) => {
                if let Err(v) = regex_matches(p, "", res) {
                    return v;
                }
                fail(actual())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: &str, v: &str) -> Header {
        Header {
            name: n.into(),
            value: v.into(),
        }
    }

    fn s(v: &str) -> Expected {
        Expected::Str(v.into())
    }

    fn st(op: Op, expected: Expected) -> Assertion {
        Assertion::Status { op, expected }
    }

    fn hd(name: &str, op: Op, expected: Expected) -> Assertion {
        Assertion::Header {
            name: name.into(),
            op,
            expected,
        }
    }

    fn js(path: &str, op: Op, expected: Expected) -> Assertion {
        Assertion::Json {
            path: path.into(),
            op,
            expected,
        }
    }

    fn run(a: Assertion, status: u16, headers: &[Header], body: Option<&[u8]>) -> Verdict {
        let facts = Facts {
            status,
            headers,
            body,
        };
        evaluate(&[a], &facts).remove(0).verdict
    }

    fn json(a: Assertion, body: &str) -> Verdict {
        run(a, 200, &[], Some(body.as_bytes()))
    }

    fn pass(v: Verdict) {
        assert_eq!(v, Verdict::Pass);
    }

    fn failed(v: Verdict) {
        assert!(matches!(v, Verdict::Fail { .. }), "{v:?}");
    }

    fn skip_reason(v: Verdict) -> String {
        match v {
            Verdict::Skipped { reason } => reason,
            other => panic!("not skipped: {other:?}"),
        }
    }

    #[test]
    fn status_equals() {
        pass(run(st(Op::Equals, Expected::Int(200)), 200, &[], None));
        failed(run(st(Op::Equals, Expected::Int(200)), 404, &[], None));
    }

    #[test]
    fn status_matches_over_three_digit_text() {
        pass(run(st(Op::Matches, s("^2..$")), 204, &[], None));
        failed(run(st(Op::Matches, s("^2..$")), 404, &[], None));
    }

    #[test]
    fn status_fail_reports_actual() {
        let v = run(st(Op::Equals, Expected::Int(200)), 404, &[], None);
        assert_eq!(
            v,
            Verdict::Fail {
                actual: Some("404".into())
            }
        );
    }

    #[test]
    fn header_equals_contains_matches() {
        let hs = [h("Content-Type", "application/json")];
        pass(run(
            hd("content-type", Op::Equals, s("application/json")),
            200,
            &hs,
            None,
        ));
        failed(run(
            hd("content-type", Op::Equals, s("json")),
            200,
            &hs,
            None,
        ));
        pass(run(
            hd("CONTENT-TYPE", Op::Contains, s("json")),
            200,
            &hs,
            None,
        ));
        failed(run(
            hd("content-type", Op::Contains, s("xml")),
            200,
            &hs,
            None,
        ));
        pass(run(
            hd("content-type", Op::Matches, s("^application/")),
            200,
            &hs,
            None,
        ));
        failed(run(
            hd("content-type", Op::Matches, s("^text/")),
            200,
            &hs,
            None,
        ));
    }

    #[test]
    fn header_exists_defaults_true_and_can_be_false() {
        let hs = [h("X-Id", "1")];
        pass(run(
            hd("x-id", Op::Exists, Expected::Bool(true)),
            200,
            &hs,
            None,
        ));
        failed(run(
            hd("x-missing", Op::Exists, Expected::Bool(true)),
            200,
            &hs,
            None,
        ));
        pass(run(
            hd("x-missing", Op::Exists, Expected::Bool(false)),
            200,
            &hs,
            None,
        ));
        failed(run(
            hd("x-id", Op::Exists, Expected::Bool(false)),
            200,
            &hs,
            None,
        ));
    }

    #[test]
    fn header_any_of_several_values_may_pass() {
        let hs = [h("Set-Cookie", "a=1"), h("set-cookie", "b=2")];
        pass(run(hd("set-cookie", Op::Equals, s("b=2")), 200, &hs, None));
        pass(run(hd("set-cookie", Op::Contains, s("b=")), 200, &hs, None));
        pass(run(hd("set-cookie", Op::Matches, s("^b=")), 200, &hs, None));
        failed(run(hd("set-cookie", Op::Equals, s("c=3")), 200, &hs, None));
    }

    #[test]
    fn json_exists() {
        pass(json(
            js("$.a", Op::Exists, Expected::Bool(true)),
            r#"{"a":null}"#,
        ));
        failed(json(
            js("$.b", Op::Exists, Expected::Bool(true)),
            r#"{"a":1}"#,
        ));
        pass(json(
            js("$.b", Op::Exists, Expected::Bool(false)),
            r#"{"a":1}"#,
        ));
        failed(json(
            js("$.a", Op::Exists, Expected::Bool(false)),
            r#"{"a":1}"#,
        ));
    }

    #[test]
    fn json_equals_is_type_strict() {
        pass(json(
            js("$.n", Op::Equals, Expected::Int(200)),
            r#"{"n":200}"#,
        ));
        failed(json(js("$.n", Op::Equals, s("200")), r#"{"n":200}"#));
        failed(json(
            js("$.n", Op::Equals, Expected::Int(200)),
            r#"{"n":"200"}"#,
        ));
        pass(json(
            js("$.n", Op::Equals, Expected::Float(1.5)),
            r#"{"n":1.5}"#,
        ));
        failed(json(
            js("$.n", Op::Equals, Expected::Int(1)),
            r#"{"n":1.0}"#,
        ));
        pass(json(
            js("$.n", Op::Equals, Expected::Bool(true)),
            r#"{"n":true}"#,
        ));
        failed(json(
            js("$.n", Op::Equals, Expected::Bool(true)),
            r#"{"n":"true"}"#,
        ));
        pass(json(js("$.n", Op::Equals, s("x")), r#"{"n":"x"}"#));
        failed(json(js("$.m", Op::Equals, s("x")), r#"{"n":"x"}"#));
    }

    #[test]
    fn json_contains_string_or_array_element() {
        pass(json(js("$.s", Op::Contains, s("ell")), r#"{"s":"hello"}"#));
        failed(json(js("$.s", Op::Contains, s("xyz")), r#"{"s":"hello"}"#));
        pass(json(
            js("$.a", Op::Contains, Expected::Int(2)),
            r#"{"a":[1,2]}"#,
        ));
        failed(json(
            js("$.a", Op::Contains, Expected::Int(3)),
            r#"{"a":[1,2]}"#,
        ));
        pass(json(js("$.a", Op::Contains, s("b")), r#"{"a":["a","b"]}"#));
        failed(json(js("$.a", Op::Contains, s("2")), r#"{"a":[1,2]}"#));
        failed(json(
            js("$.n", Op::Contains, Expected::Int(1)),
            r#"{"n":1}"#,
        ));
    }

    #[test]
    fn json_matches_string_values_only() {
        pass(json(
            js("$.s", Op::Matches, s("^(ready|done)$")),
            r#"{"s":"done"}"#,
        ));
        failed(json(
            js("$.s", Op::Matches, s("^(ready|done)$")),
            r#"{"s":"new"}"#,
        ));
        failed(json(js("$.s", Op::Matches, s("1")), r#"{"s":1}"#));
        failed(json(js("$.t", Op::Matches, s("1")), r#"{"s":1}"#));
    }

    #[test]
    fn json_path_subset_resolves() {
        let body = r#"{"data":{"items":[{"id":7},{"id":8}],"a b":3}}"#;
        pass(json(
            js("$.data.items[1].id", Op::Equals, Expected::Int(8)),
            body,
        ));
        pass(json(
            js("$['data']['a b']", Op::Equals, Expected::Int(3)),
            body,
        ));
        pass(json(js("$", Op::Exists, Expected::Bool(true)), body));
        pass(json(
            js("$.data.items[0]['id']", Op::Equals, Expected::Int(7)),
            body,
        ));
    }

    #[test]
    fn json_path_rejects_unsupported_syntax() {
        for p in [
            "$.a.*",
            "$.a[*]",
            "$..a",
            "$.a[?(@.b)]",
            "$.a[0:2]",
            "$.a[-1]",
            "a.b",
            "$.a[",
            "$.a[x]",
            "$.",
            "$.a b",
        ] {
            assert!(check_path(p).is_err(), "{p} should be rejected");
            let r = json(js(p, Op::Exists, Expected::Bool(true)), r#"{"a":[1]}"#);
            assert!(skip_reason(r).starts_with("invalid path"), "{p}");
        }
    }

    #[test]
    fn json_path_limits() {
        let long = format!("$.{}", "a".repeat(256));
        assert!(check_path(&long).is_err());
        let ok16 = format!("${}", ".a".repeat(16));
        assert!(check_path(&ok16).is_ok());
        let over = format!("${}", ".a".repeat(17));
        assert!(check_path(&over).is_err());
    }

    #[test]
    fn regex_limits() {
        let long = "a".repeat(257);
        let r = run(st(Op::Matches, s(&long)), 200, &[], None);
        assert!(skip_reason(r).contains("256"));
        let r = run(st(Op::Matches, s("(")), 200, &[], None);
        assert_eq!(skip_reason(r), "invalid regex");
        let big = "(a{100}){100}{100}";
        assert!(check_regex(big).is_err());
    }

    #[test]
    fn body_over_limit_is_skipped_with_size_reason() {
        let big = vec![b' '; MAX_JSON_BODY + 1];
        let r = run(
            js("$.a", Op::Exists, Expected::Bool(true)),
            200,
            &[],
            Some(&big),
        );
        assert_eq!(skip_reason(r), "body over 1 MiB");
    }

    #[test]
    fn non_json_and_missing_body_have_distinct_reasons() {
        let a = js("$.a", Op::Exists, Expected::Bool(true));
        let not_json = skip_reason(run(a.clone(), 200, &[], Some(b"<html>")));
        let none = skip_reason(run(a, 200, &[], None));
        assert_eq!(not_json, "body is not JSON");
        assert_ne!(not_json, none);
        assert_ne!(not_json, "body over 1 MiB");
    }

    #[test]
    fn body_parsed_once_per_evaluate() {
        let a = js("$.a", Op::Exists, Expected::Bool(true));
        let list = vec![a.clone(), a.clone(), a];
        let facts = Facts {
            status: 200,
            headers: &[],
            body: Some(b"not json"),
        };
        let out = evaluate(&list, &facts);
        assert!(out.iter().all(
            |r| matches!(&r.verdict, Verdict::Skipped { reason } if reason == "body is not JSON")
        ));
    }

    #[test]
    fn entries_after_the_fiftieth_are_skipped() {
        let list: Vec<Assertion> = (0..51)
            .map(|_| st(Op::Equals, Expected::Int(200)))
            .collect();
        let facts = Facts {
            status: 200,
            headers: &[],
            body: None,
        };
        let out = evaluate(&list, &facts);
        assert_eq!(out.len(), 51);
        assert!(out[..50].iter().all(|r| r.verdict == Verdict::Pass));
        assert!(skip_reason(out[50].verdict.clone()).starts_with("limit"));
    }

    #[test]
    fn unrecognised_is_skipped() {
        let nope = Assertion::Unrecognised {
            raw: "x".into(),
            reason: "not supported".into(),
        };
        assert_eq!(
            skip_reason(run(nope, 200, &[], None)),
            "not supported by this version"
        );
        let bad = Assertion::Unrecognised {
            raw: "x".into(),
            reason: "malformed".into(),
        };
        assert_eq!(skip_reason(run(bad, 200, &[], None)), "malformed");
    }

    #[test]
    fn fail_actual_is_capped_at_80_chars() {
        let long = "é".repeat(200);
        let hs = [h("x", &long)];
        let v = run(hd("x", Op::Equals, s("nope")), 200, &hs, None);
        let Verdict::Fail { actual: Some(a) } = v else {
            panic!("expected fail");
        };
        assert_eq!(a.chars().count(), 80);
    }

    #[test]
    fn labels() {
        assert_eq!(st(Op::Equals, Expected::Int(200)).label(), "status equals");
        assert_eq!(
            hd("content-type", Op::Contains, s("j")).label(),
            "header content-type contains"
        );
        assert_eq!(
            js("$.data.id", Op::Exists, Expected::Bool(true)).label(),
            "json $.data.id exists"
        );
    }

    #[test]
    fn fingerprint_is_16_hex_and_tracks_the_definition() {
        let a = st(Op::Equals, Expected::Int(200));
        let f = a.fingerprint();
        assert_eq!(f.len(), 16);
        assert!(
            f.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        );
        assert_eq!(f, st(Op::Equals, Expected::Int(200)).fingerprint());
        assert_ne!(
            js("$.a", Op::Equals, s("1")).fingerprint(),
            js("$.b", Op::Equals, s("1")).fingerprint()
        );
        assert_ne!(
            js("$.a", Op::Equals, s("1")).fingerprint(),
            js("$.a", Op::Contains, s("1")).fingerprint()
        );
    }

    #[test]
    fn from_parts_applies_the_matrix() {
        let ok = |k, t, o, v| Assertion::from_parts(k, t, o, v).is_ok();
        assert!(ok("status", None, "equals", Some(Expected::Int(200))));
        assert!(!ok("status", None, "equals", Some(Expected::Int(99))));
        assert!(!ok("status", None, "equals", Some(s("200"))));
        assert!(!ok("status", Some("x"), "equals", Some(Expected::Int(200))));
        assert!(!ok("status", None, "contains", Some(s("2"))));
        assert!(ok("header", Some("a"), "exists", None));
        assert!(!ok("header", None, "equals", Some(s("a"))));
        assert!(!ok("header", Some("a"), "equals", Some(Expected::Int(1))));
        assert!(ok(
            "json",
            Some("$.a"),
            "equals",
            Some(Expected::Float(1.5))
        ));
        assert!(!ok("json", Some("$.a"), "matches", Some(Expected::Int(1))));
        assert_eq!(
            Assertion::from_parts("grpc", None, "equals", None),
            Err(ParseError::NotSupported)
        );
        assert_eq!(
            Assertion::from_parts("status", None, "teleports", None),
            Err(ParseError::NotSupported)
        );
    }

    #[test]
    fn debug_redacts_actual_value() {
        let hs = [h("x", "CANARY-ACTUAL")];
        let r = evaluate(
            &[hd("x", Op::Equals, s("nope"))],
            &Facts {
                status: 200,
                headers: &hs,
                body: Some(b"CANARY-BODY"),
            },
        );
        assert!(matches!(&r[0].verdict, Verdict::Fail { actual: Some(a) } if a == "CANARY-ACTUAL"));
        for text in [
            format!("{:?}", r[0]),
            format!("{:#?}", r[0]),
            format!("{:?}", r[0].verdict),
            format!("{:#?}", r[0].verdict),
        ] {
            assert!(!text.contains("CANARY"), "{text}");
        }
        let facts = Facts {
            status: 200,
            headers: &hs,
            body: Some(b"CANARY-BODY"),
        };
        for text in [format!("{facts:?}"), format!("{facts:#?}")] {
            assert!(!text.contains("CANARY"), "{text}");
        }
    }

    #[test]
    fn fingerprint_ignores_the_expected_value() {
        let a = |v: &str| hd("x", Op::Equals, s(v)).fingerprint();
        assert_eq!(a("one"), a("two"));
        let st = |n| Assertion::Status {
            op: Op::Equals,
            expected: Expected::Int(n),
        };
        assert_eq!(st(200).fingerprint(), st(404).fingerprint());
        assert_ne!(
            hd("x", Op::Equals, s("a")).fingerprint(),
            hd("y", Op::Equals, s("a")).fingerprint()
        );
    }

    fn many_headers() -> Vec<Header> {
        (0..100).map(|i| h("x-a", &format!("v{i}"))).collect()
    }

    #[test]
    fn regex_compiled_once_per_evaluation() {
        let hs = many_headers();
        let facts = Facts {
            status: 200,
            headers: &hs,
            body: None,
        };
        let ok: Vec<_> = (0..50)
            .map(|_| hd("x-a", Op::Matches, s("^nomatch$")))
            .collect();
        REGEX_BUILDS.with(|c| c.set(0));
        evaluate(&ok, &facts);
        assert_eq!(REGEX_BUILDS.with(|c| c.get()), 1);
        let bad: Vec<_> = (0..50).map(|_| hd("x-a", Op::Matches, s("("))).collect();
        REGEX_BUILDS.with(|c| c.set(0));
        let r = evaluate(&bad, &facts);
        assert_eq!(REGEX_BUILDS.with(|c| c.get()), 1);
        assert!(matches!(r[0].verdict, Verdict::Skipped { .. }));
    }
}
