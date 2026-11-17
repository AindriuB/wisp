//! What a finished request amounts to: its checks, its delivery state and its history entry.

use std::time::{Duration, SystemTime};

use wisp_core::assert::{Assertion, AssertionResult, Facts, evaluate};
use wisp_core::{EngineError, Header};
use wisp_storage::history::{AssertionRecord, Delivery, NewEntry, Outcome};

use crate::send::SentRecord;

/// One evaluated assertion. The definition is kept beside the result so the
/// Checks pane shows the expected value as sent, after the draft changes.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    pub assertion: Assertion,
    pub result: AssertionResult,
}

/// How a send ended, as the adapter copies it from the terminal event.
#[derive(Debug, Clone, Copy)]
pub enum Terminal<'a> {
    /// `body_prefix` is the start of the body, at most the check cap plus one byte.
    Completed {
        body_prefix: Option<&'a [u8]>,
    },
    /// `nothing_sent` is copied from `Event::Failed`.
    Failed {
        error: &'a EngineError,
        nothing_sent: bool,
    },
    Cancelled,
}

/// The timings the engine reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResponseTimings {
    pub ttfb: Option<Duration>,
    pub total: Option<Duration>,
    pub dns: Option<Duration>,
    pub connect: Option<Duration>,
}

/// What arrived of the response.
#[derive(Debug, Clone, Copy)]
pub struct Response<'a> {
    pub status: Option<u16>,
    pub headers: &'a [Header],
    pub timings: ResponseTimings,
    pub size: Option<u64>,
    /// A `Headers` event arrived.
    pub headers_seen: bool,
}

/// Everything derived at a terminal event.
#[derive(Debug, Clone)]
pub struct Finished {
    pub checks: Vec<CheckOutcome>,
    pub delivery: Delivery,
    /// Whether the status is acceptable; `None` without a status.
    pub status_ok: Option<bool>,
    pub entry: NewEntry,
}

/// Evaluate `assertions` over a response; each outcome keeps its definition.
pub fn check_outcomes(
    assertions: &[Assertion],
    status: u16,
    headers: &[Header],
    body: Option<&[u8]>,
) -> Vec<CheckOutcome> {
    let facts = Facts {
        status,
        headers,
        body,
    };
    assertions
        .iter()
        .cloned()
        .zip(evaluate(assertions, &facts))
        .map(|(assertion, result)| CheckOutcome { assertion, result })
        .collect()
}

/// The history records of `outcomes`, in order: label, fingerprint and state only.
pub fn assertion_records(outcomes: &[CheckOutcome]) -> Vec<AssertionRecord> {
    outcomes
        .iter()
        .map(|o| AssertionRecord::from_result(&o.assertion, &o.result))
        .collect()
}

/// Whether the request reached the server: a response head arrived, or the engine proved
/// that no byte left. Anything unproven is `Unknown`.
pub fn delivery(terminal: &Terminal, headers_seen: bool) -> Delivery {
    match (terminal, headers_seen) {
        (_, true) => Delivery::Responded,
        (
            Terminal::Failed {
                nothing_sent: true, ..
            },
            false,
        ) => Delivery::NotSent,
        _ => Delivery::Unknown,
    }
}

/// `status` is in `expected` when that is non-empty, else below 400.
pub fn status_ok(status: u16, expected: &[u16]) -> bool {
    if expected.is_empty() {
        status < 400
    } else {
        expected.contains(&status)
    }
}

