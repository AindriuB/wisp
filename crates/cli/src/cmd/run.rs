//! `run`: send one saved request and map what happened to a document and an exit code.

use std::path::PathBuf;
use std::time::Duration;

use wisp_core::assert::Verdict;
use wisp_core::{ErrorKind, Method};
use wisp_service::run::{
    HistoryStatus, RunEnd, RunError, RunInput, RunProblem, RunReport, run_once,
};
use wisp_storage::history::Delivery;

use super::explain::problem;
use super::inspect::{Failure, cap};
use crate::dto::{CheckDoc, DynamicDoc, ErrorDoc, ProblemDoc, RunDoc, SCHEMA, TimingDoc};

pub const PASSED: u8 = 0;
pub const ASSERTION_FAILED: u8 = 1;
pub const NOT_PREPARED: u8 = 4;
pub const TRANSPORT_FAILED: u8 = 5;
pub const OUTCOME_UNKNOWN: u8 = 6;
pub const STATUS_UNEXPECTED: u8 = 7;

pub struct Args {
    pub collection: PathBuf,
    pub request: String,
    pub env: Option<String>,
    pub keychain: bool,
    pub history: bool,
}

/// The fixed text of a failure kind; the engine's message is never printed.
pub fn error_text(kind: &str) -> &'static str {
    match kind {
        "invalid_url" => "the URL is not valid",
        "dns" => "the host name could not be resolved",
        "connect" => "the connection failed",
        "tls" => "the TLS handshake failed",
        "timeout" => "the request timed out",
        "body" => "reading the response body failed",
        "body_file" => "the request body file could not be read",
        "auth" => "authentication failed",
        _ => "the request failed",
    }
}

fn kind_name(k: ErrorKind) -> &'static str {
    match k {
        ErrorKind::InvalidUrl => "invalid_url",
        ErrorKind::Dns => "dns",
        ErrorKind::Connect => "connect",
        ErrorKind::Tls => "tls",
        ErrorKind::Timeout => "timeout",
        ErrorKind::Body => "body",
        ErrorKind::BodyFile => "body_file",
        ErrorKind::Auth => "auth",
        ErrorKind::Other => "other",
    }
}

fn method_name(m: Method) -> &'static str {
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

fn delivery_name(d: Delivery) -> &'static str {
    match d {
        Delivery::NotSent => "not_sent",
        Delivery::Unknown => "unknown",
        Delivery::Responded => "responded",
    }
}

fn history_name(h: HistoryStatus) -> &'static str {
    match h {
        HistoryStatus::Off => "off",
        HistoryStatus::Recorded => "recorded",
        HistoryStatus::SkippedNoId => "skipped (collection has no id)",
        HistoryStatus::SkippedNotSent => "skipped (not sent)",
        HistoryStatus::Failed => "failed",
    }
}

fn problem_doc(p: &RunProblem) -> ProblemDoc {
    let fixed = |kind, name: Option<String>, message: String| ProblemDoc {
        kind,
        name,
        location: None,
        message: Some(message),
    };
    match p {
        RunProblem::Resolve(p) => problem(p),
        RunProblem::KeychainSecret { name } => fixed(
            "keychain-secret",
            Some(cap(name)),
            format!("secret {} is in the keychain; pass --keychain", cap(name)),
        ),
        RunProblem::MissingSecret { name } => fixed(
            "missing-secret",
            Some(cap(name)),
            format!("secret `{}` has no value in the keychain", cap(name)),
        ),
        RunProblem::KeychainUnavailable => fixed(
            "keychain-unavailable",
            None,
            "the keychain secrets could not be loaded".into(),
        ),
        RunProblem::UnknownStore => fixed(
            "unknown-secret-store",
            None,
            "the environment uses a secret store this version does not know".into(),
        ),
        RunProblem::InteractiveAuth => fixed(
            "interactive-auth",
            None,
            "OAuth2 authorization code needs interactive authorization".into(),
        ),
    }
}

/// The result name and exit code. Precedence: not prepared, transport, status, assertions.
fn classify(r: &RunReport) -> (&'static str, u8) {
    if r.end == RunEnd::NotPrepared {
        return ("not_prepared", NOT_PREPARED);
    }
    if r.end != RunEnd::Completed {
        return if r.delivery == Delivery::Unknown {
            ("outcome_unknown", OUTCOME_UNKNOWN)
        } else {
            ("transport_failed", TRANSPORT_FAILED)
        };
    }
    if r.status_ok != Some(true) {
        return ("status_unexpected", STATUS_UNEXPECTED);
    }
    if r.checks
        .iter()
        .any(|c| matches!(c.result.verdict, Verdict::Fail { .. }))
    {
        return ("assertion_failed", ASSERTION_FAILED);
    }
    ("passed", PASSED)
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn map(r: &RunReport) -> RunDoc {
    let (result, exit) = classify(r);
    RunDoc {
        schema: SCHEMA,
        command: "run",
        request: cap(&r.request),
        environment: r.environment.as_deref().map(cap),
        method: method_name(r.method),
        url: r.url.clone(),
        result,
        exit,
        delivery: delivery_name(r.delivery),
        retry_safe: r.retry_safe,
        status: r.status,
        reason: r.reason.clone(),
        expected_status: r.expected_status.clone(),
        status_ok: r.status_ok,
        timing_ms: TimingDoc {
            ttfb: r.ttfb.map(ms),
            total: r.total.map(ms),
        },
        size: r.size,
        error: match r.end {
            RunEnd::Failed(k) => Some(ErrorDoc { kind: kind_name(k) }),
            RunEnd::Cancelled => Some(ErrorDoc { kind: "other" }),
            _ => None,
        },
        assertions: r
            .checks
            .iter()
            .map(|c| {
                let (state, reason) = match &c.result.verdict {
                    Verdict::Pass => ("pass", None),
                    Verdict::Fail { .. } => ("fail", None),
                    Verdict::Skipped { reason } => ("skipped", Some(cap(reason))),
                };
                CheckDoc {
                    label: c.result.label.clone(),
                    state,
                    reason,
                }
            })
            .collect(),
        dynamic: r
            .dynamics
            .iter()
            .map(|d| DynamicDoc {
                name: d.name.clone(),
                value: d.value.clone(),
            })
            .collect(),
        history: history_name(r.history),
        problems: r.problems.iter().map(problem_doc).collect(),
    }
}

pub fn run(a: Args) -> Result<RunDoc, Failure> {
    let report = run_once(RunInput {
        collection: a.collection,
        request: a.request,
        env: a.env,
        keychain: a.keychain,
        history: a.history,
        data: None,
        stores: None,
    })
    .map_err(|RunError::Unreadable(m)| Failure::Unreadable(m))?;
    Ok(map(&report))
}
