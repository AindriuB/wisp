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

## Review round 1 — findings
Reviewed HEAD: 53ce0655497450b1a68b5635617df56d24f21d4e

- [P2] crates/app/src/workspace.rs:3230 — The AC6 tests (`tls_apply_choose_toggle_and_clear_change_the_trust` :5248 and `tls_apply_startup_loads_the_ca_file_or_starts_without_it` :5226) assert through `tls_trust()`. That getter rebuilds the value from `self.settings.system_trust` and `self.tls_extra`. It does not record what was passed to `Engine::start` or `Engine::set_tls_trust`, so both tests still pass when the engine never gets the trust.
  route: implementer — AC6 already asks for a test seam or getter that shows the trust the workspace hands to the engine. Fix: record the value in `apply_tls` and in the `EngineConfig.tls` passed at :1270 (for example a `last_applied_tls` field, or one test-only field), then assert on that. This stays inside workspace.rs.
  input: delete `self.apply_tls();` at :3245, `w.apply_tls();` at :3280 or `self.apply_tls();` at :3309, or change `tls: TlsTrust {..}` at :1270 to `TlsTrust::default()`  →  both `tls_apply_*` tests still pass, while the engine keeps its old trust or starts without the CA file.
  proof: not run. It follows from the code: `tls_trust()` at :3230-3235 reads no state that `apply_tls` or `Engine::start` writes.
- [STYLE] crates/app/src/settings.rs:404-431 — `save_tls` runs on every settings save, including unrelated ones such as a theme toggle. It re-inserts `ca_bundle` with `t.insert`, which in toml_edit 0.22.27 (`table.rs:412-418`) replaces the value and its decor and resets the key format. It also removes an explicit `system_trust = true`. Both drop the user's comments and formatting.
  route: implementer — keep the line when its value already matches, as `set_if_differs` does, and keep the decor when it changes. The behaviour stays the same.
  input: settings.toml `[tls]\nca_bundle = "/etc/ca.pem"  # work CA\n`, then run "Toggle theme"  →  the file is rewritten as `ca_bundle = "/etc/ca.pem"` without the comment. Likewise, `system_trust = true  # keep` is removed entirely.
  rule: docs/conventions.md:96 ("Writes go through `toml_edit` so user comments and formatting survive"); the neighbouring idiom is settings.rs:266-276 (`set_if_differs`: an unchanged value is left alone, and a changed one keeps "the original line's decor (spacing, trailing comment)").

## Review round 2 — findings
Reviewed HEAD: 5097f02d6fc5d37e05c6df5fd7b356e13c88b2cb

- [P2] crates/app/src/workspace.rs:5281-5283 — `tls_apply_choose_toggle_and_clear_change_the_trust` still passes when `clear_ca_file` never gives the engine the new trust. The test toggles system trust right after the clear. That toggle calls `apply_tls`, which rebuilds the trust from `tls_extra = []`, so it hides a missing apply in the clear. This is one of the mutations named in the round 1 finding.
  route: implementer — assert `w.tls_trust().extra.is_empty()` (with `system` still false) between `clear_ca_file` and the final `toggle_system_trust`. This is the test seam AC6 already asks for, and it changes only workspace.rs.
  input: delete `self.apply_tls();` in `clear_ca_file` (workspace.rs:3315)  →  both `tls_apply_*` tests still pass, while the engine keeps trusting the cleared CA file until the next toggle.
  proof: ran it on an archive copy of HEAD with that one line removed: `cargo test -p wisp-app --locked tls_apply_` gave "ok. 2 passed; 0 failed".
- [STYLE] crates/app/src/settings.rs:403-412, 434-440 — Changing a `[tls]` value still drops user formatting, in two ways:
  - `set_keep_decor` calls `TableLike::insert`. In toml_edit 0.22.27 (table.rs:417) that runs `Key::fmt()`, which deletes a comment line above the key and resets the key's spacing.
  - Turning system trust back on removes an existing `system_trust = false  # c` line, comment included, where the neighbouring idiom rewrites the value and keeps the comment.
  route: implementer — copy `set_if_differs` (settings.rs:260-281): replace the item in place through `get_mut(key)` and restore the value decor, and only `insert` when the key is absent. For system trust going false→true, rewrite an existing key to `true` instead of removing it. Behaviour stays the same.
  input: settings.toml `[tls]\n# my work CA\n  ca_bundle   = "/etc/ca.pem"  # c\n`, then choose a different CA file  →  output `[tls]\nca_bundle = "/etc/new.pem"  # c`, without the `# my work CA` line. A standalone toml_edit 0.22.27 probe showed this. The same edit through `doc["tls"]["ca_bundle"] = ...` (the `set_if_differs` way) keeps the comment line and the spacing. Separately, `[tls]\nsystem_trust = false  # off for proxy\n`, then "Toggle system certificate trust"  →  the line and its comment are gone. By contrast, theme dark→light keeps `theme = "light"  # comment` through `set_if_differs`.
  rule: docs/conventions.md:96 ("Writes go through `toml_edit` so user comments and formatting survive"); neighbouring idiom settings.rs:268-276.

## Wave verified — 8ff63bb7324bc13a9aa61463cbff5b31695377b0

## Outcome
Merged after two review rounds; verified at 8ff63bb7324bc13a9aa61463cbff5b31695377b0 (tester PASS, reviewer APPROVE). Delivered AC1-AC7: `[tls]` settings, three palette commands, startup trust, the unknown-issuer hint, and the three `workspace.rs` carry-overs. Three P3 follow-ups went to PLAN.md under "Follow-ups from task 190". Round 1 and 2 findings above: tests must assert what the engine was given, and `[tls]` writes must keep user comments.
