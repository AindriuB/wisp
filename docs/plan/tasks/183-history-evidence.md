# 183 — Record why a history body or header is missing

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/storage/src/history.rs
- crates/storage/tests/history.rs (new `evidence_` tests; the canary edit in `redacts_secrets_in_url_params_vars_and_bodies`)
- crates/service/src/history.rs
- crates/service/src/run.rs (the `record` call at ~:599 only)
- crates/app/src/workspace.rs (history call sites only: `append_history` ~:2705-2760 and `spawn_record` ~:2864-2890)
- crates/app/src/ui/history.rs (`body_to_store` ~:26 only)

## Goal
Each new history entry says whether its response body is complete. If it is
not, it says why, and it says whether the response head was recorded. With
this, compare (spec feature 2) can state missing evidence instead of
guessing. Also fix the `1234` canary flake in the history suite.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 1": the field table and wording. Its "Frozen types touched" lists `HistoryEntry`.
- crates/storage/src/history.rs:323-362 `HistoryEntry` (additive serde fields, as with `delivery`), :1224-1300 `append`.
- crates/service/src/history.rs:31-48 `record`.
- crates/app/src/workspace.rs:2729 pre-filters the body by the setting and then passes `HistoryBodies::Store` (:2877); the recovered-store path calls `store.append` directly (:2879).
- crates/service/src/run.rs:599 (`run` passes `None` and `Skip`).
- docs/plan/known-flakes.md, first entry: the `1234` canary, `crates/storage/tests/history.rs:356-360`.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 183 history-evidence`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-183`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `HistoryEntry` gains `evidence: Option<Evidence>`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`. `Evidence { body: BodyEvidence, headers: bool }`. `BodyEvidence` serialises snake_case as `stored`, `truncated`, `off`, `unreadable` or `no_response`, with the meanings in the spec table. `append` writes `Some(..)` on every new entry. `body_stored` and `body_truncated` are still written with their current meanings.
- [ ] AC2 A hand-written 0.4.0 index line with no `evidence` key loads with `evidence: None`. Test `evidence_old_line_loads_as_none`.
- [ ] AC3 `headers` is true exactly when the entry's `status` is `Some`.
- [ ] AC4 The GUI path: setting off and Completed records `off`; setting on with a readable body records `stored`, or `truncated` when the body is over `BODY_CAP`; setting on with an unreadable body (`None` passed) records `unreadable`; Failed or Cancelled records `no_response`. The app passes the setting to `record`, or to the recovered-store append, instead of pre-filtering. Tests at the service level, prefix `evidence_`, one per case.
- [ ] AC5 `wisp-cli run --history`: Completed records `off`, Failed records `no_response`. Test `evidence_run_*` in service.
- [ ] AC6 `redacts_secrets_in_url_params_vars_and_bodies` uses the canary `918273645` in place of `1234` for `pin_key`, and its assertions check that canary. Nothing else in that test changes.
- [ ] AC7 No file outside Owns changes. No new dependency. Every existing test passes unmodified except the AC6 edit. fmt, clippy and the workspace suite pass.

## Checks
- `evidence-storage` command — AC1, AC2, AC3
  run: `cargo test -p wisp-storage --locked --test history evidence_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `evidence-service` command — AC4, AC5
  run: `cargo test -p wisp-service --locked --lib evidence_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([6-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `canary` command — AC6
  run: `rg -q '918273645' crates/storage/tests/history.rs && cargo test -p wisp-storage --locked --test history redacts_secrets_in_url_params_vars_and_bodies 2>&1 | grep -E 'test result: ok\. 1 passed'`
- `scope` command — AC7
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/history.rs$' -e '^crates/storage/tests/history.rs$' -e '^crates/service/src/history.rs$' -e '^crates/service/src/run.rs$' -e '^crates/app/src/workspace.rs$' -e '^crates/app/src/ui/history.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `call-sites` read — AC4, AC5
  how: the reviewer checks that the `workspace.rs` and `run.rs` edits are limited to the history call sites, and that no path can record `unreadable` when the setting was off.

## Out of scope
- Any compare logic, or showing evidence in the History panel (tasks 188 and 192).
- The `delivery` field and the history redaction rules.
- Rewriting old entries to add `evidence`.
