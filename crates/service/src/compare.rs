//! Pure comparison of two history entries: status, headers and a structural JSON body diff,
//! with a per-collection ignore list. It never calls an incomplete side equal.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use wisp_core::Header;
use wisp_storage::history::{BodyEvidence, Evidence, HistoryEntry, StoredEntry};

/// Most changes listed per diff; the total is still counted.
pub const MAX_CHANGES: usize = 1_000;
/// Longest value shown, in characters.
pub const MAX_VALUE_CHARS: usize = 256;
/// Deepest JSON nesting that is diffed.
pub const MAX_DEPTH: usize = 128;

// ---- ignore list ----

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Any,
    Key(String),
}

/// A parsed ignore list: header names (lowercase) and body pointer patterns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IgnoreList {
    headers: HashSet<String>,
    bodies: Vec<Vec<Seg>>,
}

/// One pattern that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreProblem {
    pub index: usize,
    pub reason: String,
}

impl IgnoreList {
    /// Parse patterns. Valid ones are kept; bad ones are returned as problems.
    pub fn parse(patterns: &[String]) -> (IgnoreList, Vec<IgnoreProblem>) {
        let mut list = IgnoreList::default();
        let mut problems = Vec::new();
        for (index, p) in patterns.iter().enumerate() {
            let bad = |reason: &str| IgnoreProblem {
                index,
                reason: reason.to_string(),
            };
            if let Some(name) = p.strip_prefix("header:") {
                if name.is_empty() {
                    problems.push(bad("empty header name"));
                } else {
                    list.headers.insert(name.to_ascii_lowercase());
                }
            } else if let Some(ptr) = p.strip_prefix("body:") {
                match parse_pointer(ptr) {
                    Ok(segs) => list.bodies.push(segs),
                    Err(reason) => problems.push(bad(reason)),
                }
            } else {
                problems.push(bad("unknown prefix (use header: or body:)"));
            }
        }
        (list, problems)
    }

    fn header_ignored(&self, lower: &str) -> bool {
        self.headers.contains(lower)
    }

    /// True when `path` is covered by a pattern (the pattern is a prefix of it).
    fn body_ignored(&self, path: &[String]) -> bool {
        self.bodies.iter().any(|pat| {
            pat.len() <= path.len()
                && pat.iter().zip(path).all(|(s, k)| match s {
                    Seg::Any => true,
                    Seg::Key(x) => x == k,
                })
        })
    }
}

fn parse_pointer(ptr: &str) -> Result<Vec<Seg>, &'static str> {
    let rest = ptr.strip_prefix('/').ok_or("pointer must start with '/'")?;
    rest.split('/')
        .map(|raw| {
            if raw == "*" {
                return Ok(Seg::Any);
            }
            let mut out = String::with_capacity(raw.len());
            let mut it = raw.chars();
            while let Some(c) = it.next() {
                if c == '~' {
                    match it.next() {
                        Some('0') => out.push('~'),
                        Some('1') => out.push('/'),
                        _ => return Err("bad '~' escape (use ~0 or ~1)"),
                    }
                } else {
                    out.push(c);
                }
            }
            Ok(Seg::Key(out))
        })
        .collect()
}

fn escape(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

// ---- inputs and results ----

/// What the comparison reads from one history entry.
#[derive(Debug, Clone, Copy)]
pub struct Side<'a> {
    pub status: Option<u16>,
    pub evidence: Option<Evidence>,
    pub body_stored: bool,
    pub body_truncated: bool,
    pub headers: &'a [Header],
    pub redacted: &'a [String],
    pub body: &'a [u8],
    /// The stored entry could not be loaded, so headers and body are unknown.
    pub entry_missing: bool,
    /// A body was recorded but cannot be read now (entry not loadable or `.bin` gone).
    pub body_unavailable: bool,
}

