# 182 — Stop the history scrub emptying truncated JSON request bodies

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/storage/src/history.rs (`append`'s folder creation, `scrub_legacy`'s request-body pass, and their unit tests)
- crates/storage/tests/history.rs (new `scrub_truncated_` tests only)

## Goal
Needed before the 0.4.0 release. A history folder created by `append` gets no
`.scrubbed-1` marker, so the next `HistoryStore::open` runs `scrub_legacy`.
That re-runs `redact_request`, and `redact_json_body` empties any JSON body cut
at `BODY_CAP` (the cut text is not valid JSON), losing the stored 1 MiB prefix.
The CLI's `ReadOnlyHistory::list` also reports "not yet migrated" for every
such folder. Write the marker when `append` creates the folder, and make the
scrub leave bodies already flagged truncated alone.

## Context
- crates/storage/src/history.rs:1047-1057 `open` (skips the scrub only when the folder is absent or holds the marker); :1147 the only marker write, at the end of `scrub_legacy`; :1112-1130 the `.req.json` pass calling `redact_request`.
- :1219-1234 `append`: `ensure_private_dir(&self.bodies())`, `redact_request`, then `cap_request_body` (redaction runs before the cut, so a truncated body is already redacted); :405 `ReqFile.truncated`.
- :754 `redact_json_body`; :1007 `ReadOnlyHistory::list`'s marker check; :25 `BODY_CAP`.
- 168 (retired brief `docs/plan/tasks/retired/168-history-hardening.md`) added truncation; 178 changes service/app only, not storage.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 182 history-scrub-truncated-body`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-182`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 When `append` finds the history folder absent and creates it, it writes the `.scrubbed-1` marker (via `atomic_write_private`, under the append lock) before returning. A folder that already existed without the marker gets none from `append`; `open` still scrubs it.
- [ ] AC2 `scrub_legacy` does not change the request body of a `.req.json` whose `truncated` is true; it still redacts that file's URL, headers, auth and response headers as today.
- [ ] AC3 Test `scrub_truncated_*`: on a fresh data dir, append an entry whose JSON request body is over 1 MiB (for example `{"a":"xxxx…"}` of 1.5 MiB); `ReadOnlyHistory::list` on that collection returns the entry with no note; then `HistoryStore::open` again and read the stored entry: `request_truncated` is true and the body is exactly the 1 MiB prefix stored at append (byte-equal, non-empty).
- [ ] AC4 Test `scrub_truncated_*`: a folder with no marker holding a `.req.json` with `truncated: true` and a cut JSON body keeps that body byte-for-byte after `open`, and the marker exists afterwards.
- [ ] AC5 Every existing history test passes unmodified (legacy re-redaction of untruncated bodies still works); no file outside Owns changes; no new dependency; fmt, clippy and the workspace suite pass.

## Checks
- `scrub-truncated` command — AC3, AC4
  run: `cargo test -p wisp-storage --locked --test history scrub_truncated_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `history-suite` command — AC5
  run: `cargo test -p wisp-storage --locked --test history 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (8[9]|9[0-9]|[1-9][0-9]{2,}) passed; 0 failed; 0 ignored'`
- `scope` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/history.rs$' -e '^crates/storage/tests/history.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `marker-and-skip` read — AC1, AC2
  how: the reviewer checks the marker is written only when `append` created the folder (not for an existing unmarked one), and the scrub skips only the body of a truncated file, not its other redaction.

## Out of scope
- 178's resend refusal (service `plan.rs`, app `workspace.rs`, `ui/history.rs`).
- Making `redact_json_body` parse partial JSON, or changing `BODY_CAP`.
- Bumping the marker to `.scrubbed-2` or re-scrubbing marked folders.
- `ReadOnlyHistory::list`'s note text.

## Amendment — 2026-11-18 (user decision)
Owns gains one edit: in crates/storage/tests/history.rs `open_scrubs_old_entries_once` (~:1111), delete the `.scrubbed-1` marker right after the `append` that builds the legacy folder, so the test still models a pre-marker folder. Nothing else in that test changes. AC5 now reads: `open_scrubs_old_entries_once` passes with only that one-line edit.

## Review round 1 — findings
Reviewed head: a2e84bc0b3ebb5b4058f08eccf939c414bb7eea9

- [P2] crates/storage/tests/history.rs:2155 (`scrub_truncated_file_in_unmarked_folder_keeps_body`), also :2124 and :2141 (`scrub_truncated_append_keeps_json_body_across_reopen`) — both tests build the body as `Body::Text`, but only `Body::Json` goes through `redact_json_body` (crates/storage/src/history.rs:603). A Text body with no credential fragment is never emptied by the scrub, so the AC4 test does not guard the scrub skip at all. AC3 and AC4 both ask for a JSON request body.
  route: implementer — change both tests to `Body::Json(TextBody{..})`, and the match arm at :2141 to `Body::Json(b)`. This brings the tests in line with AC3 and AC4 as written.
  input: remove the `if let Some(body) = kept { file.request.body = body; }` restore at history.rs:1132-1134, then run `scrub_truncated_`  →  both tests still pass.

## Wave verified — 6c29da73a6d46b4877da6849b1f60b1a6ec3fb1d

## Outcome
Merged 2026-11-18 at 6c29da73a6d46b4877da6849b1f60b1a6ec3fb1d (tester PASS, reviewer APPROVE). `append` now writes `.scrubbed-1` when it creates the history folder, and `scrub_legacy` leaves the body of a `.req.json` flagged `truncated` alone while still redacting its other fields. `open_scrubs_old_entries_once` deletes the marker after its `append` so it still models a pre-marker folder. Follow-ups are in `PLAN.md`.
