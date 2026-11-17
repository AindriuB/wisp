# 160 — Create wisp-service and move plan and send into it (1B-i, C1)

**Repo:** .
**Round:** 0
**Depends on:** 159
**Owns:**
- crates/service/** (new crate; this task creates every module file, including `src/vault.rs` and a `tempfile` dev-dependency in its `Cargo.toml` — Amendment 1)
- crates/app/src/workspace.rs (edit (a) only: the plan/send rewiring; plus, under `#[cfg(test)]`, removing the plan-function tests and the test-only shim — Amendment 1)
- crates/app/src/tab.rs (move `HistoryOrigin` and `CheckOutcome` out; re-export them)
- crates/app/src/vault.rs (move `AppStores`, `service_name`, `portable_marker`, `KEYCHAIN_SERVICE_ENV` and the `keychain_service_is_the_variable_when_set_and_non_empty` test out; re-export)
- crates/app/Cargo.toml (add `wisp-service`)
- Cargo.toml (workspace dependency and `[profile.release.package.wisp-service] opt-level = "z"`)
- Cargo.lock
- scripts/check-boundaries.sh (exit checks 1 and 2, and the `resolve(` half of 3)

## Goal
A GPUI-free crate `crates/service` (package `wisp-service`, lib `wisp_service`)
holds the send planning and the one `Command::Send` construction, and the GUI
calls it. No behaviour changes: the GUI sends exactly as in 0.3.0. Outcome and
history move in 161.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Shape" (:46-52), "Feature 1" (:54-138: what moves :62-70, API :72-114, approach :118-124, exit check :126-130), Waves 1B-i (:457).
- crates/app/src/workspace.rs: `Sent` :230, `scope_path_for` :722, `body_redaction_refusal` :1039, `SendPlan` :1082, `authorize_command` :1094, `prepare_authorize` :1179, `prepare_send` :1193 (calls `history_ui::resend_notice` :1222-1231), `recorded_request` :1241, `Workspace::send` :2523-2595, `submit_plan` :2597 (`Command::Send` :2609). Tests start at the first `#[cfg(test)]` (:3781).
- crates/app/src/tab.rs:62 `CheckOutcome`, :125 `HistoryOrigin`; users: `ui/response/mod.rs:23`, `ui/response/checks.rs:2` (keep compiling via the re-export).
- crates/app/src/vault.rs:26-73; `main.rs:191` uses `vault::AppStores` (keep via the re-export).
- Root `Cargo.toml:18-25` (why workspace members need an explicit profile entry); `scripts/check-boundaries.sh:8` (the cargo tree loop).
- 159's `resolve_with`, `Recording`, `DynamicValue` (merged first).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 160 service-crate-plan-send`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-160`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `crates/service` exists with `lib.rs` declaring `plan`, `send`, `outcome`, `history`, `vault` (Amendment 1; was `secrets`); it depends on `wisp-core` and `wisp-storage` only (plus workspace-shared crates those already use, and `tempfile` as a dev-dependency); `crates/app` depends on it; the root `Cargo.toml` has the workspace dependency and `[profile.release.package.wisp-service] opt-level = "z"`.
- [ ] AC2 `wisp_service::plan` holds `SendInput`, `SendPlan { prepared, scope_path, resend: Option<ResendFacts>, dynamics: Vec<DynamicValue> }`, `prepare(&SendInput, &mut dyn DynamicSource)`, `prepare_authorize`, `scope_path_for` (signature unchanged), `body_redaction_refusal`, `recorded_request` and `HistoryOrigin`, with bodies moved unchanged except that `prepare` returns `ResendFacts` (the arguments `prepare_send` passed to `resend_notice`) and the app formats the notice with `history_ui::resend_notice` as before.
- [ ] AC3 `prepare` resolves through `resolve_with`; `Workspace::send` passes a `Recording` source and the plan's `dynamics` are the recorded values. A service test sends a prepared request with `{{$uuid}}` in a header through a real `Engine` to a local `std::net::TcpListener` that echoes the header, and the echoed value equals `dynamics[0].value`.
- [ ] AC4 `wisp_service::send` holds `SentRecord` (the four fields of `Sent`), `Submitted { id, record }` and `submit(engine, plan, draft, partition, root) -> Submitted`; `submit_plan` calls it. It is the only non-test `Command::Send(` outside `crates/core`.
- [ ] AC5 `wisp_service::outcome` holds `CheckOutcome` (moved from `tab.rs`, which re-exports it); `wisp_service::history` is a documented empty module that 161 fills. `tab.rs` re-exports `HistoryOrigin`.
- [ ] AC6 `wisp_service::vault` (file `crates/service/src/vault.rs`; Amendment 1) holds `KeychainStores` (was `AppStores`, same body), `service_name`, `portable_marker` and `KEYCHAIN_SERVICE_ENV`; `vault.rs` re-exports them under the old names so `main.rs` and every other app call site is unchanged.
- [ ] AC7 `scripts/check-boundaries.sh`: the cargo tree loop includes `wisp-service`; a new check fails unless `Command::Send(` occurs exactly once outside `crates/core` and that occurrence is in `crates/service/src/send.rs`; a new check fails if `crates/app/src/workspace.rs`, above its first `#[cfg(test)]`, contains a free-function `resolve(` call (`DataDir::resolve()` and method calls do not count).
- [ ] AC8 Behaviour-preserving: no existing test's expectations change; tests that move with their functions keep their names and pass in `wisp-service`; no file outside Owns changes. Exception (Amendment 1): the four moved tests that asserted on the resend notice text assert on the equivalent `ResendFacts` fields instead. The plan-function tests live in `#[cfg(test)] mod tests` of `crates/service/src/plan.rs`, the `service_name` test in `crates/service/src/vault.rs`, and `workspace.rs` has no test-only copy of `SendPlan`/`prepare_send`/`recorded_request`.
- [ ] AC9 `workspace.rs` changes are edit (a) only: `Workspace::send` keeps its tab reads, pending check and refusal notice (:2528-2560) and its id-wait path; `on_event`, `append_history` and `HistoryJob` are untouched.

## Checks
- `service-unit` command — AC2, AC3, AC4
  run: `cargo test -p wisp-service --locked 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `boundaries` command — AC1, AC7
  run: `scripts/check-boundaries.sh && test "$(rg -c 'Command::Send\(' crates | grep -v '^crates/core/')" = "crates/service/src/send.rs:1"`
- `no-resolve` command — AC7
  run: `sed '/^#\[cfg(test)\]/,$d' crates/app/src/workspace.rs | rg -n '(^|[^:.\w])resolve\('; test $? -eq 1`
- `untouched` command — AC8
  run: `git diff --exit-code main...HEAD -- crates/core crates/storage crates/cli crates/app/src/main.rs crates/app/src/ui`
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `lift` read — AC2, AC5, AC6, AC9
  how: the reviewer compares each moved body with its 0.3.0 original (`git diff -M`), checks the re-exports and that the `workspace.rs` hunks are limited to edit (a) plus the Amendment 1 test removals.
- `tests-moved` read — AC8
  how: the reviewer checks that the plan-function tests and the `service_name` test now sit in `crates/service/src/plan.rs` and `vault.rs` with names unchanged, the shim at old workspace.rs:4123-4158 is gone, and the only changed expectations are the four notice-to-`ResendFacts` swaps.

## Out of scope
- `check_outcomes`, `assertion_records`, `NewEntry` construction, `HistoryJob::open`, AC52, AC75, AC51 call site, `delivery` (161).
- `run.rs` (166); any change to `Command`, `Event`, `ErrorKind` or `PreparedRequest`.
- A trait-object "SendService" (spec :124 rejects it).

## Amendment 1 (user, 2026-11-17)
Reason: the wave stopped at the round-1 fix. The project deny rule `Read(./**/secrets.*)` (.claude/settings.json:46) blocks agents from reading `secrets.rs`, and the review found the plan-function tests left behind in the GUI crate.

1. Rename the service module `wisp_service::secrets` to `wisp_service::vault` (file `crates/service/src/vault.rs`); `vault` also matches the app's `crates/app/src/vault.rs`. AC1 and AC6 changed. The app re-exports stay under their old names.
2. The plan-function unit tests move from `crates/app/src/workspace.rs` (the test-only shim at about :4123-4158) into `#[cfg(test)] mod tests` in `crates/service/src/plan.rs`, names unchanged, and the shim is removed.
   - `folder_bearer_token_follows_the_environment`, `edited_url_drops_the_folder_scope`, `missing_original_falls_back_to_collection_defaults` and `resend_chain_keeps_scope` assert on the equivalent `ResendFacts` fields (`env`, `url_edited`, `original_missing`) instead of the notice text. This is an allowed exception to AC8.
   - Tests that exercise app-side `sanitize_redacted`, `sanitized_origin` or `resend_notice_text` (e.g. `opening_an_entry_sanitises_it`, `redacted_fields_never_send_an_empty_value_or_override_the_environment`, `sanitize_redacted_drops_a_redacted_path_var_and_refuses_the_resend`, `oauth_authorize_command_carries_environment_and_client`) stay in the app and call `wisp_service::plan::prepare` directly.
   - `keychain_service_is_the_variable_when_set_and_non_empty` moves from crates/app/src/vault.rs:1260 to `crates/service/src/vault.rs`.
