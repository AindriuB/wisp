# 118 — Close the history redaction gaps that store secrets on disk

**Repo:** `.`
**Depends on:** none
**Size impact:** small (expected well under 20 KB on the Windows binary; no new dependency)
**Owns:**
- crates/core/src/redact.rs (including its inline tests)
- crates/storage/src/history.rs (including its inline tests)
- crates/storage/tests/history.rs
- crates/app/src/workspace.rs — `sanitize_redacted` and its tests only. Task 121 concurrently owns `finish_env_load` in the same file; whichever merges second rebases. No other part of workspace.rs may change.

## Goal
History files live in the per-user data dir and must hold no secret values. Five gaps still write one in plain text: credential words missing from the shared list, response headers not checked by name, `Set-Cookie` with no `name=`, path variables, and JSON bodies with a duplicate secret key. Carried from task 99 AC45 (history and core parts), AC46, AC10, AC16, AC47.

## Context
- crates/core/src/redact.rs:5-22: `SECRET_FRAGMENTS` and `is_credential_name`, used by the importers (crates/storage/src/import/mod.rs:370) and history.
- crates/storage/src/history.rs:296-364 `redact_request`. Line 356 matches `SECRET_FRAGMENTS` directly for Text and XML bodies. `out.path` (`Request.path`, crates/core/src/request.rs:313) is never redacted.
- crates/storage/src/history.rs:456-473 `redact_set_cookies` skips a value with no `=`. crates/storage/src/history.rs:553-563 `append` redacts response headers only through it.
- crates/storage/src/history.rs:420-453 `redact_json_body`: a parse into `serde_json::Value` keeps the last of duplicate keys, so `{"token":"SECRETVAL","token":"{{t}}"}` finds nothing to redact and the original text is stored.
- crates/storage/tests/history.rs:319 `raw_files` and :413 `header_name_heuristic_table` are the patterns for "the secret appears in no stored file".

## Acceptance
- [ ] AC1 (99-AC45) `SECRET_FRAGMENTS` also holds `sig`, `pass`, `pwd`, `jwt`, `bearer`, `dpop` and `assertion`. A unit test in redact.rs asserts `is_credential_name` is true for a name containing each of the seven, and the existing cases still hold.
- [ ] AC2 (99-AC45) No crate outside crates/core/src/redact.rs matches on the fragment list itself: the Text/XML body check in history.rs goes through a function exported from `wisp_core::redact`. `rg -n 'SECRET_FRAGMENTS' crates/storage crates/app` prints nothing.
- [ ] AC3 (99-AC46) Every response header whose name passes `is_credential_name` is stored with an empty value, and its name is added to the entry's `redacted` list. `Set-Cookie` keeps its current rule (name kept, value emptied) as amended by AC4. A test stores a response carrying `X-Auth-Token: SECRETVAL` and `X-Subject-Token: SECRETVAL` and asserts `SECRETVAL` appears in no file under the data dir.
- [ ] AC4 (99-AC10) A `Set-Cookie` value with no `=` before its first `;` (or with no `=` at all), such as `abc123token; Path=/`, is stored with the part before the first `;` emptied. A test asserts the token appears in no stored file.
- [ ] AC5 (99-AC16) Path variables (`Request.path`) whose names pass `is_credential_name` are emptied the same way as query params, a `{{ref}}` value is kept, and each is noted as `path.<name>`. A test stores path vars `token=SECRETVAL` and `id=42` and asserts `SECRETVAL` is in no stored file and `42` survives.
- [ ] AC6 (99-AC47) A JSON body holding a duplicate key in any object, at any depth, is dropped whole and `body` is noted (fail closed). A test stores `{"token":"SECRETVAL","token":"{{t}}"}` and asserts `SECRETVAL` appears in no stored file. A body without duplicate keys is redacted exactly as today.
- [ ] AC7 The existing tests in crates/storage/tests/history.rs pass. Where a new fragment changes an expected value, the test is updated to the new expectation, and the PR description names each such test.
- [ ] AC8 (Decisions 2026-11-14) A resend of a history entry whose path var is noted `path.<name>` drops that var in `sanitize_redacted`, so resolution reports it missing (`MissingPathParam`) and nothing reaches the wire. A test in crates/app/src/workspace.rs resends `DELETE {{base}}/sessions/{token}` with `token` noted `path.token` and asserts the missing-variable result and no send.

## Checks
cargo writes no JUnit XML here, so these are command checks.
- `redact-unit` command — AC1
  run: `cargo test -p wisp-core --lib redact`
  pass: exit 0, at least 2 passed.
- `history` command — AC3, AC4, AC5, AC6, AC7
  run: `cargo test -p wisp-storage --test history`
  pass: exit 0, 0 failed, and the passed count is at least the count on main 7ec7505 plus 4 (re-measure the base with the same command first and state both numbers in the PR).
- `no-local-fragments` command — AC2
  run: `! rg -n 'SECRET_FRAGMENTS' crates/storage crates/app`
- `storage-suite` command — AC7 (the importers share the list)
  run: `cargo test -p wisp-storage`
- `app-resend` command — AC8
  run: `cargo test -p wisp-app sanitize_redacted`
  pass: exit 0, 0 failed, at least 1 passed.
- `lint` command — all
  run: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Standing constraints
- Worktree via `./.claude/scripts/wt-new.sh . 118 history-redaction`, `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-118`.
- Secrets never logged or in docs. Tests use the literal marker `SECRETVAL` only.
- Tests never touch the real OS keychain; none is needed here.
- Storage tests use temp dirs (a temp `WISP_DATA_DIR` where a data dir is needed) and clean up.
- Never run `pkill` or `killall`.
- No Windows-only code is expected; the Windows CI job (54-CI) lands later.

## Out of scope
- crates/storage/src/import/**: the importers pick up the new fragments through `is_credential_name` with no edit. If an importer test fails because of the new fragments, stop and report; do not edit import files.
- 99-AC8, AC9, AC11, AC12, AC13, AC14, AC15, AC17, AC48, AC49, AC50 (deferred history items).
- The opt-in setting for unredacted `Set-Cookie`.

## Decisions
- 2026-11-14 (user decision, code review round 1 of PR #62): on resend from history, a path variable that history stored redacted (noted as `path.<name>`) is dropped, so resolution reports it missing (`MissingPathParam`) and the request is not sent; the user re-enters the value. Fail closed. This adds crates/app/src/workspace.rs (`sanitize_redacted` and its tests only) to Owns, and AC8.

## Review round 1 — findings (HEAD ea2203d)
- [P2] crates/storage/src/history.rs:330-334 — path vars are stored empty and noted `path.<name>`, but the app's resend sanitiser (`sanitize_redacted`, crates/app/src/workspace.rs:693-774) has no `path.` case, so a resend sends an empty path segment (`DELETE {{base}}/sessions/` instead of holding back). Settled by the Decisions entry above.
- [NOTE, follow-up] crates/core/src/redact.rs:16-22 — the new fragments are substring matches: importers now move headers like X-Bypass-Cache / X-Assignment-Id into the placeholder secrets env, and Text/XML history bodies containing words like "design", "signal", "compass" are dropped whole.
- [NOTE, follow-up] crates/storage/src/history.rs:534-545 — response header names share the unprefixed `redacted` namespace with request headers; a same-named request header with an intentionally empty value is disabled on resend, and WWW-Authenticate etc. appear in every resend notice.