impl<'a> Side<'a> {
    /// Build a side from the index line and, when loadable, the stored entry.
    /// `body_file_exists` says whether the entry's `.bin` is on disk
    /// (`HistoryStore::body_path(id).is_some()`); an empty body alone is not "missing".
    pub fn from_entry(
        entry: &'a HistoryEntry,
        stored: Option<&'a StoredEntry>,
        body_file_exists: bool,
    ) -> Side<'a> {
        let expected = match entry.evidence {
            Some(e) => matches!(e.body, BodyEvidence::Stored | BodyEvidence::Truncated),
            None => entry.body_stored,
        };
        Side {
            entry_missing: stored.is_none(),
            body_unavailable: expected && (stored.is_none() || !body_file_exists),
            status: entry.status,
            evidence: entry.evidence,
            body_stored: entry.body_stored,
            body_truncated: entry.body_truncated,
            headers: stored.map_or(&[], |s| &s.response_headers),
            redacted: stored.map_or(&[], |s| &s.response_redacted),
            body: stored.map_or(&[], |s| &s.body),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusDiff {
    Equal(u16),
    Changed { a: u16, b: u16 },
    Missing { a: Option<u16>, b: Option<u16> },
}

/// A value cut to [`MAX_VALUE_CHARS`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub cut: bool,
}

fn shown(s: &str) -> Shown {
    match s.char_indices().nth(MAX_VALUE_CHARS) {
        Some((i, _)) => Shown {
            text: s[..i].to_string(),
            cut: true,
        },
        None => Shown {
            text: s.to_string(),
            cut: false,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderChange {
    Added {
        name: String,
        values: Vec<Shown>,
    },
    Removed {
        name: String,
        values: Vec<Shown>,
    },
    Changed {
        name: String,
        a: Vec<Shown>,
        b: Vec<Shown>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HeadersDiff {
    /// Side A did not record response headers.
    pub a_not_recorded: bool,
    /// Side B did not record response headers.
    pub b_not_recorded: bool,
    pub changes: Vec<HeaderChange>,
    /// Lowercase names reported as redacted and not compared.
    pub redacted: Vec<String>,
    /// Differences hidden by `header:` patterns.
    pub ignored: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Truncated,
    Off,
    Unreadable,
    NoResponse,
    /// Recorded, but the stored entry or its body file is gone or unreadable.
    StoredUnavailable,
}

impl BodyKind {
    /// Display text for the reason.
    pub fn text(self) -> &'static str {
        match self {
            BodyKind::StoredUnavailable => "Stored body unavailable (deleted or unreadable)",
            BodyKind::Truncated => "body cut at 1 MiB; not compared",
            BodyKind::Off => "body not stored (history bodies off, or a `wisp-cli run` entry)",
            BodyKind::Unreadable => "body could not be read when recorded",
            BodyKind::NoResponse => "no response body (request failed or was cancelled)",
        }
    }
}

/// Why one side's body cannot be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideReason {
    pub kind: BodyKind,
    /// The entry was recorded before 0.5.0, so the reason is inferred.
    pub before_0_5_0: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    TypeChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyChange {
    /// RFC 6901 pointer.
    pub path: String,
    pub kind: ChangeKind,
    pub before: Option<Shown>,
    pub after: Option<Shown>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyDiff {
    Equal {
        ignored: usize,
    },
    Different {
        changes: Vec<BodyChange>,
        /// All changes found, which may exceed `changes.len()`.
        total: usize,
        ignored: usize,
    },
    NotJson {
        equal: bool,
    },
    /// At least one side is incomplete; `None` means that side is complete.
    Incomplete {
        a: Option<SideReason>,
        b: Option<SideReason>,
    },
    TooDeep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub status: StatusDiff,
    pub headers: HeadersDiff,
    pub body: BodyDiff,
}

pub fn compare(a: &Side, b: &Side, ignore: &IgnoreList) -> Comparison {
    Comparison {
        status: compare_status(a.status, b.status),
        headers: compare_headers(a, b, ignore),
        body: compare_body(a, b, ignore),
    }
}

fn compare_status(a: Option<u16>, b: Option<u16>) -> StatusDiff {
    match (a, b) {
        (Some(x), Some(y)) if x == y => StatusDiff::Equal(x),
        (Some(a), Some(b)) => StatusDiff::Changed { a, b },
        (a, b) => StatusDiff::Missing { a, b },
    }
}

// ---- headers ----

fn group<'a>(
    headers: &'a [Header],
    redacted: &HashSet<String>,
    order: &mut Vec<(String, &'a str)>,
    seen: &mut HashSet<String>,
) -> HashMap<String, Vec<&'a str>> {
    let mut map: HashMap<String, Vec<&str>> = HashMap::new();
    for h in headers {
        let lower = h.name.to_ascii_lowercase();
        if redacted.contains(&lower) {
            continue;
        }
        if seen.insert(lower.clone()) {
            order.push((lower.clone(), h.name.as_str()));
        }
        map.entry(lower).or_default().push(h.value.as_str());
    }
    map
}

fn compare_headers(a: &Side, b: &Side, ignore: &IgnoreList) -> HeadersDiff {
    let mut out = HeadersDiff {
        a_not_recorded: a.entry_missing || a.evidence.is_some_and(|e| !e.headers),
        b_not_recorded: b.entry_missing || b.evidence.is_some_and(|e| !e.headers),
        ..HeadersDiff::default()
    };
    if out.a_not_recorded || out.b_not_recorded {
        return out;
    }
    let mut red: HashSet<String> = HashSet::new();
    for n in a.redacted.iter().chain(b.redacted) {
        red.insert(n.to_ascii_lowercase());
    }
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let ga = group(a.headers, &red, &mut order, &mut seen);
    let gb = group(b.headers, &red, &mut order, &mut seen);
    // Redacted names that actually appear on either side.
    let present: HashSet<String> = a
        .headers
        .iter()
        .chain(b.headers)
        .map(|h| h.name.to_ascii_lowercase())
        .collect();
    let mut redacted: Vec<String> = red.into_iter().filter(|n| present.contains(n)).collect();
    redacted.sort();
    out.redacted = redacted;
    for (lower, name) in order {
        let (va, vb) = (ga.get(&lower), gb.get(&lower));
        if va == vb {
            continue;
        }
        if ignore.header_ignored(&lower) {
            out.ignored += 1;
            continue;
        }
        let list = |v: &Vec<&str>| v.iter().map(|s| shown(s)).collect::<Vec<_>>();
        let name = name.to_string();
        out.changes.push(match (va, vb) {
            (Some(x), Some(y)) => HeaderChange::Changed {
                name,
                a: list(x),
                b: list(y),
            },
            (Some(x), None) => HeaderChange::Removed {
                name,
                values: list(x),
            },
            (None, Some(y)) => HeaderChange::Added {
                name,
                values: list(y),
            },
            (None, None) => continue,
        });
    }
    out
}

// ---- body ----

/// `None` means the side's body is stored complete.
fn side_reason(s: &Side) -> Option<SideReason> {
    if s.body_unavailable {
        return Some(SideReason {
            kind: BodyKind::StoredUnavailable,
            before_0_5_0: s.evidence.is_none(),
        });
    }
    let (kind, pre) = match s.evidence {
        Some(e) => match e.body {
            BodyEvidence::Stored => return None,
            BodyEvidence::Truncated => (BodyKind::Truncated, false),
            BodyEvidence::Off => (BodyKind::Off, false),
            BodyEvidence::Unreadable => (BodyKind::Unreadable, false),
            BodyEvidence::NoResponse => (BodyKind::NoResponse, false),
        },
        None if s.body_stored && !s.body_truncated => return None,
        None if s.body_truncated => (BodyKind::Truncated, true),
        None => (BodyKind::Off, true),
    };
    Some(SideReason {
        kind,
        before_0_5_0: pre,
    })
}

fn compare_body(a: &Side, b: &Side, ignore: &IgnoreList) -> BodyDiff {
    let (ra, rb) = (side_reason(a), side_reason(b));
    if ra.is_some() || rb.is_some() {
        return BodyDiff::Incomplete { a: ra, b: rb };
    }
    let (va, vb) = match (parse(a.body), parse(b.body)) {
        (Parsed::TooDeep, _) | (_, Parsed::TooDeep) => return BodyDiff::TooDeep,
        (Parsed::Ok(x), Parsed::Ok(y)) => (x, y),
        _ => {
            return BodyDiff::NotJson {
                equal: a.body == b.body,
            };
        }
    };
    let mut w = Walk {
        ignore,
        changes: Vec::new(),
        total: 0,
        ignored: 0,
        path: Vec::new(),
        too_deep: false,
    };
    w.walk(Some(&va), Some(&vb));
    if w.too_deep {
        return BodyDiff::TooDeep;
    }
    if w.total == 0 {
        BodyDiff::Equal { ignored: w.ignored }
    } else {
        BodyDiff::Different {
            changes: w.changes,
            total: w.total,
            ignored: w.ignored,
        }
    }
}

enum Parsed {
    Ok(Value),
    NotJson,
    TooDeep,
}

fn parse(bytes: &[u8]) -> Parsed {
    match serde_json::from_slice(bytes) {
        Ok(v) => Parsed::Ok(v),
        Err(e) if e.to_string().starts_with("recursion limit exceeded") => Parsed::TooDeep,
        Err(_) => Parsed::NotJson,
    }
}

fn type_name(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

fn leaf_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_f64() || y.is_f64() {
                x.as_f64() == y.as_f64()
            } else if let (Some(i), Some(j)) = (x.as_i64(), y.as_i64()) {
                i == j
            } else {
                x.as_u64() == y.as_u64()
            }
        }
        _ => a == b,
    }
}

/// Deep equality using the same number rule as the diff.
fn deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| deep_equal(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| deep_equal(v, w)))
        }
        _ => leaf_equal(a, b),
    }
}

