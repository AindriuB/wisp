# 155 — Confirm before sending to a guarded environment (3A)

**Repo:** .
**Depends on:** none (2B = 140 and 2E = 143 are merged; 154 runs in parallel on `ui/sidebar.rs` only)
**Owns:**
- crates/app/src/guard.rs (new: pure decisions and their tests)
- crates/app/src/main.rs (insert-only: `mod guard;`)
- crates/app/src/actions.rs (one new action, no key binding)
- crates/app/src/palette.rs (one `CommandId` variant, its label, `ALL`)
- crates/app/src/ui/palette.rs (one `action_for` arm)
- crates/app/src/ui/url_bar.rs (Send goes through the guard; the guard badge beside the context strip)
- crates/app/src/ui/root.rs (guarded send, toggle listener, the confirm and hint prompts)
- crates/app/src/workspace.rs (edit (d) only: `suggest_confirm` plumbing in `finish_env_load`, one event variant, one pub wrapper over `resolve_id_then`; about 30 lines)

## Goal
For a (collection, environment) the user has guarded, Send asks "Send to
<environment>?" and does nothing on Cancel. The user turns the guard on or off
from the palette or by clicking a badge in the context strip, and an
environment that commits `suggest_confirm = true` offers once to turn it on.
It is an accident guard, per user, keyed by name (spec Feature 2b).

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "2b. Environment guard" (:119-143) — behaviour, storage, invariants; Amendment 2 (task 153) keeps `suggest_confirm`: the soft line is now 15,900,000 B.
- Seams already merged (task 139): `Settings.confirm_send: BTreeSet<GuardKey>` and `GuardKey` (`settings.rs:25-59`, load/save `:186-202`, `:311-317`); `Workspace::known_collection_id(ix)` (`workspace.rs:2482`); `update_settings` (`workspace.rs:3230`, saves and notices on failure); `active_collection_ix` (`:1392`).
- Hint (task 132): `LoadedEnvironment.suggest_confirm: SuggestConfirm {No, Yes, Malformed}` (`storage/src/environment.rs:145-160`, `vault.rs:409`); it arrives in `finish_env_load` (`workspace.rs:1938-1962`) as `result.loaded`.
- The two interactive send paths: `url_bar.rs:386-394` `on_send` (discards errors) and `root.rs:421-429` `on_send`. Resend from history opens a tab that is sent through these same paths. `Workspace::send` (`workspace.rs:2497`) and the autosend harness (`:1573-1587`) do not change.
- Prompts: `root.rs:241-270` `show_prompt` and the `prompt_up` / `has_active_prompt()` guard (`root.rs:50-75`, `workspace.rs:1281`).
- Id creation: `resolve_id_then` (`workspace.rs:3487`, private) — the toggle uses it so a guard is never stored without an id.
- `strip_text` (`url_bar.rs:54`) is called by `ui/explain.rs:336,486` tests: keep its signature; the badge is a separate element.
- Exhaustive registries this task must extend: `CommandId::ALL` (`palette.rs:~68`, count test `:366-373`) and `action_for` (`ui/palette.rs:27-48`).
- Baseline: `cargo test -p wisp-app --bin wisp -- --list` lists 360 tests at 55d55ba.
- Worktree: `./.claude/scripts/wt-new.sh . 155 environment-guard`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-155`. Run every command in the foreground; never start a background command; never use `pkill` or `killall` (the user may be running Wisp).

## Decisions (planner, inside the spec)
- The hint prompt is `["Enable", "Not now"]`, shown at most once per (collection root, environment name) per app run, only when the hint is `Yes` and the guard is off. If another prompt is up, it is not shown and does not count as shown.
- When the environment changed while the confirm prompt was up, nothing is sent; if the new environment is guarded the prompt is shown again for it, otherwise the notice "Environment changed; not sent" appears.

## Acceptance
- [ ] AC1 `guard.rs` has a pure `needs_confirm(settings, collection_id: Option<&str>, env_name: Option<&str>) -> bool`: true only when both are present and `GuardKey{id, env}` is in `confirm_send`. Table test covers: guarded, other env, other collection, no id, no environment, env name containing `:`.
- [ ] AC2 `guard.rs` has a pure `after_prompt(confirmed, current, answer, current_guarded)` returning one of Send, Abort, Reprompt, AbortChanged; table test covers Cancel, Confirm with the same target, Confirm after the environment changed (guarded and not), Confirm after the active collection or tab changed.
- [ ] AC3 Both send paths call one guarded entry point in `root.rs` (the URL bar's Send dispatches it rather than calling `Workspace::send`). For a guarded target it shows "Send to <environment>?" with detail "<METHOD> <host>" (host without userinfo), buttons `["Send", "Cancel"]`; Cancel sends nothing. An unguarded target sends with no prompt. Cancel during Sending still cancels without a prompt.
- [ ] AC4 The prompt reuses `prompt_up` / `has_active_prompt()`: no second prompt while one is up, and `prompt_up` is cleared on every path.
- [ ] AC5 Palette command "Toggle confirm before send (this environment)" and a click on the guard badge both toggle the active (collection, environment) through `update_settings`. With no active environment the notice is "Select an environment first". If the collection id is not yet known, enabling creates it through `resolve_id_then` first; a failed id gives a notice and stores nothing. Each toggle shows a notice ending "set per user; a renamed environment loses it".
- [ ] AC6 The badge beside the context strip shows only while the active target is guarded; `strip_text`'s signature is unchanged.
- [ ] AC7 `finish_env_load` emits one new `WorkspaceEvent` variant carrying root and environment name when the loaded hint is `Yes`; root shows the hint prompt per the Decisions, and Enable takes the AC5 path. A `Malformed` hint shows the notice "suggest_confirm in <env>.toml must be true or false; ignored" and never prompts. The hint never enables or disables a guard by itself.
- [ ] AC8 OAuth Authorize is not guarded; `Workspace::send`, `close_step`, `quit_step` and the autosend path are unchanged.
- [ ] AC9 `workspace.rs` changes stay within edit (d): the event variant, the hint emission in `finish_env_load`, and one pub wrapper over `resolve_id_then`; the reviewer sees no other hunk.
- [ ] AC10 No key binding added; theme tokens only; the full suite, fmt and clippy pass.

## Checks
- `guard-unit` command — AC1, AC2
  run: `cargo test -p wisp-app --bin wisp guard:: 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([7-9]|[1-9][0-9]+) passed'`
