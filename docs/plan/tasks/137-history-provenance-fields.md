# 137 — Record environment name and assertion results in history entries

**Repo:** .
**Depends on:** 131
**Owns:**
- crates/storage/src/history.rs
- crates/storage/tests/history.rs
- crates/app/src/workspace.rs (insert-only: the two new fields in the `NewEntry`/`HistoryEntry` literals near lines 2677, 3972, 4301, set to `None`/`Vec::new()`)
- crates/app/src/ui/history.rs (insert-only: the same in the literals near lines 320, 481)

## Goal
History entries gain the environment name and per-assertion records (label,
definition fingerprint, state), never a value. Split from task 131 because
`HistoryEntry` and `NewEntry` have no `Default` and five `crates/app`
literals must gain the fields in the same change for the workspace to build.
Not wave 1: it is in the `workspace.rs` serial queue, before 2A (or folded
into 2A), and before H1 (both edit `history.rs`).

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Feature 3": Data model (history bullet), example index fragment, Redaction rules for history
- crates/storage/src/history.rs:162-190 `HistoryEntry`; :196-208 `NewEntry`; `AssertionRecord` from task 131
- Worktree `wt-new.sh . 137 history-provenance-fields`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-137`

## Acceptance
- [ ] AC1 `HistoryEntry` and `NewEntry` gain `environment: Option<String>` and `assertions: Vec<AssertionRecord>`, `serde(default)` on `HistoryEntry`; `append` writes them into the index line (keys absent when `None`/empty is acceptable).
- [ ] AC2 An index line written by 0.2.0 (no new keys) loads with `None` and empty (test with a literal 0.2.0 line).
- [ ] AC3 Round-trip: an entry with an environment and three records (pass, fail, skipped) reads back equal.
- [ ] AC4 Canary: the expected and actual values of a failing header assertion (task 131 AC10 sense) and the actual value of a failing non-header assertion appear nowhere in `index.jsonl` or `<id>.req.json`. Non-credential response headers stay in `<id>.req.json` as in 0.2.0, even when one equals a header assertion's actual value (see Decisions).
- [ ] AC5 The `crates/app` diff is exactly the two field insertions per listed literal; no behaviour change in the app (production `NewEntry` sets `None` and empty; 2F wires real values).

## Checks
- `storage-history` command — AC1-AC4
  run: `cargo test -p wisp-storage --test history && cargo test -p wisp-storage --lib history`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `app-diff` read — AC5
  how: `git diff main...HEAD -- crates/app` shows only `environment: None,` and `assertions: Vec::new(),` insertions.

## Out of scope
- Evaluating assertions in the Completed handler, `Sent.environment`, history chips, Resend notice: 2F.
- H1 history hardening (AC50, AC8, AC9, AC48, AC17, 62-1, 62-2).

## Decisions
- orchestrator, 2026-11-15 — AC4 "header value" means the expected or actual value carried by a header assertion (as in task 131 AC10 and its CANARY-HEADER-EXPECTED test). Those values never appear in the history index line or in `<id>.req.json`: the `.req.json` copy drops assertions (spec:212) and assertion records hold label/fingerprint/state only (spec:210). Non-credential response headers stay in `<id>.req.json` as in 0.2.0 (only Set-Cookie and credential-named headers are redacted), including when one equals a header assertion's actual value; that is not a canary failure.

## Review round 1 — findings
Reviewed HEAD d8a17cd.
- [SPEC] AC4 "response header value" was ambiguous. Settled by the 2026-11-15 decision above; AC4 reworded.
- [P2] crates/storage/tests/history.rs:918-948 `provenance_canary_values_never_stored` — HEADER-CANARY sits inside a Status assertion's actual text, so no header assertion goes through `AssertionRecord::from_result`. Fix: use an `Assertion::Header` whose expected value and failing actual are both canaries, and make the stored request carry that assertion so the `.req.json` drop path is exercised.