3. `tempfile` may be added as a dev-dependency of `wisp-service`.

## Review round 1 — findings

Reviewed HEAD: a62ab87cf0a7360ac6ea59eaa0cc04fecab67c16

- [STYLE] crates/app/src/workspace.rs:4123 — The pure plan functions moved to `wisp_service::plan`, but their unit tests stayed in the GUI crate. They run there through a test-only copy of the 0.3.0 `SendPlan`/`prepare_send`/`recorded_request` (workspace.rs:4123-4158; the users are `plan(` :4203, `try_send(` :4264, `missing_original_falls_back_to_collection_defaults` :4431, `resend_chain_keeps_scope` :4449, and the scope_path_for test at :4444). The `service_name` test at crates/app/src/vault.rs:1261 also stayed behind while the function moved to `crates/service/src/secrets.rs`. As a result, `cargo test -p wisp-service` holds one test, and nothing in that crate tests `body_redaction_refusal`, `empty_json_leaves`, `scope_path_for` or `prepare`'s refusal and ResendFacts paths without GPUI.
  route: implementer — move those tests, with their names unchanged, into the `#[cfg(test)] mod tests` of `crates/service/src/plan.rs` and `secrets.rs`, and drop the shim. Both files are in Owns and no test expectation changes (AC8 allows moved tests).
  rule: docs/conventions.md:101 ("Unit tests live in the module they test"); docs/plan/specs/2026-11-17-wisp-0-4-0.md:129 ("moved unit tests pass in their new crate").

