# 195 — Harden the OpenAPI importer, the import writer and URL credentials (H4b)

**Repo:** .
**Round:** 0
**Depends on:** 189, 194
**Owns:**
- crates/storage/src/import/openapi.rs
- crates/storage/src/import/write.rs
- crates/storage/src/import/mod.rs (the URL helpers: `url_host`, `split_userinfo`, `may_hold_credentials` and their callers)
- crates/storage/tests/import.rs
- crates/storage/src/ops.rs (amended 2026-11-20: `unique_stem` and the import-path entry points to `create_request`/`create_folder` only; see Amendment)

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
- crates/storage/src/import/write.rs: :194 `unique` (AC34), :133 `check_folder_rel`. AC35's `unique_stem` is no longer in `write.rs`; `crates/storage/src/ops.rs:254` has one. Find the import path's stem picker. The import path does call it (see Amendment 2026-11-20); fix it there.
- crates/storage/src/import/mod.rs: `url_host` :298, `split_userinfo` :438.
- PLAN.md "Follow-ups from task 170" (:103-114).
- Worktree: `./.claude/scripts/wt-new.sh . 195 importer-hardening-h4b`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-195`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (99-AC21) OpenAPI example text is charged to the budget while it is serialised, through a size-limited writer. The pretty string is never built past the budget. Test `h4b_deep_example_too_large` returns `TooLarge`.
- [ ] AC2 (99-AC34) `unique` uses a set plus a per-base counter, so n equal names cost O(n). Test `h4b_unique_many_equal_names`: 5,000 equal names give 5,000 distinct names. No timing assertion.
- [ ] AC3 (99-AC35, restated 2026-11-20) The import path's stem picking is linear in the number of colliding items. Test `h4b_import_many_colliding_slugs` (see Amendment). No timing assertion.
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
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/import/openapi.rs$' -e '^crates/storage/src/import/write.rs$' -e '^crates/storage/src/import/mod.rs$' -e '^crates/storage/tests/import.rs$' -e '^crates/storage/src/ops.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `budget-and-linear` read — AC1, AC2, AC3, AC7
  how: the reviewer checks that the writer stops at the budget before allocating more, that both pickers are linear, and that the AC7 change is behaviour-neutral.

## Out of scope
- `postman.rs` and `SecretSink`/`RequestCounter` (task 194).
- The curl `--frobnicate SECRETVAL` note and other NOTEs left in PLAN.
- `ops.rs` beyond `unique_stem` and what the import path needs from `create_request`/`create_folder` (see Amendment). Renames, moves, deletes, `create_collection`, `create_environment` and `slug` stay untouched.

## Amendment 2026-11-20 — own `ops.rs` for AC3

**Decided by:** Andrew, 2026-11-20.
**Reason:** the implementer found the import path does reach a quadratic
stem picker. `write_items` calls `create_folder` / `create_request`
(crates/storage/src/import/write.rs:217, :235), which call `unique_stem`
(crates/storage/src/ops.rs:254). `unique_stem` probes the filesystem from
suffix 1 on every call, so k items whose distinct names share a slug
(`"a/b"`, `"a b"`, ...) cost O(k^2) stats. Equal names are already handled
by the new `unique` in write.rs (AC2); this is the different-name,
same-slug case.

**Owns change:** `crates/storage/src/ops.rs` is added, scoped to
`unique_stem` and the callers the import path needs (`create_request`,
`create_folder`, or crate-private variants of them). Nothing else in the
file changes.

**Shape (the implementer picks the details):** let the import carry a
per-(directory, slug) "next suffix to try" hint across calls, e.g. a
`pub(crate)` variant of `create_request`/`create_folder` that takes and
updates the hint, with the public functions delegating with a fresh hint.
`unique_stem` starts probing at the hint instead of 1 and returns the
next suffix along with the stem.

**Non-import behaviour stays unchanged:**
- `create_request` and `create_folder` keep their public signatures.
  Called without a hint they choose the same name for the same inputs and
  the same directory state as today (first free of `<slug>`, `<slug>-2`,
  ...; reserved-name rules unchanged).
- Within one import, the hinted picker chooses the same names the old
  picker would, as long as nothing else removes an entry from the target
  directory during the import: every suffix below the hint was seen taken
  by this import, and the import never deletes. If another process frees
  a lower suffix mid-import, the import may pick a higher suffix than the
  old picker would. That is accepted; it is the only divergence.
- Existing `ops.rs` unit tests (e.g. `create_request_and_suffixes`,
  `create_folder_nested`) pass unmodified.

**AC3 restated:** the import path's stem picking is linear in the number
of colliding items. Test `h4b_import_many_colliding_slugs` in
`tests/import.rs`: import at least 2,000 items with distinct names that
all slug to the same stem (both requests and folders, or one test each),
and assert every item lands under a distinct stem and the run finishes
(no timing assertion). If the implementer adds a probe counter instead
(e.g. a `#[cfg(test)]` counter in `ops.rs`), assert the probe count is at
most a small constant times k; either form satisfies AC3.