/// Derive the checks, the delivery state and the history entry at a terminal event.
/// Checks run only for a completed response.
pub fn finish(
    record: SentRecord,
    response: &Response,
    terminal: &Terminal,
    expected_status: &[u16],
) -> Finished {
    let checks = match terminal {
        Terminal::Completed { body_prefix } => check_outcomes(
            &record.draft.assertions,
            response.status.unwrap_or(0),
            response.headers,
            *body_prefix,
        ),
        _ => Vec::new(),
    };
    let delivery = delivery(terminal, response.headers_seen);
    let status_ok = response.status.map(|s| status_ok(s, expected_status));
    let outcome = match terminal {
        Terminal::Completed { .. } => Outcome::Completed,
        Terminal::Failed { error, .. } => Outcome::Failed { kind: error.kind },
        Terminal::Cancelled => Outcome::Cancelled,
    };
    let t = response.timings;
    let entry = NewEntry {
        at: SystemTime::now(),
        request_path: record.request,
        request: record.draft,
        outcome,
        status: response.status,
        ttfb: t.ttfb,
        total: t.total,
        dns: t.dns,
        connect: t.connect,
        size: response.size,
        response_headers: response.headers.to_vec(),
        environment: record.environment,
        assertions: assertion_records(&checks),
        delivery: Some(delivery),
    };
    Finished {
        checks,
        delivery,
        status_ok,
        entry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::ErrorKind;
    use wisp_core::assert::{Expected, Op, Verdict};
    use wisp_storage::history::{AssertionState, BODY_CAP};

    fn status_is(code: i64) -> Assertion {
        Assertion::Status {
            op: Op::Equals,
            expected: Expected::Int(code),
        }
    }

    #[test]
    fn check_outcomes_records_pass_fail_skipped_in_order() {
        let list = [
            status_is(200),
            status_is(500),
            Assertion::Json {
                path: "a".into(),
                op: Op::Exists,
                expected: Expected::Bool(true),
            },
        ];
        let outcomes = check_outcomes(&list, 200, &[], None);
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes[1].assertion, list[1]);
        assert!(matches!(outcomes[0].result.verdict, Verdict::Pass));
        assert!(matches!(outcomes[1].result.verdict, Verdict::Fail { .. }));
        assert!(matches!(
            outcomes[2].result.verdict,
            Verdict::Skipped { .. }
        ));
        let records = assertion_records(&outcomes);
        let states: Vec<_> = records.iter().map(|r| r.state).collect();
        assert_eq!(
            states,
            [
                AssertionState::Pass,
                AssertionState::Fail,
                AssertionState::Skipped
            ]
        );
        assert_eq!(records[1].hash, list[1].fingerprint());
        assert!(check_outcomes(&[], 200, &[], None).is_empty());
    }

    #[test]
    fn check_outcomes_over_cap_body_skips_with_over_limit_reason() {
        let list = [Assertion::Json {
            path: "$.a".into(),
            op: Op::Exists,
            expected: Expected::Bool(true),
        }];
        let big = vec![b'x'; BODY_CAP + 1];
        let outcomes = check_outcomes(&list, 200, &[], Some(&big));
        match &outcomes[0].result.verdict {
            Verdict::Skipped { reason } => assert!(reason.contains("over 1 MiB"), "{reason}"),
            v => panic!("expected skipped, got {v:?}"),
        }
    }

    #[test]
    fn history_records_hold_no_expected_or_header_values() {
        let list = [
            Assertion::Header {
                name: "x-thing".into(),
                op: Op::Equals,
                expected: Expected::Str("CANARY-EXPECTED".into()),
            },
            status_is(200),
        ];
        let headers = [Header {
            name: "x-thing".into(),
            value: "CANARY-HEADER".into(),
        }];
        let records = assertion_records(&check_outcomes(&list, 200, &headers, None));
        let text = format!("{records:?}");
        assert!(!text.contains("CANARY"), "{text}");
    }

    #[test]
    fn delivery_follows_the_table() {
        let err = EngineError {
            kind: ErrorKind::Connect,
            message: String::new(),
        };
        let failed = |nothing_sent| Terminal::Failed {
            error: &err,
            nothing_sent,
        };
        let done = Terminal::Completed { body_prefix: None };
        assert_eq!(delivery(&done, true), Delivery::Responded);
        assert_eq!(delivery(&failed(false), true), Delivery::Responded);
        assert_eq!(delivery(&failed(true), true), Delivery::Responded);
        assert_eq!(delivery(&failed(true), false), Delivery::NotSent);
        assert_eq!(delivery(&failed(false), false), Delivery::Unknown);
        assert_eq!(delivery(&Terminal::Cancelled, true), Delivery::Responded);
        assert_eq!(delivery(&Terminal::Cancelled, false), Delivery::Unknown);
    }

    #[test]
    fn status_ok_uses_the_list_or_the_400_line() {
        assert!(status_ok(399, &[]));
        assert!(!status_ok(400, &[]));
        assert!(!status_ok(404, &[]));
        assert!(!status_ok(200, &[201]));
        assert!(status_ok(201, &[201]));
    }

    /// The `NewEntry` the app built before this logic moved (task 161 AC1), as `Debug` text:
    /// this crate has no JSON dependency. The app's JSON golden was committed before the move.
    const GOLDEN_ENTRY: &str = r#"NewEntry { at: SystemTime { tv_sec: 10, tv_nsec: 0 }, request_path: Some("a.toml"), request: Request { name: "Untitled", method: Get, url: "http://example.test/x", path: [], params: [], headers: [], auth: Inherit, body: None, vars: [], settings: RequestSettings { timeout: None, follow_redirects: true, verify_tls: true }, assertions: [Status { op: Equals, expected: Int(200) }, Status { op: Equals, expected: Int(500) }], description: "", expected_status: [] }, outcome: Failed { kind: Timeout }, status: Some(200), ttfb: Some(5ms), total: Some(9ms), dns: Some(2ms), connect: Some(3ms), size: Some(2), response_headers: [Header { name: "X", value: "<redacted>" }], environment: Some("dev"), assertions: [AssertionRecord { label: "status equals", hash: "123681db9b04d061", state: Pass }, AssertionRecord { label: "status equals", hash: "123681db9b04d061", state: Fail }], delivery: None }"#;

    #[test]
    fn prepare_then_finish_builds_the_entry_the_app_used_to() {
        use crate::plan::{SendInput, prepare};
        use wisp_core::request::{CollectionDefaults, Request};
        use wisp_core::resolve::Fresh;

        let draft = Request {
            name: "Untitled".into(),
            url: "http://example.test/x".into(),
            assertions: vec![status_is(200), status_is(500)],
            ..Request::default()
        };
        let defaults = CollectionDefaults::default();
        let plan = prepare(
            &SendInput {
                draft: &draft,
                tab_path: Some(std::path::Path::new("a.toml")),
                origin: None,
                tree: None,
                empty_defaults: &defaults,
                env: None,
            },
            &mut Fresh,
        )
        .unwrap();
        let record = SentRecord {
            request: crate::plan::recorded_request(plan.scope_path.as_deref()),
            draft: draft.clone(),
            root: None,
            environment: Some("dev".into()),
        };
        let headers = [Header {
            name: "X".into(),
            value: "y".into(),
        }];
        let err = EngineError {
            kind: ErrorKind::Timeout,
            message: "timed out".into(),
        };
        let response = Response {
            status: Some(200),
            headers: &headers,
            timings: ResponseTimings {
                ttfb: Some(Duration::from_millis(5)),
                total: Some(Duration::from_millis(9)),
                dns: Some(Duration::from_millis(2)),
                connect: Some(Duration::from_millis(3)),
            },
            size: Some(2),
            headers_seen: true,
        };
        // The golden's checks came from a completed response; its terminal was a failure.
        let mut finished = finish(
            record,
            &response,
            &Terminal::Failed {
                error: &err,
                nothing_sent: false,
            },
            &[],
        );
        assert_eq!(finished.delivery, Delivery::Responded);
        assert_eq!(finished.status_ok, Some(true));
        // The checks are skipped for a failure; take the completed run's records instead.
        finished.entry.assertions = assertion_records(&check_outcomes(
            &draft.assertions,
            200,
            &headers,
            Some(b"{}"),
        ));
        finished.entry.delivery = None;
        finished.entry.at = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        assert_eq!(format!("{:?}", finished.entry), GOLDEN_ENTRY);
    }
}
