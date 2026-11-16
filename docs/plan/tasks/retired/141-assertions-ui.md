# 141 — Edit assertions in the request editor and show results in the Checks pane

**Repo:** .
**Depends on:** 139
**Owns:**
- crates/app/src/ui/request/assertions.rs (replaces the 139 stub)
- crates/app/src/ui/response/checks.rs (replaces the 139 stub)
- crates/app/src/ui/kv.rs (insert-only, and only if a shared row widget is reused)

## Goal
A request's `[[assert]]` entries can be added, removed, reordered and edited in
an "Assertions" sub-tab, and after a send the "Checks" response sub-tab lists
each check as pass, fail or skipped. Evaluation itself is wired by 147; this
task only renders `ResponseState.assertions`.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md Feature 3 "Behaviour", "File format" validity matrix, "Expected values are literals", "Forward compatibility"
- crates/core/src/assert.rs:39-120 `Op`, `Expected`, `Assertion` (incl. `Unrecognised`), `from_parts`; :185 `label`; :461 `check_regex`; :565 `check_path`; :282-330 `Verdict`, `AssertionResult`
- 139: `CheckOutcome` and `ResponseState.assertions` in crates/app/src/tab.rs; `DirtySections.assertions`
- crates/app/src/ui/request/headers.rs — editor shape to follow (`tab.edit(...)`, subscriptions)
- crates/app/src/theme.rs — theme tokens only
- Worktree: `./.claude/scripts/wt-new.sh . 141 assertions-ui`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-141`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 The editor lists `draft.assertions` with kind (status, header, json), target (header name or JSON path), operator and expected value controls limited to the spec's validity matrix; add, remove, move up/down; every change goes through `tab.edit` so `DirtySections.assertions` flags it.
- [ ] AC2 Inline errors come from a pure `fn row_error(&RowInput) -> Option<String>` using `check_regex` and `check_path`: invalid regex, invalid path, status outside 100-599, a 51st assertion refused, and a warning (not an error) when an expected value contains `{{`. Table test covers each.
- [ ] AC3 `Assertion::Unrecognised` entries render read-only with their reason, keep their position, and survive any edit of other rows (test: editing row 0 of `[Unrecognised, Status]` leaves the `Unrecognised` value byte-equal).
- [ ] AC4 `ChecksPane` lists each `CheckOutcome` with its label and pass/fail/skipped; a fail shows expected (from the definition) and actual, each cut to 80 characters by a pure `fn detail_text(&CheckOutcome) -> Option<String>` (tested: cut at 80, skipped shows its reason, pass shows none). With no outcomes it says "No checks ran".
- [ ] AC5 No assertion value reaches a log: no `tracing` call in either file formats an expected or actual value (read check), and theme tokens only. Full suite, fmt and clippy pass.

## Checks
- `assertions-ui` command — AC2, AC3
  run: `cargo test -p wisp-app --bin wisp ui::request::assertions 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `checks-pane` command — AC4
  run: `cargo test -p wisp-app --bin wisp ui::response::checks 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `editor-shape` read — AC1, AC5
  how: edits go through `tab.edit`; controls offer only matrix-valid kind/op pairs; no `tracing::` line takes an expected or actual value; no colour literals.

## Out of scope
- Running assertions, `Sent.environment`, history records and chips (147).
- `tab.rs`, `draft.rs`, `ui/request/mod.rs`, `ui/response/mod.rs` (139 owns the seams; the "3/4 checks" status text is 139's).
- Variables inside expected values (later release); codec changes in `wisp-storage`.

## Outcome
Merged as PR #100 (a28e5e7, 2026-11-16) at bb12c83. The Assertions sub-tab edits `[[assert]]` entries with matrix-valid controls, inline errors from `row_error`, and read-only `Unrecognised` rows that survive other edits; the Checks pane lists pass, fail and skipped with 80-character detail. A schema-invalid edit keeps the last schema-valid draft assertion so Save and reopen stay editable. Evaluation and history wiring are task 147; visual hand testing is task 149.
