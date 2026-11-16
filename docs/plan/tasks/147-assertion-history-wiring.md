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
- [ ] AC2 On `Completed`, the `Sent` draft's assertions are evaluated once with `Facts { status, headers: &t.response.headers, body }` where `body` is the stored bytes when at most `BODY_CAP`, else `None` (so JSON checks report "skipped: body over 1 MiB"); outcomes are zipped with their definitions into `ResponseState.assertions`. A failed or cancelled send leaves it empty and records an empty list.
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