- `palette` command — AC5
  run: `cargo test -p wisp-app --bin wisp palette 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `untouched` command — AC8
  run: `git diff --exit-code main...HEAD -- crates/storage crates/core crates/cli crates/app/src/save_close.rs crates/app/src/settings.rs`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `flows` read — AC3, AC4, AC5, AC6, AC7, AC9
  how: the reviewer traces both send paths to the one entry point, the prompt_up handling on every branch, the toggle's id-creation branch, and the `workspace.rs` hunks against edit (d).

## Out of scope
- The guard docs, CLI docs and README (task 157); spec text (task 153).
- Persisting "hint already shown" across runs; guarding OAuth Authorize; any policy beyond the per-user accident guard (roadmap C5b).
- `ui/sidebar.rs` (task 154) and the PLAN follow-up `history_scope` Pending-for-Failed (139).
- Hand test on macOS: task 158's checklist.

## Wave stopped — step 2 — orchestrator finding: hint prompt wording

Reviewed HEAD a6a118da1605052e67dc6ac59852eb46f3ac1029. [SPEC] root.rs:637-639 hint text "Confirm before sending to {env}?" / "This environment asks to be guarded. …" vs spec :131 "This environment suggests confirming sends. Enable?". Pending in-spec finding, held: [STYLE] workspace.rs:3511 `resolve_id_for_guard` splits `resolve_id_then` from its doc comment; give the wrapper its own one-line doc.

## Wave resumed — user decision 2026-11-17: the hint prompt uses the spec's text (spec :131, "This environment suggests confirming sends. Enable?"), buttons stay ["Enable", "Not now"]

## Review round 1 — findings

Reviewed HEAD a6a118da1605052e67dc6ac59852eb46f3ac1029.

- [SPEC, decided] crates/app/src/ui/root.rs:637-639 — the suggest_confirm hint prompt reads "Confirm before sending to {env}?" / "This environment asks to be guarded. …". Change it to the spec's text, "This environment suggests confirming sends. Enable?" (docs/plan/specs/2026-11-14-wisp-0-3-0.md:131), naming the environment where the prompt form needs it. Keep the buttons ["Enable", "Not now"].
- [STYLE] crates/app/src/workspace.rs:3511 — the new `pub fn resolve_id_for_guard` sits between `resolve_id_then`'s doc comment and the function, so the comment now documents the wrapper (and misdescribes it: the wrapper drops the partition argument) and `resolve_id_then` has none. Move the wrapper so the comment stays on `resolve_id_then`, and give the wrapper its own one-line doc. Stays inside edit (d).

## Attempt 2 — failed

Reviewer REQUEST CHANGES at b638dccc31c71fb722cc4b4816f147cb8af5bf74. The `guard-unit` check matches only 3 tests (`needs_confirm_table`, `after_prompt_table`, `host_drops_userinfo_and_path`), and its grep needs `ok. [7-9]+ passed`, so it exits 1 although the tables cover AC1 and AC2. Fix: split the table rows of `needs_confirm_table` and `after_prompt_table` (crates/app/src/guard.rs:94-173) into individually named tests whose names read as sentences (docs/conventions.md:106), at least 7 under `guard::`, and keep every existing case. No production code change.

## Wave verified — 9d5837a548e8d81769c165ac9753d87b0ae242d4

## Outcome
Merged via record/155 at 9d5837a (stacked on record/156 and record/154). Sends to a guarded environment ask first, `suggest_confirm` offers to enable the guard with the spec's text, and a palette command and badge show and toggle it. Two rounds: review round 1 (hint wording, doc comment) and Attempt 2 (guard-unit gate, table tests split). Tester: app 375, guard-unit 15, fmt and clippy clean, no flakes. Follow-ups are in PLAN.