**Replaces:** the old Context line "If the fix needs `ops.rs`, stop and
report" and the Out-of-scope line "`ops.rs` (stop and report if needed)".
The `scope` check now also allows `crates/storage/src/ops.rs`.

**No overlap:** no other open task owns `ops.rs`. Task 196 owns
`crates/cli/**`, `crates/service/src/run.rs` and
`scripts/check-boundaries.sh`; 197 owns README and docs files; 198 owns
`scripts/measure/results/v0.5.0-wave-3-size.json`. Only retired task 134
ever owned `ops.rs`.

## Review round 1 — findings

Reviewed HEAD: 66c308e8047c30455663c10d66c36d1b7c6b079e. Verdict FIX. Fix these in the same round as the amended AC3 (see "Amendment 2026-11-20").

- [P2] crates/storage/tests/import.rs:1100 — `h4b_deep_example_too_large` still passes when the size-limited writer is removed. The old code built the whole pretty string and then charged it with `spend`, which also returned `TooLarge`. So the test does not check that AC1's writer stops before the string is built.
  route: implementer — make the test check that the error came from serialising, e.g. `err.to_string().contains("serialising")`. The new writer's message is "work over budget while serialising an example" (openapi.rs:96). The old path's message from `spend` is "...while resolving $refs and parameters" (openapi.rs:63), so this check would fail if the writer were removed.
  input: the branch's deep example (50 nested arrays, 2.5M elements) run against the old `to_string_pretty` + `spend(content.len())` code → still `TooLarge`, so the test is green.
- [P3, optional] crates/storage/src/import/mod.rs:451-462 — `split_userinfo` strips at most 9 credentials (the first plus 8 more passes). It stops without a warning, so a 10th chained credential is kept in the URL, returned as the host by `url_host`, and not flagged by `may_hold_credentials`. The doc comment says leftover credentials are removed.
  route: implementer — loop until `split_userinfo_once` finds nothing (every pass shortens the string and URLs are capped by MAX_URL_BYTES, so it terminates), or flag the URL instead of returning it silently if a cap is hit.
  input: `https://u:p@` followed by `u0:pw0/?a@u1:pw1/?a@…u9:pw9/?a@h/y` → `split_userinfo` returns `https://u8:pw8/?a@u9:pw9/?a@h/y`, `url_host` returns `u8:pw8`, `may_hold_credentials` returns false; `pw8` ends up in the stored base URL and in the server warning.

Note (orchestrator, 2026-11-20): this task file changes on the branch only through the planner's amendment and the /review loop's records. When running the `scope` check, also exclude `^docs/plan/tasks/195-importer-hardening-h4b.md$`.

## Wave verified — 19bb45a (2026-11-20)

## Outcome
Landed on `task/195-importer-hardening-h4b` (commits 66c308e, 19bb45a on top of eeae6f4). Verified on the Linux host: tester PASS and reviewer APPROVE after one review round; the workspace-wide test and clippy runs could not run there (`wisp-app` needs system fontconfig), so CI carries them. OpenAPI example text is charged through a size-limited writer while it is serialised, `unique` is a set plus a per-base counter, the import path carries a per-(directory, slug) suffix hint into `ops.rs` (`create_request_hinted`, `create_folder_hinted`), `url_host` never returns a userinfo-shaped host, `split_userinfo` strips every chained credential, a missing import folder has its own error, and the redundant `suspect ||` is gone. Nine `h4b_` tests were added. AC3 was amended by Andrew (2026-11-20) to give the task `ops.rs`. Open follow-ups are in PLAN.md under "Follow-ups from task 195".