## Wave stopped — step 2 — permissions block the round-1 fix
The round-1 fix implementer changed nothing. The project deny rule `Read(./**/secrets.*)` (.claude/settings.json:46) blocks `crates/service/src/secrets.rs`, a file this task owns, and two read-only compound Bash commands were refused. Open questions: whether the four notice-asserting tests that move may assert on `ResendFacts` instead of notice text, and whether `tempfile` may be added as a dev-dependency of `wisp-service`.

## Wave resumed — user: rename secrets to vault, moved tests assert ResendFacts, tempfile dev-dep (Amendment 1)

## Review round 2 — findings

Reviewed HEAD: a62ab87cf0a7360ac6ea59eaa0cc04fecab67c16 (Amendment 1 not yet implemented)

- [STYLE] crates/app/src/workspace.rs:4123 — The test-only shim (the old `SendPlan`, `prepare_send`, `recorded_request`, at :4123-4158) is still there. The plan-function tests still run in the GUI crate through it, and `cargo test -p wisp-service` runs only one test (`dynamic_header_value_reaches_the_wire_and_is_recorded`, crates/service/src/send.rs). Nothing in the service crate tests `body_redaction_refusal`, `empty_json_leaves`, `scope_path_for`, or `prepare`'s refusal and ResendFacts paths. This is the round-1 finding, unchanged.
  route: implementer — move the tests, names unchanged, into `#[cfg(test)] mod tests` in crates/service/src/plan.rs. Switch the four notice tests to assert on `ResendFacts` fields (`env`, `url_edited`, `original_missing`), as Amendment 1 item 2 allows. Keep the tests that exercise `sanitize_redacted`, `sanitized_origin` or `resend_notice_text` in the app, calling `wisp_service::plan::prepare` directly. Then delete the shim.
  rule: docs/conventions.md:101 ("Unit tests live in the module they test"); AC8 as amended.
- [STYLE] crates/app/src/vault.rs:1261 — `keychain_service_is_the_variable_when_set_and_non_empty` is still in the app crate. `service_name` has moved to the service crate.
  route: implementer — move the test to `#[cfg(test)] mod tests` in crates/service/src/vault.rs, name unchanged (Amendment 1 item 2, last bullet).
  rule: docs/conventions.md:101; AC8 as amended.
- [P2] crates/service/src/lib.rs:7 — The module is still `pub mod secrets` (file crates/service/src/secrets.rs). The app re-exports from `wisp_service::secrets::*` at crates/app/src/vault.rs:27-29. AC1 and AC6 (Amendment 1 item 1) require `wisp_service::vault` in crates/service/src/vault.rs.
  route: implementer — rename the file and module to `vault` (use `git mv`; the deny rule blocks reading secrets.rs but not renaming it), and point the two `pub use` lines in crates/app/src/vault.rs at `wisp_service::vault`. The old names stay as they are.
  input: `use wisp_service::vault::KeychainStores;` → does not compile at HEAD (no module `vault`).

Orchestrator (not for the implementer): spec docs/plan/specs/2026-11-17-wisp-0-4-0.md :70, :108, :149 still say `wisp_service::secrets`; the scribe updates them to `vault` when recording 160 (follows from Amendment 1).
