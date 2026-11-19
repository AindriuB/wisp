# 195 — Harden the OpenAPI importer, the import writer and URL credentials (H4b)

**Repo:** .
**Round:** 0
**Depends on:** 189, 194
**Owns:**
- crates/storage/src/import/openapi.rs
- crates/storage/src/import/write.rs
- crates/storage/src/import/mod.rs (the URL helpers: `url_host`, `split_userinfo`, `may_hold_credentials` and their callers)
- crates/storage/tests/import.rs

## Goal
Close the remaining H4b ACs: the OpenAPI example budget charged while
serialising, and linear name uniqueness. Also close the 0.4.0 importer
follow-ups that can still leak a credential into an environment name or a
stored base URL, or that report the wrong error.

## Context
- docs/plan/tasks/99-hardening-0-2-0.md:75 (AC21), :93-94 (AC34, AC35); docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 6".
- crates/storage/src/import/openapi.rs:
  - ~:621 `to_string_pretty` (AC21; the brief's `:451-484` is stale);
  - :61 `spend`;
  - :389 the redundant `suspect ||`.
- crates/storage/src/import/write.rs: :194 `unique` (AC34), :133 `check_folder_rel`. AC35's `unique_stem` is no longer in `write.rs`; `crates/storage/src/ops.rs:254` has one. Find the import path's stem picker. If the import does not call a quadratic picker, record that with citations in the PR and tick AC3. If the fix needs `ops.rs`, stop and report.
- crates/storage/src/import/mod.rs: `url_host` :298, `split_userinfo` :438.
- PLAN.md "Follow-ups from task 170" (:103-114).
- Worktree: `./.claude/scripts/wt-new.sh . 195 importer-hardening-h4b`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-195`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (99-AC21) OpenAPI example text is charged to the budget while it is serialised, through a size-limited writer. The pretty string is never built past the budget. Test `h4b_deep_example_too_large` returns `TooLarge`.
- [ ] AC2 (99-AC34) `unique` uses a set plus a per-base counter, so n equal names cost O(n). Test `h4b_unique_many_equal_names`: 5,000 equal names give 5,000 distinct names. No timing assertion.
- [ ] AC3 (99-AC35) The import path's stem picker is linear, or the PR shows with citations that the import path has no quadratic picker.
- [ ] AC4 (170) `url_host` never returns a userinfo-shaped host. Test: `https://bob:1234?rest@h2/y` does not give `bob:1234`.
- [ ] AC5 (170) A non-digit password behind a `split_userinfo` carve-out never reaches a host-derived environment name unwarned. Test: `https://http:hunter2?x@h/y` neither yields a name containing `hunter2` nor stays silent.
- [ ] AC6 (181) A second credential left after the first strip (`https://u:p@x:pw/?a@h2/y`) is removed or warned about. Test.
- [ ] AC7 (179) `openapi.rs:389`: the redundant `suspect ||` is removed with no behaviour change.
- [ ] AC8 (170) `check_folder_rel` reports a missing folder with its own error variant or message, not "cannot parse input". Test.
- [ ] AC9 New tests in `tests/import.rs` are prefixed `h4b_`, with at least 7. Existing tests pass unmodified.
- [ ] AC10 No file outside Owns changes. No new dependency. fmt, clippy and the workspace suite pass.

## Checks
- `h4b-tests` command — AC1, AC2, AC4, AC5, AC6, AC8, AC9
  run: `cargo test -p wisp-storage --locked --test import h4b_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([7-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC10
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/import/openapi.rs$' -e '^crates/storage/src/import/write.rs$' -e '^crates/storage/src/import/mod.rs$' -e '^crates/storage/tests/import.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `budget-and-linear` read — AC1, AC2, AC3, AC7
  how: the reviewer checks that the writer stops at the budget before allocating more, that both pickers are linear, and that the AC7 change is behaviour-neutral.

## Out of scope
- `postman.rs` and `SecretSink`/`RequestCounter` (task 194).
- The curl `--frobnicate SECRETVAL` note and other NOTEs left in PLAN.
- `ops.rs` (stop and report if needed).
