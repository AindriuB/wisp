# 100 — Freeze the 0.2.0 wave-2 types and seams (OAuth2, timings, cookie jar)

**Repo:** `.`
**Depends on:** none (main at 71f9e1a; wave-1 measure task 98 is independent)
**Size impact:** low, but measured later. Adds `serde_json` (already in the binary via storage), the `getrandom` version already in core's tree, and tokio `net` (already compiled in through reqwest) to `wisp-core`. No new `Cargo.lock` package is allowed.
**Owns:**
- Cargo.toml, crates/core/Cargo.toml, Cargo.lock, THIRD-PARTY-NOTICES (dependency lines above only)
- crates/core/src/lib.rs, crates/core/src/model.rs, crates/core/src/request.rs, crates/core/src/auth/mod.rs
- crates/core/src/auth/oauth2.rs (new, stub), crates/core/src/client.rs (new, moved code), crates/core/src/cookies.rs (new, stub), crates/core/src/timing.rs (new, stub)
- crates/core/src/engine.rs, crates/core/src/resolve/engine.rs, crates/core/src/resolve/tests.rs
- crates/core/tests/engine.rs, crates/core/tests/auth.rs, crates/core/tests/tls.rs (new match arms and the AC7 test only)
- crates/storage/src/codec.rs, crates/storage/src/history.rs, crates/storage/tests/history.rs
- crates/app/src/ui/request/auth.rs (match arms and a read-only display only), crates/app/src/workspace.rs (event arms and timing pass-through only), crates/app/src/tab.rs, crates/app/src/ui/history.rs, crates/app/src/ui/response/mod.rs (one `ErrorKind` arm only)

## Goal
Land every cross-crate type change wave 2 needs in one compiling commit, so tasks 102-107 can work in parallel on disjoint files. OAuth2 auth types go fully through the model, `resolve`, the TOML codec and history redaction; the engine refuses them with a clear error until task 103. Phase timings get their type, a home on `Event::Completed`, the tab state and the history entry; values stay `None` until task 105. The cookie jar gets its handle type and `EngineConfig` field, unused until task 104. This is a freeze like tasks 21 and 91: one big mechanical change because a new enum variant breaks every exhaustive match at once.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#3-auth-core, #6-phase-timings-core, #7-cookie-jar-core
- crates/core/src/request.rs:35-66 `Auth`, :80-130 `redacted()`, :132- hand-written Debug; crates/core/src/auth/mod.rs:13-50 `EngineAuth` and its Debug; crates/core/src/model.rs:153-230 `Command`, `ErrorKind`, `Event`
- crates/core/src/resolve/engine.rs:465-520 builds `EngineAuth` for Digest/SigV4 — follow it
- crates/core/src/engine.rs:52-110 `ClientCache` (moves to `client.rs` unchanged); :26-44 `EngineConfig`; :160-245 `submit`
- crates/storage/src/codec.rs auth arms (search `Digest`); crates/storage/src/history.rs:232-262 `keep_var_ref`, `auth_fields`; :163-205 `HistoryEntry`, `NewEntry`
- crates/app/src/workspace.rs:1037-1180 `on_event`, `append_history`; crates/app/src/ui/history.rs:110-120; crates/app/src/tab.rs:54-63 `ResponseState`
- docs/conventions.md#logging (redacting Debug, `debug_redacts_*` tests), #dependencies (notices regeneration)
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-100`. No app launch is needed or allowed in this task.

## Decisions (fixed here; later tasks build on them)
- `Auth::OAuth2ClientCredentials { token_url, client_id, client_secret, scope, client_auth }` and `Auth::OAuth2AuthCode { auth_url, token_url, client_id, client_secret, scope, client_auth, redirect_port }`. `client_auth` is `ClientAuth::{Basic, Body}` (default Basic); `redirect_port: u16` with 0 meaning ephemeral; empty `client_secret` means a public client. TOML `type = "oauth2_client_credentials"` / `"oauth2_auth_code"`, keys named as the fields, `client_auth = "basic" | "body"`.
- `EngineAuth` gains matching variants with resolved strings, plus `persist_refresh: bool` on the auth-code variant. `resolve` always sets it to false; wave 3 sets it for keychain-backed environments. This, with the trait below, is the keychain seam.
- `wisp_core::auth::oauth2` declares `TokenKey` (grant, token_url, client_id, scope; Debug prints all four, none is secret) and `pub trait RefreshTokenStore: Send + Sync { load, save, forget }`, keyed by `TokenKey`, with `MemoryOnly` as the default. `EngineConfig` gains `refresh_tokens: Arc<dyn RefreshTokenStore>` and `cookies: CookieJar`, with a hand-written Debug for `EngineConfig`.
- `Command::AuthorizeStart { auth: EngineAuth, client: ClientSettings, timeout: Option<Duration> }` and `Command::AuthorizeCancel(RequestId)`. `submit` returns a `RequestId` for both. Events `AuthUrl { id, url }`, `AuthCompleted { id }` and `AuthFailed { id, error: EngineError }`. No token, code, verifier or refresh token ever appears in a command or event.
- `ErrorKind::Auth` is new.
- `pub struct Timings { dns: Option<Duration>, connect: Option<Duration> }` (connect is connect+TLS combined) is carried as `Event::Completed { .., timings }`. `None` means not measured or reused connection, never zero.
- `HistoryEntry` and `NewEntry` gain `dns_ms` and `connect_ms` (`#[serde(default)]`, so old index lines still load). `ResponseState` gains `dns` and `connect`.

