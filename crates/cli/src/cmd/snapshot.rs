//! `snapshot`: one bounded, redacted, deterministic document of a collection's persisted
//! state. Built from the other commands' DTOs; nothing is written and no value is read
//! into output.

use std::path::Path;

use wisp_storage::collection::read_collection_id;
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::{environment_info, list_environments};
use wisp_storage::history::{AssertionState, Delivery, HistoryEntry, HistoryStore, Outcome};

use super::inspect::{Failure, cap, safe_url};
use super::{explain, inspect, validate};
use crate::dto::{
    EnvSnapshot, ExplainDoc, RecentCheck, RecentInfo, RequestSlot, SCHEMA, SNAPSHOT_SOURCE,
    SnapshotCounts, SnapshotDoc, SnapshotMinimal, ValidationSnapshot,
};

/// Neither the JSON nor the text rendering of the document exceeds this.
pub const MAX_BYTES: usize = 1024 * 1024;
const MAX_FINDINGS: usize = 500;
const MAX_ENVIRONMENTS: usize = 200;
const MAX_ENV_VARS: usize = 500;
const MAX_REQUEST_ROWS: usize = 500;

pub enum Out {
    Full(SnapshotDoc),
    Minimal(SnapshotMinimal),
}

pub struct Snapshot {
    pub doc: Out,
    /// Printed on stderr.
    pub notes: Vec<String>,
}

pub fn run(
    collection: &Path,
    env: Option<&str>,
    request: Option<&str>,
    recent: Option<usize>,
) -> Result<Snapshot, Failure> {
    build(collection, env, request, recent, MAX_BYTES)
}

fn mark(truncated: &mut Vec<String>, name: &str) {
    if !truncated.iter().any(|t| t == name) {
        truncated.push(name.to_owned());
    }
}

/// Serialised size as `--json` prints it.
#[cfg(test)]
fn size<T: serde::Serialize>(doc: &T) -> usize {
    serde_json::to_string_pretty(doc).map_or(usize::MAX, |s| s.len() + 1)
}

/// Bytes written, nothing kept. Refuses further writes once past `limit`, so a renderer
/// stops instead of producing a document nobody will use.
struct Count {
    n: usize,
    limit: usize,
}

impl Count {
    fn new(limit: usize) -> Self {
        Self { n: 0, limit }
    }
}

