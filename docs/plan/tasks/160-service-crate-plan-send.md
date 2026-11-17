# 160 — Create wisp-service and move plan and send into it (1B-i, C1)

**Repo:** .
**Round:** 0
**Depends on:** 159
**Owns:**
- crates/service/** (new crate; this task creates every module file)
- crates/app/src/workspace.rs (edit (a) only: the plan/send rewiring)
- crates/app/src/tab.rs (move `HistoryOrigin` and `CheckOutcome` out; re-export them)
- crates/app/src/vault.rs (move `AppStores`, `service_name`, `portable_marker`, `KEYCHAIN_SERVICE_ENV` out; re-export)
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
- [ ] AC1 `crates/service` exists with `lib.rs` declaring `plan`, `send`, `outcome`, `history`, `secrets`; it depends on `wisp-core` and `wisp-storage` only (plus workspace-shared crates those already use); `crates/app` depends on it; the root `Cargo.toml` has the workspace dependency and `[profile.release.package.wisp-service] opt-level = "z"`.
- [ ] AC2 `wisp_service::plan` holds `SendInput`, `SendPlan { prepared, scope_path, resend: Option<ResendFacts>, dynamics: Vec<DynamicValue> }`, `prepare(&SendInput, &mut dyn DynamicSource)`, `prepare_authorize`, `scope_path_for` (signature unchanged), `body_redaction_refusal`, `recorded_request` and `HistoryOrigin`, with bodies moved unchanged except that `prepare` returns `ResendFacts` (the arguments `prepare_send` passed to `resend_notice`) and the app formats the notice with `history_ui::resend_notice` as before.
- [ ] AC3 `prepare` resolves through `resolve_with`; `Workspace::send` passes a `Recording` source and the plan's `dynamics` are the recorded values. A service test sends a prepared request with `{{$uuid}}` in a header through a real `Engine` to a local `std::net::TcpListener` that echoes the header, and the echoed value equals `dynamics[0].value`.
- [ ] AC4 `wisp_service::send` holds `SentRecord` (the four fields of `Sent`), `Submitted { id, record }` and `submit(engine, plan, draft, partition, root) -> Submitted`; `submit_plan` calls it. It is the only non-test `Command::Send(` outside `crates/core`.
- [ ] AC5 `wisp_service::outcome` holds `CheckOutcome` (moved from `tab.rs`, which re-exports it); `wisp_service::history` is a documented empty module that 161 fills. `tab.rs` re-exports `HistoryOrigin`.
- [ ] AC6 `wisp_service::secrets` holds `KeychainStores` (was `AppStores`, same body), `service_name`, `portable_marker` and `KEYCHAIN_SERVICE_ENV`; `vault.rs` re-exports them under the old names so `main.rs` and every other app call site is unchanged.
- [ ] AC7 `scripts/check-boundaries.sh`: the cargo tree loop includes `wisp-service`; a new check fails unless `Command::Send(` occurs exactly once outside `crates/core` and that occurrence is in `crates/service/src/send.rs`; a new check fails if `crates/app/src/workspace.rs`, above its first `#[cfg(test)]`, contains a free-function `resolve(` call (`DataDir::resolve()` and method calls do not count).
- [ ] AC8 Behaviour-preserving: no existing test's expectations change; tests that move with their functions keep their names and pass in `wisp-service`; no file outside Owns changes.
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
  how: the reviewer compares each moved body with its 0.3.0 original (`git diff -M`), checks the re-exports and that the `workspace.rs` hunks are limited to edit (a).

## Out of scope
- `check_outcomes`, `assertion_records`, `NewEntry` construction, `HistoryJob::open`, AC52, AC75, AC51 call site, `delivery` (161).
- `run.rs` (166); any change to `Command`, `Event`, `ErrorKind` or `PreparedRequest`.
- A trait-object "SendService" (spec :124 rejects it).
