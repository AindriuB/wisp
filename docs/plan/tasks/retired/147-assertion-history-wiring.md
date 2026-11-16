# 147 — Run assertions after each send and record environment and checks in history

**Repo:** .
**Depends on:** 139, 143 (serial `workspace.rs` queue: 139 → 143 → 147)
**Owns:**
- crates/app/src/workspace.rs — SERIAL FILE, edit (c): `Sent` (:218-225), `submit_plan`, the `Event::Completed` arm (:2592-2615) and `append_history` (:2644-2728), `sanitized_origin` (:819) and the `resend_notice` call in `prepare_send` (:1128-1136) only; plus tests
- crates/app/src/ui/history.rs

## Goal
When a send gets a response, the request's assertions (as sent) are evaluated
over the status, headers and the body bytes already read for history, the
outcomes land in `ResponseState.assertions` for the Checks pane, and the
history entry records the environment name and one record per assertion
(label, fingerprint, state; never a value). The history panel shows both, and
the Resend notice says assertions are not resent.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md Feature 3 "Behaviour" (no response, no results; 1 MiB limit), "Redaction rules for history", "Placement and workspace.rs edits"
- crates/core/src/assert.rs:333 `Facts` (`body: Option<&[u8]>`), :384 `evaluate`; crates/storage/src/history.rs:25 `BODY_CAP`, :38-56 `AssertionRecord::from_result`
- 139: `CheckOutcome`, `ResponseState.assertions` (crates/app/src/tab.rs)
- 137 already added `environment` / `assertions` to `NewEntry` and `HistoryEntry`, set to `None` / empty in every app literal; this task sets real values in production only
- crates/app/src/workspace.rs:2601 `stored_body = bodies.read(body, 0, BODY_CAP + 1)` — a read longer than `BODY_CAP` means "over 1 MiB"
- crates/app/src/ui/history.rs:65 `resend_notice` and its tests :370-395
- Worktree: `./.claude/scripts/wt-new.sh . 147 assertion-history-wiring`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-147`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 `Sent` gains `environment: Option<String>`, set in `submit_plan` from `plan.prepared.environment`, and passed as `NewEntry.environment`.
- [ ] AC2 On `Completed`, the `Sent` draft's assertions are evaluated once with `Facts { status, headers: &t.response.headers, body }` where `body` is `Some` of the stored bytes exactly as read by `bodies.read(body, 0, BODY_CAP + 1)`, unfiltered (core's own size check turns a read longer than `BODY_CAP` into "body over 1 MiB"), and `None` only when that read fails (core then reports "no response body"; accepted). A test drives a body longer than `BODY_CAP` through the evaluation and asserts a JSON check's skip reason is "body over 1 MiB". Outcomes are zipped with their definitions into `ResponseState.assertions`. A failed or cancelled send leaves it empty and records an empty list.
- [ ] AC3 `NewEntry.assertions` holds `AssertionRecord::from_result` per outcome, in order. A pure helper maps outcomes to records and is tested (pass, fail, skipped; order kept).
- [ ] AC4 A GPUI or pure test with a canary expected value and a canary response header value shows neither in the `NewEntry` handed to the store.
- [ ] AC5 The history list shows the entry's environment name (or nothing when `None`) and a checks chip `"<passed>/<total> checks"` when the list is non-empty, failures marked; a pure test covers the chip text.
- [ ] AC6 `sanitized_origin` sets `HistoryOrigin.had_assertions` from the entry's records (field added by 139), and `resend_notice` appends "; assertions are not resent" when it is true; the existing notice tests still pass and one new case covers it.
- [ ] AC7 The workspace.rs diff is limited to the listed functions and tests; no `Command`/`Event` change; full suite, fmt and clippy pass.

## Checks
- `app-wiring` command — AC2-AC6
  run: `cargo test -p wisp-app --bin wisp 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `history-ui` command — AC5, AC6
  run: `cargo test -p wisp-app --bin wisp ui::history 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `serial-edit` read — AC1, AC7
  how: `git diff main...HEAD -- crates/app/src/workspace.rs` touches only `Sent`, `submit_plan`, the Completed arm, `append_history`, `sanitized_origin`, the `resend_notice` call and tests; `crates/core` is untouched.

## Out of scope
- The Checks pane and editor (141); the history store itself (144).
- Re-running assertions on Resend (spec: the stored copy has none, by design).
- The environment guard (wave 3, 3A).

## Wave stopped — step 2 — AC2 mechanism contradicts core: `None` gives "no response body", not "body over 1 MiB" (crates/core/src/assert.rs:436-437); reviewed HEAD 64ae49a

## Amendment 1
2026-11-16, user decision: AC2 no longer passes `body = None` for an over-cap body. Core maps `None` to "no response body" and gives "body over 1 MiB" only for `Some(b)` longer than `MAX_JSON_BODY` (= `BODY_CAP`; crates/core/src/assert.rs:436-437), while the spec requires the "body over 1 MiB" text (docs/plan/specs/2026-11-14-wisp-0-3-0.md:152). So `Facts.body` is the unfiltered `BODY_CAP + 1` read, `None` only on read failure, and a test now proves the over-cap skip reason.

## Wave resumed — user accepted amending AC2 (Amendment 1): pass the stored bytes unfiltered; test the over-cap skip reason

## Review round 1 — findings
Reviewed HEAD 64ae49af41ca6dadcb9c49ae3c5af7c8128495b2.
- [P2] crates/app/src/workspace.rs:2840 — The Completed arm still filters the stored body with `.filter(|b| b.len() <= BODY_CAP)`, so an over-cap body still reaches `evaluate` as `None`, which Amendment 1 to AC2 removed. Fix: pass `stored_body.as_deref()` unfiltered, so `Facts.body` is `None` only when the read fails. Input: a send whose response body is BODY_CAP+1 bytes or more, with a `json` assertion → the Checks pane and history show "no response body" (crates/core/src/assert.rs:436); the spec requires "body over 1 MiB" (assert.rs:437; spec :152).
- [P2] crates/app/src/workspace.rs:4177-4230 (tests) — No test sends a body longer than `BODY_CAP` through the evaluation and checks for the skip reason "body over 1 MiB", which amended AC2 requires. Both new tests pass `None`. One way: `check_outcomes(&[json assertion], 200, &[], Some(&vec![b'x'; BODY_CAP + 1]))` and assert the skip reason; this only proves the fix if the Completed arm passes the read bytes through unchanged.

## Wave verified — dbcf6791195e8018cf4a7d13d4494049faa5b714

## Outcome
Merged 2026-11-16 via the task branch PR, verified at dbcf679 (tester PASS, reviewer APPROVE). `Sent` carries the environment name; `Completed` evaluates the draft's assertions over status, headers and the unfiltered `BODY_CAP + 1` body read, fills `ResponseState.assertions`, and records the environment and one `AssertionRecord` per outcome in history. The history list shows both, and the Resend notice says assertions are not resent. AC2 was amended (Amendment 1) and review round 1 fixed the filter and added the over-cap test. Follow-ups are in PLAN under "Follow-ups from task 147".
