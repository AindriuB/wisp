# 190 — Configure TLS trust in the GUI

**Repo:** .
**Round:** 0
**Depends on:** 183, 184, 189
**Owns:**
- crates/app/src/settings.rs
- crates/app/src/actions.rs
- crates/app/src/palette.rs
- crates/app/src/ui/palette.rs
- crates/app/src/ui/root.rs (the new action handlers only)
- crates/app/src/ui/response/mod.rs (the TLS hint only)
- crates/app/src/workspace.rs (engine start and trust apply; the AC5 carry-overs)

## Goal
The user can trust their OS certificate store and/or one PEM CA file from the
GUI. The setting is stored per user in `settings.toml`, applies to the next
send without a restart, and a TLS unknown-issuer failure tells the user where
to fix it. This task also closes three small `workspace.rs` follow-ups.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 5": the "Where it is configured" paragraph and its defaults. This brief assumes open questions 1 and 2 are answered as recommended; the orchestrator amends it if not.
- Task 184 API: `TlsTrust`, `TlsTrust::load_pem_file`, `EngineConfig.tls`, `Engine::set_tls_trust`.
- crates/app/src/settings.rs:47-75 `Settings` and its TOML read/write.
- The `ToggleHistoryBodies` pattern: `palette.rs:26,81,106`, `actions.rs:30`, `ui/palette.rs:46`, `ui/root.rs:1151-1161`.
- The file picker: `cx.prompt_for_paths` (`ui/root.rs:294`).
- crates/app/src/workspace.rs:1251 `Engine::start`. crates/app/src/ui/response/mod.rs:54-65 `error_words`/`error_text`.
- PLAN.md "Follow-ups from task 161" (the `expect` at `append_history`; `late_append_after_newid_does_not_pin_old_store`) and "from task 178" (`truncated_sidecar_history_body_stays_equal_to_the_stored_body`, ~:6378).
- Worktree: `./.claude/scripts/wt-new.sh . 190 tls-trust-gui`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-190`. Foreground commands only; never `pkill` or `killall`; stop only a pid you launched.

## Acceptance
- [ ] AC1 `Settings` gains `[tls] system_trust` (bool, default true) and `ca_bundle` (an absolute path; a relative or empty value is ignored with a log line that does not include the path). Both are read and written without disturbing other keys. Tests `tls_settings_*`: default, round-trip, and a relative path ignored.
- [ ] AC2 Startup: the engine starts with the trust from settings before any send can be issued. If the CA file fails to load, the engine starts without it and a notice reads "CA certificate file not loaded: <reason>". The path is display-escaped and no file content is shown.
- [ ] AC3 Three palette commands:
  - "Toggle system certificate trust": the notice states the new state.
  - "Choose CA certificate file…": a file picker. On success the notice reads "Trusting N certificates from <file name>" and the setting is saved. On failure it shows a notice and the setting is unchanged.
  - "Clear CA certificate file".

  Each applies through `Engine::set_tls_trust` without a restart, and file loads run off the UI thread.
- [ ] AC4 A `Tls` failure whose message shows an unknown or untrusted issuer gets one extra line in the response pane naming the system-trust and CA-file commands. Other TLS failures are unchanged. Pure test `tls_hint_*` on the decision function (positive and negative).
- [ ] AC5 Carry-overs:
  - `append_history`'s `recovered.expect("checked above")` becomes an `if let`;
  - `late_append_after_newid_does_not_pin_old_store` asserts the `Ready` store is B's;
  - `truncated_sidecar_history_body_stays_equal_to_the_stored_body` uses a Text sidecar body over 1 MiB and compares a non-empty cut body.
- [ ] AC6 A gpui test `tls_apply_*` shows that choosing, toggling and clearing change the trust the workspace hands to the engine. Assert through a test seam or getter, not through a network send.
- [ ] AC7 No file outside Owns changes. No new dependency. fmt, clippy and the workspace suite pass.

## Checks
- `tls-gui-tests` command — AC1, AC4, AC6
  run: `cargo test -p wisp-app --locked tls_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([6-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `carry-overs` command — AC5
  run: `cargo test -p wisp-app --locked -- late_append_after_newid_does_not_pin_old_store truncated_sidecar_history_body_stays_equal_to_the_stored_body 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. 2 passed; 0 failed' && ! rg -n 'expect\("checked above"\)' crates/app/src/workspace.rs`
- `scope` command — AC7
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/app/src/settings.rs$' -e '^crates/app/src/actions.rs$' -e '^crates/app/src/palette.rs$' -e '^crates/app/src/ui/palette.rs$' -e '^crates/app/src/ui/root.rs$' -e '^crates/app/src/ui/response/mod.rs$' -e '^crates/app/src/workspace.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `fail-closed` read — AC2, AC3
  how: the reviewer checks that a failed load never leaves a previous wider trust in place, unless that trust was already in effect and unchanged, and that no notice or log line prints file content.

## Out of scope
- CLI flags (191). Per-collection or per-environment trust. mTLS.
- A settings window or dialog; palette commands only.
- Showing the trust state in Explain.