impl std::io::Write for Count {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.n = self.n.saturating_add(b.len());
        if self.n > self.limit {
            return Err(std::io::Error::other("over the limit"));
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The larger of the two renderings, so the bound holds whichever format is printed; the
/// text form escapes control characters and can be longer than the JSON. Rendering stops
/// once a form is past `limit`, and the result is then `usize::MAX`.
fn fsize(doc: &SnapshotDoc, limit: usize) -> usize {
    let mut c = Count::new(limit);
    if crate::output::snapshot_text(&mut c, doc).is_err() {
        return usize::MAX;
    }
    let mut j = Count::new(limit);
    if serde_json::to_writer_pretty(&mut j, doc).is_err() {
        return usize::MAX;
    }
    c.n.max(j.n.saturating_add(1))
}

fn environments(
    root: &Path,
    truncated: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Result<(Vec<EnvSnapshot>, usize), Failure> {
    let mut names = list_environments(root).map_err(|e| Failure::Unreadable(e.to_string()))?;
    names.sort();
    let total = names.len();
    if total > MAX_ENVIRONMENTS {
        mark(truncated, "environments");
    }
    let out = names
        .iter()
        .take(MAX_ENVIRONMENTS)
        .map(|n| {
            let mut vars = environment_info(root, n)
                .map(|i| i.var_names.into_iter().chain(i.secret_names).collect())
                .unwrap_or_else(|_| {
                    mark(truncated, &format!("environment:{}", cap(n)));
                    notes.push(format!(
                        "environment {} could not be read; its variables are not listed",
                        cap(n)
                    ));
                    Vec::new()
                });
            vars.sort();
            vars.dedup();
            let variable_count = vars.len();
            if variable_count > MAX_ENV_VARS {
                mark(truncated, "environment_variables");
            }
            EnvSnapshot {
                name: cap(n),
                variable_count,
                variables: vars.iter().take(MAX_ENV_VARS).map(|v| cap(v)).collect(),
            }
        })
        .collect();
    Ok((out, total))
}

fn recent_info(e: &HistoryEntry) -> RecentInfo {
    RecentInfo {
        at: e.at,
        request: e.request.as_deref().map(cap),
        method: format!("{:?}", e.method).to_ascii_uppercase(),
        url: safe_url(&e.url),
        outcome: match e.outcome {
            Outcome::Completed => "completed",
            Outcome::Failed { .. } => "failed",
            Outcome::Cancelled => "cancelled",
        },
        delivery: e.delivery.map(|d| match d {
            Delivery::NotSent => "not_sent",
            Delivery::Unknown => "unknown",
            Delivery::Responded => "responded",
        }),
        status: e.status,
        environment: e.environment.as_deref().map(cap),
        assertions: e
            .assertions
            .iter()
            .map(|a| RecentCheck {
                label: cap(&a.label),
                state: match a.state {
                    AssertionState::Pass => "pass",
                    AssertionState::Fail => "fail",
                    AssertionState::Skipped => "skipped",
                },
            })
            .collect(),
    }
}

/// Read-only: never migrates, locks, creates or writes. Returns the entries and why
/// there are none, when that is the case.
fn recent(collection: &Path, n: usize) -> (Vec<RecentInfo>, Option<String>) {
    let none = |why: &str| (Vec::new(), Some(why.to_owned()));
    let Ok(Some(id)) = read_collection_id(collection) else {
        return none("collection has no id; no history");
    };
    let Some(data) = DataDir::resolve() else {
        return none("no data directory; no history");
    };
    let dir = data.history_dir(&id);
    let Some(store) = HistoryStore::open_read_only(&dir) else {
        return none("no history for this collection yet");
    };
    match store.list(n) {
        Err(_) => none("history could not be read"),
        Ok(r) if r.entries.is_empty() && r.note.is_none() => {
            if dir.join("index.jsonl").is_file() {
                none("history has no entries")
            } else {
                none("history has no index")
            }
        }
        Ok(r) => (r.entries.iter().map(recent_info).collect(), r.note),
    }
}

fn cap_request(mut d: ExplainDoc, truncated: &mut Vec<String>) -> ExplainDoc {
    if d.variables.len() > MAX_REQUEST_ROWS {
        d.variables.truncate(MAX_REQUEST_ROWS);
        mark(truncated, "request_variables");
    }
    if d.headers.len() > MAX_REQUEST_ROWS {
        d.headers.truncate(MAX_REQUEST_ROWS);
        mark(truncated, "request_headers");
    }
    d
}

/// Keep the longest prefix of the tree that still fits, dropping entries from the end.
fn fit_tree(doc: &mut SnapshotDoc, limit: usize) {
    let mut sum = 0usize;
    let mut hi = 0usize;
    for n in &doc.tree {
        // compact length is a lower bound of the pretty length
        sum = sum.saturating_add(serde_json::to_string(n).map_or(usize::MAX, |s| s.len() + 1));
        if sum > limit {
            break;
        }
        hi += 1;
    }
    doc.tree.truncate(hi);
    let (mut lo, mut hi) = (0usize, doc.tree.len());
    let all = std::mem::take(&mut doc.tree);
    // largest n with size(first n) <= limit; n = 0 is the floor
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        doc.tree = all[..mid].to_vec();
        if fsize(&*doc, limit) <= limit {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    doc.tree = all[..lo].to_vec();
}

fn build(
    collection: &Path,
    env: Option<&str>,
    request: Option<&str>,
    recent_n: Option<usize>,
    limit: usize,
) -> Result<Snapshot, Failure> {
    let ins = inspect::run(collection, env)?;
    let val = validate::run(collection)?;
    let mut truncated = Vec::new();
    let mut notes = Vec::new();

    let (environments, env_total) = environments(collection, &mut truncated, &mut notes)?;

    if ins.truncated {
        mark(&mut truncated, "tree");
    }
    let finding_total = val.errors + val.warnings;
    let mut findings = val.findings;
    let findings_truncated = findings.len() > MAX_FINDINGS;
    if findings_truncated {
        findings.truncate(MAX_FINDINGS);
        mark(&mut truncated, "findings");
    }

    let request_doc = match request {
        Some(r) => Some(RequestSlot::Doc(Box::new(cap_request(
            explain::run(collection, r, env)?,
            &mut truncated,
        )))),
        None => None,
    };

    let recent = recent_n.map(|n| {
        let (entries, note) = recent(collection, n);
        if let Some(note) = note {
            notes.push(note);
        }
        entries
    });

    let tree_total = ins.nodes.len();
    let recent_total = recent.as_ref().map(Vec::len);
    let doc = SnapshotDoc {
        schema: SCHEMA,
        command: "snapshot",
        wisp_cli: env!("CARGO_PKG_VERSION"),
        source: SNAPSHOT_SOURCE,
        collection: ins.collection,
        environments,
        tree: ins.nodes,
        tree_truncated: ins.truncated,
        validation: ValidationSnapshot {
            errors: val.errors,
            warnings: val.warnings,
            findings,
            findings_truncated: findings_truncated || val.truncated,
        },
        request: request_doc,
        recent,
        truncated,
    };

    Ok(finish(
        doc,
        Counts {
            environments: env_total,
            tree: tree_total,
            findings: finding_total,
            recent: recent_total,
            request: request.map(|_| 1),
        },
        notes,
        limit,
    ))
}

struct Counts {
    environments: usize,
    tree: usize,
    findings: usize,
    recent: Option<usize>,
    request: Option<usize>,
}

/// Drop, in the spec's order, until the document fits `limit`; else the minimal form.
fn finish(mut doc: SnapshotDoc, counts: Counts, mut notes: Vec<String>, limit: usize) -> Snapshot {
    let mut fits = fsize(&doc, limit) <= limit;
    if !fits && doc.recent.as_ref().is_some_and(|r| !r.is_empty()) {
        doc.recent = Some(Vec::new());
        mark(&mut doc.truncated, "recent");
        fits = fsize(&doc, limit) <= limit;
    }
    if !fits && !doc.validation.findings.is_empty() {
        doc.validation.findings.clear();
        doc.validation.findings_truncated = true;
        mark(&mut doc.truncated, "findings");
        fits = fsize(&doc, limit) <= limit;
    }
    if !fits && !doc.tree.is_empty() {
        fit_tree(&mut doc, limit);
        doc.tree_truncated = true;
        mark(&mut doc.truncated, "tree");
        fits = fsize(&doc, limit) <= limit;
    }
    if !fits && matches!(doc.request, Some(RequestSlot::Doc(_))) {
        doc.request = Some(RequestSlot::Omitted { omitted: true });
        mark(&mut doc.truncated, "request");
        fits = fsize(&doc, limit) <= limit;
    }
    if !fits {
        let mut any = false;
        for e in &mut doc.environments {
            if !e.variables.is_empty() {
                e.variables.clear();
                any = true;
            }
        }
        if any {
            mark(&mut doc.truncated, "environment_variables");
        }
        fits = fsize(&doc, limit) <= limit;
    }
    if fits {
        return Snapshot {
            doc: Out::Full(doc),
            notes,
        };
    }

    let mut truncated = vec![
        "environments".to_owned(),
        "tree".to_owned(),
        "findings".to_owned(),
    ];
    if counts.request.is_some() {
        truncated.push("request".into());
    }
    if counts.recent.is_some() {
        truncated.push("recent".into());
    }
    notes.push("snapshot is too large; printing the counts only".into());
    Snapshot {
        doc: Out::Minimal(SnapshotMinimal {
            schema: SCHEMA,
            command: "snapshot",
            wisp_cli: env!("CARGO_PKG_VERSION"),
            source: SNAPSHOT_SOURCE,
            collection: doc.collection,
            counts: SnapshotCounts {
                environments: counts.environments,
                tree: counts.tree,
                findings: counts.findings,
                recent: counts.recent,
                request: counts.request,
            },
            truncated,
        }),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{CollectionInfo, NodeInfo};

    fn doc(n: usize) -> SnapshotDoc {
        SnapshotDoc {
            schema: SCHEMA,
            command: "snapshot",
            wisp_cli: "0",
            source: SNAPSHOT_SOURCE,
            collection: CollectionInfo {
                id: None,
                format: 1,
                folders: 0,
                requests: n,
                invalid: Some(0),
            },
            environments: Vec::new(),
            tree: (0..n)
                .map(|i| NodeInfo {
                    path: format!("r{i}.toml"),
                    kind: "request",
                    name: format!("R{i}"),
                    method: Some("GET".into()),
                    url: Some("https://h.test/".into()),
                    auth: Some("none"),
                    headers: Vec::new(),
                    valid: true,
                    has_description: false,
                    expected_status: Vec::new(),
                })
                .collect(),
            tree_truncated: false,
            validation: ValidationSnapshot {
                errors: 0,
                warnings: 0,
                findings: Vec::new(),
                findings_truncated: false,
            },
            request: None,
            recent: None,
            truncated: Vec::new(),
        }
    }

    fn counts(n: usize) -> Counts {
        Counts {
            environments: 0,
            tree: n,
            findings: 0,
            recent: None,
            request: None,
        }
    }

    #[test]
    fn snapshot_text_rendering_is_bounded_too() {
        // A bidi mark is 3 bytes in JSON and 10 once the text form escapes it.
        let mut d = doc(40);
        for n in &mut d.tree {
            n.name = "\u{200f}".repeat(400);
        }
        let limit = size(&d) + 100;
        assert!(size(&d) <= limit);
        let mut c = Count::new(usize::MAX);
        crate::output::snapshot_text(&mut c, &d).unwrap();
        assert!(c.n > limit, "text {} must exceed {limit}", c.n);
        let s = finish(d, counts(40), Vec::new(), limit);
        let Out::Full(f) = s.doc else {
            panic!("expected a trimmed full document")
        };
        assert!(f.tree.len() < 40);
        assert!(f.truncated.contains(&"tree".to_owned()));
        assert!(fsize(&f, limit) <= limit);
    }

    #[test]
    fn snapshot_fallback_when_nothing_fits() {
        let s = finish(doc(30), counts(30), Vec::new(), 300);
        let Out::Minimal(m) = s.doc else {
            panic!("expected the minimal form")
        };
        assert_eq!(m.counts.tree, 30);
        assert!(m.truncated.contains(&"tree".to_owned()));
        assert!(!s.notes.is_empty());
        assert!(size(&m) < 1500);
    }

    #[test]
    fn snapshot_tree_drops_from_the_end_to_fit() {
        let full = size(&doc(30));
        let s = finish(doc(30), counts(30), Vec::new(), full - 100);
        let Out::Full(f) = s.doc else { panic!() };
        assert!(f.tree.len() < 30 && !f.tree.is_empty());
        assert!(f.tree_truncated);
        assert!(f.truncated.contains(&"tree".to_owned()));
        assert!(size(&f) <= full - 100);
        assert_eq!(f.tree[0].path, "r0.toml");
    }

    #[test]
    fn snapshot_fallback_names_request_and_recent_when_present() {
        let mut d = doc(30);
        d.request = Some(RequestSlot::Omitted { omitted: true });
        d.recent = Some(Vec::new());
        let mut c = counts(30);
        c.request = Some(1);
        c.recent = Some(4);
        let s = finish(d, c, Vec::new(), 300);
        let Out::Minimal(m) = s.doc else {
            panic!("expected the minimal form")
        };
        assert_eq!(m.counts.request, Some(1));
        assert_eq!(m.counts.recent, Some(4));
        for name in ["environments", "tree", "findings", "request", "recent"] {
            assert!(m.truncated.contains(&name.to_owned()), "{name}");
        }
    }

    #[test]
    fn snapshot_counting_writer_stops_past_the_limit() {
        let d = doc(40);
        let mut full = Count::new(usize::MAX);
        crate::output::snapshot_text(&mut full, &d).unwrap();
        let mut c = Count::new(100);
        assert!(crate::output::snapshot_text(&mut c, &d).is_err());
        assert!(c.n < full.n, "stopped at {} of {}", c.n, full.n);
        assert_eq!(fsize(&d, 100), usize::MAX);
    }
}
