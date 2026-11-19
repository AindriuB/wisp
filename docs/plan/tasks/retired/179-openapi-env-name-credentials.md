# 179 — Keep URL credentials out of OpenAPI environment file names

**Repo:** .
**Round:** 0
**Depends on:** 170
**Owns:**
- crates/storage/src/import/openapi.rs (the server loop's environment-name choice only)
- crates/storage/tests/import.rs (new `h4_server_name_` tests only)

## Goal
PR #130 review item 130-1. When a server URL is flagged by
`may_hold_credentials`, `split_userinfo` leaves it unchanged and `url_host`
returns `user:digits`, so `https://bob:1234?rest@h2/y` with no description
writes `environments/bob-1234.toml`, with the possible password in the file
name. Name such a server's environment `server-N` instead.

Decision: fix it in `openapi.rs`, not in `url_host`. `url_host` is shared with
other importers' warnings (curl's extra-URL warning, 170 AC3), and changing it
would widen the blast radius beyond this finding.

## Context
- crates/storage/src/import/openapi.rs:364-372 the `may_hold_credentials` warning; :385-393 the name choice (`desc_is_safe` → `env_name(desc)`, else `env_name(url_host(url))`, empty → `server-{i+1}`). Line numbers are from `record/170-importer-hardening-descriptions`; re-locate on `main` after 170 merges.
- crates/storage/src/import/mod.rs:298 `url_host`, :484 `may_hold_credentials` (read only).
- crates/storage/tests/import.rs — the existing `h4_` tests (11 on the 170 branch) show the OpenAPI fixture shape.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 179 openapi-env-name-credentials`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-179`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 A server with no description whose URL makes `may_hold_credentials` true gets the environment name `server-N` (N its 1-based index, suffixed as today if taken). Test `h4_server_name_*`: importing a spec with server `https://bob:1234?rest@h2/y` and no description writes no environment file whose name contains `1234` or `bob`, and writes `environments/server-1.toml`.
- [ ] AC2 A server with a safe description still uses the description, and a plain `https://api.example.com:8443/v1` with no description still gets a host-derived name (not `server-1`). Test `h4_server_name_*`.
- [ ] AC3 The `may_hold_credentials` warning and the stored `baseUrl` value are unchanged (the user is still told to check the URL).
- [ ] AC4 `crates/storage/src/import/mod.rs` is unchanged; no file outside Owns changes; no new dependency; fmt, clippy and the workspace suite pass, including every existing `h4_` test.

## Checks
- `server-name` command — AC1, AC2
  run: `cargo test -p wisp-storage --locked --test import h4_server_name_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `h4-all` command — AC4
  run: `cargo test -p wisp-storage --locked h4_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[3-9]|[2-9][0-9]|[1-9][0-9]{2,}) passed; 0 failed; 0 ignored'`
- `scope` command — AC4
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/import/openapi.rs$' -e '^crates/storage/tests/import.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC4
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `warning-unchanged` read — AC3
  how: the reviewer confirms the diff does not touch the warning text or the `Var { name: "baseUrl", value: url }` push.

## Out of scope
- Changing `url_host`, `split_userinfo` or `may_hold_credentials` in `mod.rs`.
- Stripping the possible credential from the stored `baseUrl` (the warning covers it; a separate decision).
- curl and Postman importers.

## Review round 1 — findings
Reviewed head: 546adf6b0da654d80022c25a3cec5b38f4c16782

- [P2] crates/storage/src/import/openapi.rs:364 — The branch sets `suspect` from the URL before `split_userinfo` strips real userinfo. If the URL has real userinfo in front of the suspect `user:digits?…@` shape, the check returns false (the authority segment contains `@`). Stripping then exposes `bob:1234?…@h2`, and the name falls back to `url_host`, which puts the possible password in the file name. This is the leak the task exists to close.
  route: implementer — the fix stays inside the name choice that Owns allows. Recompute the check on the stripped URL for naming, e.g. `suspect || super::may_hold_credentials(&url)` in the `else if` at :389. That needs no change to `mod.rs`, the warning text or `baseUrl`.
  input: OpenAPI server `{"url":"https://u:p@bob:1234?rest@h2/y"}` with no description  →  environment name `bob-1234`, so the file is `environments/bob-1234.toml`. The only warning is "server 1 (`bob:1234`): credentials in the URL were dropped", and there is no "may contain credentials" warning.

## Review round 2 — findings
Reviewed head: 7ad2c30c9e8fc83f086ccfb19267d67b686a5433
Round 1's P2 is FIXED at openapi.rs:389. Orchestrator ruling on the DRIFT below: option (a), move the test; Owns is unchanged.

- [DRIFT] crates/storage/src/import/openapi.rs:1174 — The regression test for the round-1 finding (`env_name_ignores_suspect_host_exposed_after_userinfo_strip`) was added to openapi.rs's `mod tests`. Owns allows openapi.rs changes only to the server loop's environment-name choice, and allows new tests only as `h4_server_name_*` in crates/storage/tests/import.rs. So the test is outside Owns, and it is also not run by the `server-name` check (`--test import h4_server_name_`).
  fix: move it to crates/storage/tests/import.rs as an `h4_server_name_*` test, and remove it from openapi.rs.

## Wave verified — dd241de3df8fe7a8562b386145b808e02c1ab1b3

## Outcome
Merged at dd241de (merge 4716999 on record/179-openapi-env-name). AC1-AC4 passed; tester PASS, reviewer APPROVE after two review rounds. The name choice re-checks the stripped URL, and the regression test lives in `crates/storage/tests/import.rs`. Follow-ups are in PLAN.md under the task 170 follow-ups.

## Rounds 4 and 5
Round 4 made the Windows path assert in `h4_server_name_avoids_possible_credentials` portable (Attempt 3 above).

Round 5, external review of PR #138 at head e93bc5fab82c, finding 138-1 (P2, `openapi.rs:389`): `url_host` runs `split_userinfo` a second time, so a second strip can expose a `user:digits` segment that neither earlier check saw. Failing input (synthetic): `https://u:p@x:y?z@bob:1234?rest@h2/y` with no description gave `bob-1234`. This was over the cap of 4, so nothing was routed. The user decided on one fresh implementer run under systematic-debugging. The decision: with no safe description, any server whose URL had userinfo is named `server-N`; `h4_openapi_env_file_name_never_comes_from_a_secret_description` now expects `server-1`. Round 5 code review: CLEAN. Verified at 8c1f3cbbb46b1960afd6305bcb5de5898c2b91e7.