## Acceptance
- [ ] AC1 The types, fields, commands, events and the `ErrorKind` listed under Decisions exist with those names. `cargo build --workspace --locked` passes, and the `Cargo.lock` diff adds no `[[package]]` entry.
- [ ] AC2 `Auth`, `EngineAuth`, `EngineConfig` and every new type that holds a secret have a hand-written Debug that prints `<redacted>` for `client_secret` and any token. Each has a `debug_redacts_*` test checking both `{:?}` and `{:#?}` against a marker value.
- [ ] AC3 `Auth::redacted()` passes every OAuth2 field through `keep_var_ref`, as the other variants do. A test shows a literal `client_secret` is emptied and `{{secret}}` is kept.
- [ ] AC4 `resolve` expands every OAuth2 field into `EngineAuth`, sets `persist_refresh: false`, and adds no `Authorization` header. It reports an unresolved variable as it does for Digest. Tests cover both variants.
- [ ] AC5 Codec: both OAuth2 types round-trip through TOML byte for byte (comments kept). Unknown `client_auth` values are a load error naming the key. Tests cover both.
- [ ] AC6 History: a `NewEntry` whose request has OAuth2 auth with literal `client_secret = "SECRETVAL"` stores no file containing `SECRETVAL`, and the entry's `redacted` list names `auth.client_secret`. `dns_ms`/`connect_ms` round-trip, and an index line without them loads with `None`. Tests in `crates/storage/tests/history.rs`.
- [ ] AC7 The engine answers `Send` with OAuth2 auth with `Failed { kind: Auth }` and the message "OAuth2 is not available in this build yet". It answers `AuthorizeStart` with `AuthFailed` carrying the same message. `Completed` carries `Timings { dns: None, connect: None }`. Tests in `engine.rs`'s unit tests or `crates/core/tests/engine.rs`.
- [ ] AC8 `ClientCache` moves to `crates/core/src/client.rs` with no behaviour change. It receives the `CookieJar` from `EngineConfig` and ignores it for now. Every existing core test passes unchanged.
- [ ] AC9 App: a request whose auth is OAuth2 opens without error. The auth editor shows its type name and fields read-only and never rewrites them on save. `on_event` ignores the three auth events. `Completed.timings` is copied into `ResponseState` and into `NewEntry`, and a history entry reopened through `ui/history.rs` restores `dns`/`connect`. `ErrorKind::Auth` maps to the label "Authorization failed".
- [ ] AC10 `cargo fmt --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` are clean, and `THIRD-PARTY-NOTICES` is regenerated if `Cargo.lock` changed.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks. Each asserts a minimum count with `--list` before it runs.
- `core` command — AC2, AC3, AC4, AC7, AC8
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-100; cargo test -p wisp-core --locked`
- `storage` command — AC5, AC6
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-100; cargo test -p wisp-storage --locked`
- `app` command — AC9
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-100; cargo test -p wisp-app --locked`
- `no-new-packages` command — AC1
  run: `! git diff main...HEAD -- Cargo.lock | rg -q '^\+name = '`
- `lint` command — AC10
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-100; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `surface` read — AC1, AC9
  how: the names match Decisions; no command or event carries a token, code or verifier; app changes are limited to arms, read-only display and pass-through.

## Out of scope
- Any real OAuth2 network behaviour, loopback listener or PKCE (task 103); the OAuth2 editor (106).
- Measuring timings (105), the cookie provider and the reqwest `cookies` feature (104), the cookie file (102).
- Importer mapping of Postman/OpenAPI OAuth2 auth (follow-up; not in wave 2).
- Task 99 hardening items in history.rs or engine.rs, even when adjacent.