struct Walk<'a> {
    ignore: &'a IgnoreList,
    changes: Vec<BodyChange>,
    total: usize,
    ignored: usize,
    path: Vec<String>,
    too_deep: bool,
}

impl Walk<'_> {
    fn pointer(&self) -> String {
        self.path.iter().fold(String::new(), |mut s, seg| {
            s.push('/');
            s.push_str(&escape(seg));
            s
        })
    }

    fn record(&mut self, kind: ChangeKind, a: Option<&Value>, b: Option<&Value>) {
        self.total += 1;
        if self.changes.len() < MAX_CHANGES {
            let show = |v: &Value| shown(&v.to_string());
            self.changes.push(BodyChange {
                path: self.pointer(),
                kind,
                before: a.map(show),
                after: b.map(show),
            });
        }
    }

    /// Visit the node at `self.path`, present on one or both sides.
    fn walk(&mut self, a: Option<&Value>, b: Option<&Value>) {
        if self.too_deep {
            return;
        }
        if self.path.len() > MAX_DEPTH {
            self.too_deep = true;
            return;
        }
        if !self.path.is_empty() && self.ignore.body_ignored(&self.path) {
            let differs = match (a, b) {
                (Some(x), Some(y)) => !deep_equal(x, y),
                _ => true,
            };
            if differs {
                self.ignored += 1;
            }
            return;
        }
        match (a, b) {
            (None, None) => {}
            (Some(x), None) => self.record(ChangeKind::Removed, Some(x), None),
            (None, Some(y)) => self.record(ChangeKind::Added, None, Some(y)),
            (Some(Value::Object(x)), Some(Value::Object(y))) => {
                for (k, v) in x {
                    self.path.push(k.clone());
                    self.walk(Some(v), y.get(k));
                    self.path.pop();
                }
                for (k, v) in y {
                    if !x.contains_key(k) {
                        self.path.push(k.clone());
                        self.walk(None, Some(v));
                        self.path.pop();
                    }
                }
            }
            (Some(Value::Array(x)), Some(Value::Array(y))) => {
                for i in 0..x.len().max(y.len()) {
                    self.path.push(i.to_string());
                    self.walk(x.get(i), y.get(i));
                    self.path.pop();
                }
            }
            (Some(x), Some(y)) => {
                if type_name(x) != type_name(y) {
                    self.record(ChangeKind::TypeChanged, Some(x), Some(y));
                } else if !leaf_equal(x, y) {
                    self.record(ChangeKind::Changed, Some(x), Some(y));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: &[Header] = &[];
    const NOREDACT: &[String] = &[];

    fn h(n: &str, v: &str) -> Header {
        Header {
            name: n.into(),
            value: v.into(),
        }
    }

    fn side<'a>(body: &'a [u8]) -> Side<'a> {
        Side {
            status: Some(200),
            evidence: Some(Evidence {
                body: BodyEvidence::Stored,
                headers: true,
            }),
            body_stored: true,
            body_truncated: false,
            headers: NONE,
            redacted: NOREDACT,
            body,
            entry_missing: false,
            body_unavailable: false,
        }
    }

    fn ig(p: &[&str]) -> IgnoreList {
        let v: Vec<String> = p.iter().map(|s| s.to_string()).collect();
        let (l, probs) = IgnoreList::parse(&v);
        assert!(probs.is_empty(), "{probs:?}");
        l
    }

    fn body(a: &str, b: &str, i: &IgnoreList) -> BodyDiff {
        compare(&side(a.as_bytes()), &side(b.as_bytes()), i).body
    }

    #[test]
    fn compare_ignore_parse_problems_and_valid_kept() {
        let v: Vec<String> = [
            "header:",
            "body:a",
            "body:/a~2",
            "cookie:x",
            "header:Date",
            "body:/a/*",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (l, p) = IgnoreList::parse(&v);
        assert_eq!(p.iter().map(|x| x.index).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(l.header_ignored("date"));
        assert!(l.body_ignored(&["a".into(), "7".into(), "z".into()]));
        assert!(!l.body_ignored(&["a".into()]));
    }

    #[test]
    fn compare_status_variants() {
        assert_eq!(compare_status(Some(200), Some(200)), StatusDiff::Equal(200));
        assert_eq!(
            compare_status(Some(200), Some(500)),
            StatusDiff::Changed { a: 200, b: 500 }
        );
        assert_eq!(
            compare_status(None, Some(500)),
            StatusDiff::Missing {
                a: None,
                b: Some(500)
            }
        );
    }

    #[test]
    fn compare_headers_diff_case_and_repeats() {
        let ha = [
            h("X-A", "1"),
            h("Set-Cookie", "a"),
            h("set-cookie", "b"),
            h("Gone", "x"),
        ];
        let hb = [
            h("x-a", "1"),
            h("Set-Cookie", "b"),
            h("Set-Cookie", "a"),
            h("New", "y"),
        ];
        let mut a = side(b"");
        let mut b = side(b"");
        a.headers = &ha;
        b.headers = &hb;
        let d = compare(&a, &b, &IgnoreList::default()).headers;
        assert_eq!(d.changes.len(), 3, "{d:?}");
        assert!(
            matches!(&d.changes[0], HeaderChange::Changed { name, .. } if name == "Set-Cookie")
        );
        assert!(matches!(&d.changes[1], HeaderChange::Removed { name, .. } if name == "Gone"));
        assert!(matches!(&d.changes[2], HeaderChange::Added { name, .. } if name == "New"));
    }

    #[test]
    fn compare_headers_ignore_counts() {
        let ha = [h("Date", "1")];
        let hb = [h("date", "2")];
        let mut a = side(b"");
        let mut b = side(b"");
        a.headers = &ha;
        b.headers = &hb;
        let d = compare(&a, &b, &ig(&["header:DATE"])).headers;
        assert!(d.changes.is_empty());
        assert_eq!(d.ignored, 1);
    }

    #[test]
    fn compare_headers_redacted_on_one_side() {
        let ha = [h("Authorization", ""), h("A", "1")];
        let hb = [h("Authorization", "secret"), h("A", "1")];
        let ra = ["authorization".to_string()];
        let mut a = side(b"");
        let mut b = side(b"");
        a.headers = &ha;
        a.redacted = &ra;
        b.headers = &hb;
        let d = compare(&a, &b, &IgnoreList::default()).headers;
        assert!(d.changes.is_empty(), "{d:?}");
        assert_eq!(d.redacted, ["authorization"]);
    }

    #[test]
    fn compare_headers_not_recorded_note() {
        let mut a = side(b"");
        a.evidence = Some(Evidence {
            body: BodyEvidence::NoResponse,
            headers: false,
        });
        let d = compare(&a, &side(b""), &IgnoreList::default()).headers;
        assert!(d.a_not_recorded && !d.b_not_recorded);
        assert!(d.changes.is_empty());
    }

    #[test]
    fn compare_body_changes_all_kinds_and_arrays() {
        let d = body(
            r#"{"a":1,"b":"x","c":[1,2,3],"d":true,"e":null}"#,
            r#"{"a":2,"c":[1,9],"d":"yes","f":0,"e":null}"#,
            &IgnoreList::default(),
        );
        let BodyDiff::Different { changes, total, .. } = d else {
            panic!("{d:?}")
        };
        assert_eq!(total, 6);
        let get = |p: &str| changes.iter().find(|c| c.path == p).unwrap().kind;
        assert_eq!(get("/a"), ChangeKind::Changed);
        assert_eq!(get("/b"), ChangeKind::Removed);
        assert_eq!(get("/c/1"), ChangeKind::Changed);
        assert_eq!(get("/c/2"), ChangeKind::Removed);
        assert_eq!(get("/d"), ChangeKind::TypeChanged);
        assert_eq!(get("/f"), ChangeKind::Added);
    }

    #[test]
    fn compare_body_pointer_escaping() {
        let d = body(r#"{"a/b":1}"#, r#"{"a/b":2}"#, &IgnoreList::default());
        let BodyDiff::Different { changes, .. } = d else {
            panic!()
        };
        assert_eq!(changes[0].path, "/a~1b");
    }

    #[test]
    fn compare_body_float_int_pair() {
        let eq = body(r#"{"n":1}"#, r#"{"n":1.0}"#, &IgnoreList::default());
        assert_eq!(eq, BodyDiff::Equal { ignored: 0 });
        let big = body(
            r#"{"n":9007199254740993}"#,
            r#"{"n":9007199254740992}"#,
            &IgnoreList::default(),
        );
        assert!(
            matches!(big, BodyDiff::Different { total: 1, .. }),
            "{big:?}"
        );
    }

    #[test]
    fn compare_body_wildcard_and_subtree_ignore() {
        let a = r#"{"items":[{"id":1,"v":1},{"id":2,"v":1}],"meta":{"t":1,"u":2}}"#;
        let b = r#"{"items":[{"id":3,"v":1},{"id":4,"v":2}],"meta":{"t":9,"u":8}}"#;
        let d = body(a, b, &ig(&["body:/items/*/id", "body:/meta"]));
        let BodyDiff::Different {
            changes, ignored, ..
        } = d
        else {
            panic!("{d:?}")
        };
        assert_eq!(ignored, 3);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "/items/1/v");
        let all = body(a, b, &ig(&["body:/items", "body:/meta"]));
        assert_eq!(all, BodyDiff::Equal { ignored: 2 });
    }

    #[test]
    fn compare_body_not_json() {
        let d = compare(&side(b"hello"), &side(b"hello"), &IgnoreList::default()).body;
        assert_eq!(d, BodyDiff::NotJson { equal: true });
        let d = compare(&side(b"{}"), &side(b"nope"), &IgnoreList::default()).body;
        assert_eq!(d, BodyDiff::NotJson { equal: false });
    }

    fn with(body_ev: BodyEvidence) -> Side<'static> {
        let mut s = side(b"{}");
        s.evidence = Some(Evidence {
            body: body_ev,
            headers: true,
        });
        s
    }

    #[test]
    fn compare_incomplete_each_evidence_reason() {
        for (ev, kind) in [
            (BodyEvidence::Truncated, BodyKind::Truncated),
            (BodyEvidence::Off, BodyKind::Off),
            (BodyEvidence::Unreadable, BodyKind::Unreadable),
            (BodyEvidence::NoResponse, BodyKind::NoResponse),
        ] {
            // identical bodies on both sides must still not be Equal
            let d = compare(&with(ev), &with(ev), &IgnoreList::default()).body;
            let r = Some(SideReason {
                kind,
                before_0_5_0: false,
            });
            assert_eq!(d, BodyDiff::Incomplete { a: r, b: r });
            let d = compare(&with(ev), &side(b"{}"), &IgnoreList::default()).body;
            assert_eq!(d, BodyDiff::Incomplete { a: r, b: None });
        }
    }

    #[test]
    fn compare_incomplete_pre_0_5_0_fallback() {
        let mut old = side(b"{}");
        old.evidence = None;
        old.body_stored = false;
        let d = compare(&old, &side(b"{}"), &IgnoreList::default()).body;
        assert_eq!(
            d,
            BodyDiff::Incomplete {
                a: Some(SideReason {
                    kind: BodyKind::Off,
                    before_0_5_0: true
                }),
                b: None
            }
        );
        old.body_stored = true;
        old.body_truncated = true;
        let d = compare(&old, &side(b"{}"), &IgnoreList::default()).body;
        assert!(matches!(
            d,
            BodyDiff::Incomplete {
                a: Some(SideReason {
                    kind: BodyKind::Truncated,
                    before_0_5_0: true
                }),
                ..
            }
        ));
        old.body_truncated = false;
        let d = compare(&old, &side(b"{}"), &IgnoreList::default()).body;
        assert_eq!(d, BodyDiff::Equal { ignored: 0 });
    }

    #[test]
    fn compare_cap_at_1000_keeps_total() {
        let mk = |n: i32| {
            let items: Vec<String> = (0..1500).map(|i| (i + n).to_string()).collect();
            format!("[{}]", items.join(","))
        };
        let d = body(&mk(0), &mk(1), &IgnoreList::default());
        let BodyDiff::Different { changes, total, .. } = d else {
            panic!()
        };
        assert_eq!(total, 1500);
        assert_eq!(changes.len(), MAX_CHANGES);
    }

    #[test]
    fn compare_values_cut_at_char_boundary() {
        let long = "é".repeat(400);
        let a = format!(r#"{{"k":"{long}"}}"#);
        let d = body(&a, r#"{"k":1}"#, &IgnoreList::default());
        let BodyDiff::Different { changes, .. } = d else {
            panic!()
        };
        let before = changes[0].before.as_ref().unwrap();
        assert!(before.cut);
        assert_eq!(before.text.chars().count(), MAX_VALUE_CHARS);
    }

    #[test]
    fn compare_too_deep_has_no_partial_list() {
        let deep = |n: usize, leaf: &str| format!("{}{leaf}{}", "[".repeat(n), "]".repeat(n));
        let d = body(&deep(200, "1"), &deep(200, "2"), &IgnoreList::default());
        assert_eq!(d, BodyDiff::TooDeep);
        let ok = body(&deep(100, "1"), &deep(100, "2"), &IgnoreList::default());
        assert!(matches!(ok, BodyDiff::Different { total: 1, .. }), "{ok:?}");
    }

    #[test]
    fn body_kind_text_matches_spec_wording() {
        assert_eq!(BodyKind::Truncated.text(), "body cut at 1 MiB; not compared");
        assert_eq!(
            BodyKind::Off.text(),
            "body not stored (history bodies off, or a `wisp-cli run` entry)"
        );
        assert_eq!(BodyKind::Unreadable.text(), "body could not be read when recorded");
        assert_eq!(
            BodyKind::NoResponse.text(),
            "no response body (request failed or was cancelled)"
        );
        assert_eq!(
            BodyKind::StoredUnavailable.text(),
            "Stored body unavailable (deleted or unreadable)"
        );
    }

    fn stored_entry(id_hint: &str) -> HistoryEntry {
        serde_json::from_value(serde_json::json!({
            "id": id_hint, "at": 0, "method": "Get", "url": "http://x/", "outcome": "Completed",
            "status": 200, "body_stored": true,
            "evidence": {"body": "stored", "headers": true}
        }))
        .unwrap()
    }

    fn unavailable_reason() -> Option<SideReason> {
        Some(SideReason {
            kind: BodyKind::StoredUnavailable,
            before_0_5_0: false,
        })
    }

    #[test]
    fn compare_unloadable_stored_entry_is_unavailable_not_equal() {
        let e = stored_entry("abc");
        let (a, b) = (
            Side::from_entry(&e, None, true),
            Side::from_entry(&e, None, true),
        );
        let c = compare(&a, &b, &IgnoreList::default());
        assert_eq!(
            c.body,
            BodyDiff::Incomplete {
                a: unavailable_reason(),
                b: unavailable_reason()
            }
        );
        assert!(c.headers.a_not_recorded && c.headers.b_not_recorded);
        assert_eq!(
            BodyKind::StoredUnavailable.text(),
            "Stored body unavailable (deleted or unreadable)"
        );
    }

    #[test]
    fn compare_deleted_bin_in_real_store_is_unavailable_not_equal() {
        use wisp_core::request::Request;
        use wisp_storage::collection::CollectionId;
        use wisp_storage::data_dir::DataDir;
        use wisp_storage::history::{HistoryStore, NewEntry, Outcome};
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::at(dir.path());
        let id = CollectionId::parse("abcdef0123456789").unwrap();
        let store = HistoryStore::open(&data, &id);
        let new = || NewEntry {
            at: std::time::SystemTime::UNIX_EPOCH,
            request_path: None,
            request: Request::default(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb: None,
            total: None,
            dns: None,
            connect: None,
            size: None,
            response_headers: Vec::new(),
            environment: None,
            assertions: Vec::new(),
            delivery: None,
        };
        let mut entries = Vec::new();
        for _ in 0..2 {
            let mut e = store.append(new(), Some(b"{}")).unwrap();
            e.evidence = Some(Evidence {
                body: BodyEvidence::Stored,
                headers: true,
            });
            let bin = store.body_path(&e.id).expect("bin written");
            std::fs::remove_file(bin).unwrap();
            entries.push(e);
        }
        let sa = store.load(&entries[0].id).unwrap();
        let sb = store.load(&entries[1].id).unwrap();
        let a = Side::from_entry(
            &entries[0],
            Some(&sa),
            store.body_path(&entries[0].id).is_some(),
        );
        let b = Side::from_entry(
            &entries[1],
            Some(&sb),
            store.body_path(&entries[1].id).is_some(),
        );
        let c = compare(&a, &b, &IgnoreList::default());
        assert_eq!(
            c.body,
            BodyDiff::Incomplete {
                a: unavailable_reason(),
                b: unavailable_reason()
            }
        );
    }
}
