# 181 — Keep a possibly secret host out of the OpenAPI "credentials dropped" warning

**Repo:** .
**Round:** 0
**Depends on:** 179
**Owns:**
- crates/storage/src/import/openapi.rs (the server loop's two credential warnings only)
- crates/storage/tests/import.rs (new `h4_dropped_creds_` tests only)

## Goal
The OpenAPI importer's "credentials in the URL were dropped" warning
interpolates `url_host` of the stripped URL. For
`https://u:p@bob:1234?rest@h2/y` the strip leaves `https://bob:1234?rest@h2/y`,
whose `url_host` is `bob:1234`, so the warning echoes a possible second
user:password. Say `server N` without the host when the stripped URL is still
flagged by `may_hold_credentials`, and make the "may contain credentials"
warning fire for that input.

Decision (warning text): this supersedes 179's AC3 freeze for the
"credentials dropped" warning only. A stripped URL that `may_hold_credentials`
flags gets `server {N}: credentials in the URL were dropped`; any other keeps
today's `server {N} (`{host}`): ...` text.

Decision (suspect check): today `suspect` is computed on the URL before the
strip. `may_hold_credentials` is false for `https://u:p@bob:1234?rest@h2/y`,
because its first segment contains `@`, but true for the stripped URL. So the
user is not told to check a URL that still holds a possible secret. Compute
`suspect` as "original or stripped URL is flagged" and emit the
"may contain credentials" warning once, unchanged in wording. 179's
environment-name choice already uses `suspect || may_hold_credentials(&url)`,
so it keeps working.

## Context
- crates/storage/src/import/openapi.rs:364-385 on `record/179-openapi-env-name` (the `suspect` warning, the `split_userinfo` strip, the dropped warning, the name choice); re-locate on `main` after 179 merges.
- crates/storage/src/import/mod.rs:484 `may_hold_credentials`, `split_userinfo`, `url_host` (read only).
- crates/storage/tests/import.rs — 179's `h4_server_name_` tests show the fixture shape. Compare paths with `Path::ends_with` on components, not a `/` string (179 attempt 3 failed on Windows that way).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 181 openapi-dropped-credentials-warning`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-181`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 Importing a spec with server `https://u:p@bob:1234?rest@h2/y` yields no warning containing `bob` or `1234`, yields exactly one warning `server 1: credentials in the URL were dropped`, and yields exactly one `server 1: the URL may contain credentials; check it before committing`. Test `h4_dropped_creds_*`.
- [ ] AC2 A plain `https://alice:pw@api.example.com/v1` still warns `server 1 (`api.example.com`): credentials in the URL were dropped` and does not warn "may contain credentials". Test `h4_dropped_creds_*`.
- [ ] AC3 `https://bob:1234?rest@h2/y` (flagged before any strip) still gets exactly one "may contain credentials" warning and no "dropped" warning. Test `h4_dropped_creds_*`.
- [ ] AC4 The stored `baseUrl` value and the environment-name choice are unchanged from 179; every existing `h4_` test passes unmodified.
- [ ] AC5 `crates/storage/src/import/mod.rs` is unchanged; no file outside Owns changes; no new dependency; fmt, clippy and the workspace suite pass.

## Checks
- `dropped-creds` command — AC1, AC2, AC3
  run: `cargo test -p wisp-storage --locked --test import h4_dropped_creds_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `h4-all` command — AC4
  run: `cargo test -p wisp-storage --locked h4_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[6-9]|[2-9][0-9]|[1-9][0-9]{2,}) passed; 0 failed; 0 ignored'`
- `scope` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/import/openapi.rs$' -e '^crates/storage/tests/import.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `name-and-value` read — AC4
  how: the reviewer confirms the diff leaves the `Var { name: "baseUrl", value: url }` push and the `desc_is_safe` / `server-N` name choice untouched.

## Out of scope
- `url_host`, `split_userinfo`, `may_hold_credentials` in `mod.rs`.
- The same pattern in curl's or Postman's warnings (`postman.rs:186-189` is a separate PLAN item).
- Stripping the possible credential from the stored `baseUrl`.

## Outcome
Merged at c6f3886 (merge 2c4b792 on record/181-openapi-dropped-credentials-warning). AC1-AC5 passed; tester PASS (1535 tests; dropped-creds 3/3; h4 18/18; fmt and clippy clean), reviewer APPROVE. Follow-up in PLAN.md under the task 170 follow-ups: `https://u:p@x:pw/?a@h2/y` still keeps a possible second credential in `baseUrl` with no warning.
