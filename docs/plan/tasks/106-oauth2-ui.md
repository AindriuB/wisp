# 106 — OAuth2 in the app: editor kinds, Authorize via the system browser, status

**Repo:** `.`
**Depends on:** 100, 101, 103
**Size impact:** low (GPUI code only; the browser opens through GPUI's `App::open_url`, so no new crate)
**Owns:**
- crates/app/src/ui/request/auth.rs
- crates/app/src/ui/oauth.rs (new: the authorize status strip)
- crates/app/src/ui/mod.rs (one `pub mod oauth;` line)
- crates/app/src/workspace.rs (authorize command, auth-event routing, cancel on tab close)
- crates/app/src/tab.rs (per-tab authorize state only)

## Goal
A user can pick either OAuth2 type in the auth editor and fill its fields; the secret field is masked. Client credentials work on Send with nothing more. For auth code, an Authorize button resolves the request's auth against the current environment and starts the engine flow. It opens the system browser at the engine's URL and always shows that URL with Copy, because GPUI cannot tell whether the browser opened. It shows Waiting / Authorized / Failed with a Cancel button.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#3-auth-core (OAuth2 failures: browser cannot open, port unavailable, state mismatch, token errors)
- Task 100 Decisions (the `Auth` fields, `Command::AuthorizeStart`/`AuthorizeCancel`, the `AuthUrl`/`AuthCompleted`/`AuthFailed` events); task 103 (engine behaviour and messages)
- crates/app/src/ui/request/auth.rs:40-140 — `AuthKind`, the field slots, `type_name`. Task 100 left OAuth2 shown read-only; make it editable here. The masked input mode is the one the Digest password uses.
- crates/app/src/workspace.rs:960-1000 — `scope_for` and `send`. Authorize resolves through the same path, and unresolved variables block it with the existing message. `send` sets `prepared.client.collection` from `collection_partition` (:474-479) and the engine partitions tokens by it and by `environment` (PR #42); Authorize must build both the same way. Task 107 later replaces `collection_partition` with a cached id; do not optimise it here.
- crates/app/src/ui/request/auth.rs:100-125 `build_auth` — maps both OAuth2 kinds to `Auth::Inherit`, guarded only by an early return elsewhere (42-4).
- crates/app/src/workspace.rs:1037 `on_event` — task 100's ignore-arms for auth events become routing to the tab that started the flow
- gpui-pre 0.3.10 `App::open_url` (src/app.rs:1928)
- App launch rules (hand check): run only the binary you built, with `WISP_DATA_DIR` set to a fresh `mktemp -d` (task 101 isolates `settings.toml` this way). Stop only the pid you started; never `pkill` or `killall`. A bundle, if built, uses a bundle id other than `io.github.aindriub.wisp`. Never write the user's real `settings.toml` or data dir. Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-106`.

## Acceptance
- [ ] AC1 The auth type select lists "OAuth2 client credentials" and "OAuth2 authorization code". Their fields are token URL, client ID, client secret (masked), scope and client auth (Basic/Body). Auth code adds authorization URL and redirect port (blank or 0 = any). Edits round-trip through the draft and save: a unit test in `auth.rs` goes `Auth` → slots → `Auth` for both types.
- [ ] AC2 The Authorize button shows only for auth code. When the auth has unresolved variables, Authorize is blocked with the same unresolved-variable message Send uses, and no command is submitted (unit-tested at the helper level).
- [ ] AC3 On `AuthUrl` the app calls `open_url` only if the URL's scheme is `https`, or `http` with host `127.0.0.1`/`localhost`/`::1`; anything else is refused with a notice. The pure check is unit-tested, and `javascript:` and `file:` are refused. The strip shows the URL selectable, with a Copy button and Cancel.
- [ ] AC4 `AuthCompleted` shows "Authorized" on the tab that started the flow. `AuthFailed` shows its message, with "cancelled" shown as "Cancelled". Events for a closed tab are dropped. Closing a tab while it waits submits `AuthorizeCancel`.
- [ ] AC5 Authorize and its events create no history entry. A history entry's OAuth2 `client_secret` is redacted and its other fields kept (100, 103 AC17); no new history code is added.
- [ ] AC6 No token, code or client secret is logged, displayed or copied. The only things copied are the authorization URL and the user's own field values.
- [ ] AC7 Hand check (macOS; agent or user), in the isolated launch above: (a) client credentials against a local token stub, e.g. a small `python3 -m http.server`-style script returning `{"access_token":"t","token_type":"bearer","expires_in":60}`, shows the response and `Authorization: Bearer t` reaches a local echo server. (b) Auth code with `http://127.0.0.1:<port>/authorize` as the authorization URL opens the browser at a URL holding `code_challenge` and `state`. Completing it with `curl '<redirect_uri>?code=x&state=<state>'` shows Authorized, and the next Send carries the token. (c) Cancel during the wait shows Cancelled. Record the steps run and their result in the PR.

- [ ] AC8 `AuthorizeStart` carries `environment` = the active environment's name (`None` when none is active) and `client` = the same `ClientSettings` Send would build for that tab, `client.collection` included. The command is built by a pure helper shared with Send's settings; a unit test asserts both fields for a tab with and without an active environment.
- [ ] AC9 Before the editor is reworked, the branch's first commit adds a test that `commit` and save leave an OAuth2 draft's auth value unchanged (both types, with a literal and a `{{var}}` secret). The test still passes at the head of the branch.
- [ ] AC10 (42-4) `build_auth` has an arm for each OAuth2 kind that builds the value from its slots; no kind other than `AuthKind::Inherit` maps to `Auth::Inherit`. A unit test runs `build_auth` for every `AuthKind` and asserts this.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `app-unit` command — AC1-AC4, AC8-AC10
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-106; test "$(cargo test -p wisp-app --locked oauth -- --list 2>/dev/null | rg -c ': test$')" -ge 7 && cargo test -p wisp-app --locked`
  (test names for AC1-AC4 and AC8-AC10 contain `oauth`; at 71f9e1a `ui::request::auth` had 6 tests, none matching)
- `first-commit` read — AC9
  how: `git log --reverse main..HEAD` — the first commit adds the OAuth2 draft test and changes no editor code.
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-106; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock`
- `ui-secrets` read — AC5, AC6
  how: no history append on the authorize path; no secret reaches a label, a log or the clipboard.
- `hand` read — AC7
  how: the PR lists each of (a)-(c) with the observed result, run with a temp `WISP_DATA_DIR`.

## Out of scope
- Keychain persistence of refresh tokens and setting `persist_refresh` (wave 3).
- Importer mapping of Postman/OpenAPI OAuth2 (follow-up).
- Cookie manager UI and engine-config jar wiring (task 107); timing display (105).
- Any change to `crates/core`.
- Caching the collection id and the clone/copy partition decision (task 107).
