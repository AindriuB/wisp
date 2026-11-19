# 173 — wisp-cli snapshot and request metadata in the CLI (2G, C3, D3)

**Repo:** .
**Round:** 0
**Depends on:** 166, 168, 172
**Owns:**
- crates/cli/**

## Goal
`wisp-cli snapshot <collection>` prints one bounded, redacted, deterministic
document of a collection's persisted state for agents and people, and `explain`
and `inspect` show the new `description` and `expected_status`. Nothing is ever
written and no secret is ever printed.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 5" (:344-389) — binding shape, bounds, drop order and final fallback (:370-371), `recent` rules (:372-374); "Feature 4" CLI fields (:323, :332); invariants (:430-437); Waves 2G (:474).
- crates/cli/src/dto.rs:44 `ExplainDoc`, :76 `NodeInfo`, :175 `ProblemDoc`, `CollectionInfo`, `FindingInfo`; cmd/inspect.rs (2,000-node tree cap, history URL display without query), cmd/explain.rs:209-217 `ExplainDoc` literals; args.rs; main.rs.
- `HistoryStore::open_read_only` / `list(limit)` (168); `ResolutionReport.description`, `expected_status` (162); `DataDir::resolve()` (`storage/src/data_dir.rs:52`, creates nothing); `read_collection_id`.
- `scripts/check-boundaries.sh:23-26` deny-list for `crates/cli/src` (unchanged).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 173 cli-snapshot-and-metadata`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-173`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `args.rs` parses `snapshot <collection> [--env <name>] [--request <request-path>] [--recent <n>] [--json]`; `--recent` outside 1-50 and `--keychain` are usage errors (exit 2). Parser table rows added; `USAGE` lists it.
- [ ] AC2 JSON has exactly the top-level keys of spec :355-366 (`request` only with `--request`, `recent` only with `--recent`), built from existing DTOs; `source` is the fixed sentence; no snapshot timestamp. Text output has the `inspect`, `validate` and `explain` sections under headings.
- [ ] AC3 Bounds: tree 2,000; findings 500; recent 50; environments 200; variable names per environment 500; the `request` ExplainDoc 500 variables and 500 headers; names 200 characters; descriptions 1,000. The serialised document is at most 1 MiB, enforced by dropping in the order `recent`, `findings`, tree entries from the end, `request` (as `{"omitted":true}`), each environment's variable list (count kept), each drop named in `truncated`.
- [ ] AC4 Final fallback: still over 1 MiB after every drop → only `schema`, `command`, `wisp_cli`, `source`, collection info, a count per section and `truncated` naming every dropped section; exit 0 with a note on stderr.
- [ ] AC5 `recent` uses `open_read_only` only: no id, no index or no scrub marker → empty with the spec's note; history URLs show origin and path without query; entries carry `delivery` when present.
- [ ] AC6 `explain` gains `description` (capped 1,000 with "…") and `expected_status`; `inspect`'s `NodeInfo` gains `has_description` and `expected_status`. Additive under `wisp-cli/1`; existing golden tests change only by the added fields.
- [ ] AC7 Tests: a synthetic collection exceeding every cap gives ≤ 1 MiB with `truncated` listing the drops; a fixture that stays over 1 MiB gives the fallback form; two snapshots of an unchanged collection are byte-identical; history (a) unmigrated dir → note, and dir listing, sizes, mtimes unchanged, (b) migrated store lists entries, dir unchanged.
- [ ] AC8 Canary and read-only tests from 0.3.0 run against `snapshot` in both formats, with a history fixture whose index holds canary strings in fields the DTO does not map: no canary in stdout or stderr; collection tree and data dir unchanged.
- [ ] AC9 `scripts/check-boundaries.sh` passes unchanged; no file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `cli-snapshot` command — AC1, AC2, AC3, AC4, AC5, AC7, AC8
  run: `cargo test -p wisp-cli --locked snapshot 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `snapshot` in each new test name; at least 10.)
- `cli-meta` command — AC6
  run: `cargo test -p wisp-cli --locked description 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `boundaries` command — AC9
  run: `scripts/check-boundaries.sh && test "$(git diff --name-only main...HEAD | grep -v '^crates/cli/' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC9
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `shape` read — AC2, AC3
  how: the reviewer checks the key set and the drop order against spec :355-371.

## Out of scope
- Live IPC or unsaved drafts; `--keychain` for snapshot (never); any change to `run` beyond shared DTO additions.

## External review — findings
Key: sha256:d7c370a3066f · reviewed head: ec91130154e39a489e53b454dfc242bd4733fa29 (assumed: pasted review named no head; PR head unchanged since CI) · round 1

PR #145: one P2 finding — text snapshots can exceed the 1 MiB limit.
crates/cli/src/cmd/snapshot.rs:54–55
The size check measures JSON, but text rendering expands control and bidi characters. A modeled collection produces 1,045,314 bytes of JSON while its rendered header names and separators alone exceed 2.7 MiB. Measure the selected output format or conservatively bound both formats.
Overall, #145 adds useful snapshot functionality, but its text-output size guarantee needs fixing.

## Outcome
Merged at abb9136 (PASS + APPROVE). `wisp-cli snapshot` and description metadata landed; follow-ups are in PLAN.md. Unblocks 176.

Rounds: 1. Round 1: external P2 "text snapshots can exceed 1 MiB" fixed in be30f70 (`fsize` takes the larger of pretty-JSON and rendered-text size; test `text_rendering_is_bounded_too`); re-verified PASS + APPROVE at be30f70 (1550 tests); CI green; fresh external review clean at be30f70.
