# 103 — OAuth2 in the engine: client credentials, auth code with PKCE, token cache

**Repo:** `.`
**Depends on:** 100
**Size impact:** UNCERTAIN — spec estimate +15-30 KB (Windows). Hand-rolled on reqwest, `serde_json`, tokio `net`, `sha2`, `base64` and `getrandom`, all added or confirmed by task 100. No new dependency, and no HTTP server crate: the loopback listener is a tokio `TcpListener` with a hand-written request-line parser.
**Order:** runs before 104. Both need `engine.rs`; 104 adds the `EngineConfig.cookies` seam after this merges, and edits `client.rs`/`cookies.rs`, which this task does not touch.
**Owns:**
- crates/core/src/auth/oauth2.rs, crates/core/src/auth/oauth2/** (new submodules, e.g. `pkce.rs`, `loopback.rs`, `token.rs`)
- crates/core/src/engine.rs
- crates/core/tests/oauth2.rs (new)
- crates/core/src/request.rs (only `Auth::redacted`'s two OAuth2 arms and their unit tests; AC17)
- crates/storage/tests/history.rs (only the OAuth2 redaction test(s); AC17)

## Goal
The engine performs OAuth2 for the two auth types task 100 froze. Client credentials fetch a token on send. Auth code + PKCE runs on `Command::AuthorizeStart`: it binds a 127.0.0.1 listener, emits `AuthUrl`, validates `state`, exchanges the code and caches the tokens. Access tokens live in memory only. Refresh tokens are memory only unless `persist_refresh` is set, in which case they go through the `RefreshTokenStore` seam (wave 3 supplies the keychain). Tokens follow task 95's redirect rule and never reach another origin.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#3-auth-core (OAuth2 bullets, Failures), #timing-and-other-limits (120 s)
- Task 100 brief, Decisions — the frozen types; do not change their shape. A needed change means stop and report. Two changes are ordered by the orchestrator (PR #42 review): `RefreshTokenStore`'s methods return `Result` (AC16), and `Auth::redacted` keeps non-secret OAuth2 fields (AC17).
- crates/core/src/auth/oauth2.rs:17-35 — `TokenKey` now carries `collection` and `environment` (PR #42). Fill them from `ClientSettings.collection` and `PreparedRequest.environment` / `AuthorizeStart.environment` (crates/core/src/model.rs:59-61, :172-179).
- crates/core/src/request.rs:147-175 `Auth::redacted` OAuth2 arms; crates/storage/src/history.rs:245-299 `auth_fields`/`redact_request` (unchanged: it derives the redacted names from `redacted()`); crates/storage/tests/history.rs:656 the existing OAuth2 redaction test.
- crates/core/src/engine.rs:670-900 — the own-redirect path. `own_redirects` is true whenever `req.auth.is_some()`, and auth is dropped after the first cross-origin hop. OAuth2 must ride this path.
- crates/core/src/client.rs (from 100) — get token-endpoint clients through `ClientCache::get` with `follow_redirects: false` and the request's `collection`. Do not edit `client.rs` or `cookies.rs`, and do not add an `EngineConfig` cookie field: task 104 owns those and runs after this task.
- RFC 6749 §4.1, §4.4, §5.1-5.2; RFC 7636 Appendix B (the PKCE vector); RFC 8252 §7.3 (loopback redirect)
- docs/conventions.md#logging, #tests (wiremock, no public internet, no sleeps waiting for state)
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-103`. No app launch.

## Acceptance
**Token endpoint (both grants)**
- [ ] AC1 Requests go only to an `https` `token_url`, or `http` when the host is `127.0.0.1`, `::1` or `localhost`. Anything else fails with `ErrorKind::Auth` before any I/O. Token requests never follow redirects.
- [ ] AC2 With `client_auth = Basic`, the client id and secret go in an `Authorization: Basic` header; with `Body`, they go as form fields. A public client (empty secret) sends `client_id` in the body and no secret. Tested with wiremock matchers.
- [ ] AC3 The response is parsed with a 1 MiB cap. A `token_type` other than `bearer` (any case) is an Auth error. A non-2xx response or bad JSON gives an Auth error whose message holds the status and at most 2 KiB of the body, with the values of `access_token`, `refresh_token` and `id_token` replaced by `<redacted>`. Tested with a body containing `SECRETVAL` in each.

**Sending with OAuth2 auth**
- [ ] AC4 Client credentials: the first send fetches a token and sends `Authorization: Bearer <token>`, replacing any user-set `Authorization`. A second send within the expiry reuses it (wiremock `expect(1)` on the token endpoint). It refetches once `expires_in` minus 30 s has passed; the test uses an injectable clock, not a sleep. The cache key is `TokenKey`.
- [ ] AC5 A 401 from the resource drops that key's cached access token, so the next send refetches. There is no automatic retry.
- [ ] AC6 Auth code with no cached token and no refresh token fails with `ErrorKind::Auth`, "Not authorized: use Authorize first". If the access token has expired and a refresh token is held, the engine refreshes with `grant_type=refresh_token`. A rotated refresh token replaces the old one.
- [ ] AC7 Redirects: a send redirected from origin A to origin B carries the bearer token to A only. A same-origin hop keeps it. Tested with two wiremock servers.

**Authorize flow**
- [ ] AC8 `AuthorizeStart` checks `auth_url` as AC1 checks `token_url`. It binds `127.0.0.1:<redirect_port>` (0 = ephemeral) and emits `AuthUrl`. The URL keeps `auth_url`'s existing query and adds `response_type=code`, `client_id`, `redirect_uri=http://127.0.0.1:<port>/callback`, `scope` (if set), `state` (≥128 random bits) and `code_challenge` with `code_challenge_method=S256`.
- [ ] AC9 The PKCE challenge of the RFC 7636 Appendix B verifier equals the RFC's challenge (unit test). Generated verifiers are 43-128 characters from the unreserved set.
- [ ] AC10 The listener reads at most 8 KiB of request head, with a 5 s read timeout per connection. A request other than `GET /callback` gets 404 and listening continues. The reply page is static and echoes no query value.
- [ ] AC11 A callback with a matching `state` exchanges the code (with `code_verifier` and the same `redirect_uri`), caches the tokens and emits `AuthCompleted`; a following send uses the token. A mismatched `state` or an `error=` parameter emits `AuthFailed` and exchanges nothing. Tests drive the callback with a reqwest GET to the `AuthUrl`'s redirect URI.
- [ ] AC12 The flow ends with `AuthFailed` on a timeout (`timeout` from the command, default 120 s; the test passes a short one), on `AuthorizeCancel(id)` (message "cancelled") or when the port is in use. The listener is closed in every case. After the end, a test asserts that a connection to the port is refused.
- [ ] AC13 Refresh tokens go to `RefreshTokenStore::save` only when `persist_refresh` is true. A recording store in the test sees no call when it is false, and one call with the right `TokenKey` when it is true. Access tokens are never passed to the store.
- [ ] AC14 No `tracing` call, error message, event or Debug output contains an access token, refresh token, code, verifier, state or client secret. Every internal type that holds one has a hand-written Debug and a `debug_redacts_*` test.

**Partition, store errors, history (PR #42 follow-ups)**
- [ ] AC15 Every `TokenKey` the engine builds sets `collection` from the request's `ClientSettings.collection` and `environment` from `PreparedRequest.environment` (send) or `AuthorizeStart.environment` (authorize). Tested: two client-credentials sends that differ only in `environment` fetch twice (wiremock `expect(2)`), and the same holds for two `collection` values.
- [ ] AC16 (42-3) `RefreshTokenStore` methods return `Result`: `load -> Result<Option<String>, RefreshStoreError>`, `save`/`forget -> Result<(), RefreshStoreError>`. `RefreshStoreError` is a public message type whose Display and Debug never hold a token. `MemoryOnly` never fails. The trait doc states that implementations may block (a keychain prompt) and that the engine must call a blocking store through `tokio::task::spawn_blocking`; wiring that is left to wave 3. A `load` error fails the send with `ErrorKind::Auth` "cannot read stored refresh token: <error>". A `save` error keeps the tokens in memory, still emits `AuthCompleted`, and logs a warning with the error and no token. Both are tested with a failing store.
- [ ] AC17 (42-7) `Auth::redacted` for both OAuth2 types empties only `client_secret` (a single `{{var}}` reference is kept, as now). `token_url`, `auth_url`, `client_id`, `scope`, `client_auth` and `redirect_port` are kept verbatim, so a history entry's `redacted` list names at most `auth.client_secret`. The core unit test is updated, and a storage history test asserts that a stored entry keeps a literal `token_url` and `client_id` and holds no `SECRETVAL` from the secret.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `oauth2` command — AC1-AC16
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-103; test "$(cargo test -p wisp-core --locked --test oauth2 -- --list 2>/dev/null | rg -c ': test$')" -ge 19 && cargo test -p wisp-core --locked`
- `history-redaction` command — AC17
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-103; test "$(cargo test -p wisp-storage --locked --test history oauth2 -- --list 2>/dev/null | rg -c ': test$')" -ge 1 && cargo test -p wisp-storage --locked`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-103; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock Cargo.toml crates/core/Cargo.toml`
- `size` command — size
  run: `gh workflow run measure.yml --ref task/103-oauth2-engine`; when it finishes, `gh run download <run-id> -n measure-x86_64-pc-windows-msvc -D <scratch> && jq -e '.binary_bytes <= 15500000' <scratch>/measure-x86_64-pc-windows-msvc.json`. Report `binary_bytes` and its delta from 14,514,688 in the PR.
- `secrets` read — AC10, AC14
  how: no log, message or page echoes a secret or a query value; the listener binds 127.0.0.1 only.

## Out of scope
- The app: Authorize button, browser opening, status UI (task 106).
- Keychain storage of refresh tokens (wave 3). Only the trait call sites are built here.
- Implicit, password and device grants; OpenID Connect `id_token` validation; token revocation.
- Cookies on token-endpoint requests (task 104 decides client-wide behaviour).
- `client.rs`, `timing.rs`, `cookies.rs`, and any `EngineConfig` cookie field (tasks 104, 105).
- Calling the store through `spawn_blocking` (wave 3, with the keychain store); only the doc is written here.
- The app's Resend resetting auth to Inherit when `auth.client_secret` was redacted (`crates/app/src/workspace.rs:188`, a PLAN.md task-97 follow-up), and any other change in `crates/storage/src/history.rs`.

## Amendment 1
Orchestrator decision, 2026-11-10. No other in-flight task owns these files.

**Owns** also includes:
- crates/core/tests/engine.rs (only: remove the three tests that assert the old OAuth2 stub, `oauth2_engine_auth`, `send_with_oauth2_auth_fails_with_the_auth_kind` and `authorize_start_is_answered_with_auth_failed`; and add `.unwrap()` to `refresh_tokens.save(...)` in `debug_redacts_engine_config`)
- crates/core/Cargo.toml (only: move `serde_json` from dev-dependencies to dependencies, and add tokio's `net` feature explicitly)

Replace the hand-written JSON reader (`auth/oauth2/json.rs`) with `serde_json` parsing of token responses. `serde_json` is already in Cargo.lock via storage, so Cargo.lock gains no new `[[package]]`.

- [ ] AC18 Token responses are parsed with `serde_json`; `auth/oauth2/json.rs` is gone. The body read is capped (AC3's 1 MiB); a malformed or oversized response gives an `ErrorKind::Auth` error that does not echo the body. Tested with a malformed body and an oversized body, each containing `SECRETVAL`, asserting the error holds neither.

Checks: `oauth2` now covers AC18. `no-deps` is replaced by:
- `no-deps` command — size, AC18
  run: `git diff --exit-code main...HEAD -- Cargo.toml && ! git diff main...HEAD -- Cargo.lock | rg -q '^\+\[\[package\]\]'`

## Amendment 2
User decision, 2026-11-10.

Every successful Authorize replaces the stored refresh token for its `TokenKey`. When the token response has no `refresh_token`: the in-memory refresh token for that key is cleared, and if `persist_refresh` is true the engine calls `RefreshTokenStore::forget` for that key. A `forget` error is handled like a `save` error (AC16): tokens stay in memory, `AuthCompleted` is still emitted, and a warning is logged with the error and no token.

- [ ] AC19 Authorize as X (the response carries `RT_X`, which is saved), then authorize again for the same `TokenKey` as Y with a response that has no `refresh_token`. After `AT_Y` expires (injectable clock), a send fails with `ErrorKind::Auth` "Not authorized: use Authorize first" rather than refreshing, and the token endpoint never receives `grant_type=refresh_token` with `RT_X` (wiremock matcher, `expect(0)`). With `persist_refresh` true the recording store sees one `forget` for that key; a failing `forget` still emits `AuthCompleted`.

Checks: `oauth2` now covers AC19.

## Review round 1 — findings
Reviewed HEAD bbf5fa6.
- [P2] `manager.rs:184-205` re-authorize leaves the stored refresh token — resolved by Amendment 2 (AC19).
- [P2] `tests/oauth2.rs:725-731` the 8 KiB head cap test (AC10) passes with the cap removed: the 5 s read timeout yields the empty reply anyway, and the read timeout is not tested alone. Fix: the oversized-head connection sends 9000+ bytes and asserts the server closes well before 5 s (`tokio::time::timeout` of about 1 s, server-side close as the signal). Add a separate test that a silent connection is dropped by the read timeout — without a long sleep if a clock or config seam allows, otherwise one ~5 s test is accepted.
- [STYLE] `token.rs:351-356` `Secret`'s Debug is checked only with `{:?}`; also check `{:#?}` (`docs/conventions.md:66-68`).
- [P3, optional] `manager.rs:228-262` no single-flight on refresh; with rotating refresh tokens, concurrent refreshes may get the token revoked.
- [P3, optional] `engine.rs:720-731` the token fetch uses the full request timeout before `began`, so a send can take about twice the timeout.
