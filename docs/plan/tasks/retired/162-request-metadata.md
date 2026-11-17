# 162 — Add description and expected_status to requests (1C, D3)

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/core/src/request.rs
- crates/core/src/resolve/report.rs
- crates/core/src/resolve/report_tests.rs
- crates/storage/src/request.rs
- crates/storage/src/codec.rs
- crates/storage/src/import/curl.rs, crates/storage/src/import/openapi.rs, crates/storage/src/import/mod.rs (literal sites only)
- crates/storage/tests/request_meta.rs (new)

## Goal
A request file may carry a top-level `description` and `expected_status`. They
load, save (keep-what-exists), compare for the disk-change banner, survive in
history's request copy, and appear in the resolution report that Explain and
`wisp-cli explain` read. `format = 1` is unchanged.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 4" (:309-342), Waves 1C (:459).
- crates/core/src/request.rs:310-323 `Request`, :325-340 `Default`.
- crates/storage/src/request.rs:106 literal, :129-138 the `settings.timeout_ms` invalid-key pattern to copy, :353 `requests_equal`, :569-600 `write_request` (keep-what-exists for `[auth]` :571, :592), tests :1328-1771.
- crates/storage/tests/assert_codec.rs:15, :99 — the 0.3.0 byte-for-byte fixture tests to imitate.
- crates/core/src/resolve/report.rs:403 the only `ResolutionReport` literal; `Shown::Text`.
- Literal sites: `import/curl.rs:616`, `import/openapi.rs:504`, `import/mod.rs:108`. Filling `description` from importers is 170, not this task.
- `resolve/engine.rs` and `resolve/mod.rs` belong to 159 (parallel): do not edit them; the new report fields use existing types.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 162 request-metadata`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-162`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `Request` gains `description: String` and `expected_status: Vec<u16>`, both `#[serde(default)]`; `Default` sets them empty; the listed literal sites compile with empty values and no importer behaviour changes.
- [ ] AC2 Loading: `description` is any TOML string. `expected_status` is an integer 100-599 or an array of 1 to 16 such integers; anything else (0, 600, a string, an empty array, 17 entries, a float) is an invalid-key error naming `expected_status`, as `settings.timeout_ms` is. A table test covers each case.
- [ ] AC3 Saving: both are written as top-level keys only when non-empty or already present in the document; a single status is written as an integer, two or more as an array. Test: a file without either key round-trips byte-for-byte after a URL-only change; a file with both keeps them.
- [ ] AC4 `requests_equal` compares both fields; a test shows an outside edit to either is a difference.
- [ ] AC5 Fixture test (`crates/storage/tests/request_meta.rs`) shows a 0.3.0-shaped save path keeps unknown top-level keys: a file with `description` and `expected_status`, loaded and saved after an unrelated change by code that ignores them, keeps both keys byte-for-byte (as `assert_codec.rs:15` does for `[[assert]]`).
- [ ] AC6 `ResolutionReport` gains `description` (capped at 1,000 characters with a trailing "…", typed `Shown::Text`, never expanded: a `{{var}}` stays literal) and `expected_status: Vec<u16>`. Tests: a 1,500-character description is cut to 1,000 + "…"; `{{token}}` in a description appears literally and no secret value appears.
- [ ] AC7 A history request copy (serde) keeps both fields; a request JSON without them deserialises with empty values.
- [ ] AC8 No file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `storage-meta` command — AC2, AC3, AC4, AC5
  run: `cargo test -p wisp-storage --locked --test request_meta 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([4-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `report-meta` command — AC6, AC7
  run: `cargo test -p wisp-core --lib --locked description 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (name the AC6/AC7 tests with `description` in their names.)
- `scope` command — AC8
  run: `git diff --exit-code main...HEAD -- crates/core/src/resolve/engine.rs crates/core/src/resolve/mod.rs crates/app crates/cli crates/storage/src/import/postman.rs crates/storage/src/import/write.rs`
- `suite-and-lint` command — AC1, AC8
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Out of scope
- Importers filling `description` (170); CLI `explain`/`inspect`/`snapshot` fields (173); GUI display (175).
- GUI editing of either field (register verdict: display only).
- Using `expected_status` to decide anything (`status_ok`, 161; exit 7, 166).

## Review round 1 — findings

Reviewed HEAD: b18415b80c62a4aa99364bd9082f00da23073873

- [P2] crates/storage/src/request.rs:152-166 (`write_expected_status`) — when the draft's `expected_status` is empty but the file on disk has the key, the save writes `expected_status = []`. The loader rejects an empty array (AC2), so the file the save just wrote cannot be loaded.
  - Fix: when the draft list is empty and the key is present, remove the key instead of writing an empty array. That keeps AC2 and AC3: the draft wins, and there is no valid empty value.
  - Add a storage test that saves an empty draft over a file that has the key, then reloads it.
  - Failing input: a request file gains `expected_status = 200` on disk (for example from a git pull), and the user picks "Keep mine". `save_request_if_unchanged` → `write_request` writes `expected_status = []`, and `load_request` then returns `Invalid { key: "expected_status", ... }`.

## Wave verified — aa27725b1af98829e35ae4bb5cb89af7200216a9

## Outcome

Merged via record/162-request-metadata at aa27725b1af98829e35ae4bb5cb89af7200216a9. Tester PASS (1369 passed, 0 failed, 4 ignored) and reviewer APPROVE. Review round 1 found one P2 (saving an empty `expected_status` draft over a file that had the key wrote `expected_status = []`, which the loader rejects), fixed in aa27725 by removing the key. No follow-ups from code review. The importer literal sites needed no edit because they use `..Request::default()`, so `codec.rs` and `import/*` are unchanged. `cargo fmt` runs as `cargo fmt --all --check`.
