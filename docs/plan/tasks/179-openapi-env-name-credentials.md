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
