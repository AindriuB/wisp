# 69 — Clear the small app and core code nits left by 25, 27, 28, 52 and 55

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/ui/root.rs
- crates/app/src/ui/request/path.rs
- crates/app/src/ui/request/auth.rs
- crates/app/src/ui/request/settings.rs
- crates/app/src/actions.rs (the `key` doc comment only)
- crates/app/src/settings.rs (the test module only)
- crates/app/src/palette.rs (the test module only)
- crates/core/src/resolve/tests.rs
- docs/plan/tasks/69-app-core-nits.md (the `## Notes` section only)

**Build:** run every cargo command with
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-69`.

## Goal
Remove dead calls and stray output and fix misleading comments and test names that
earlier reviews left open, so later readers neither delete load-bearing code nor
keep dead code. No behaviour changes.

## Context
- crates/app/src/ui/root.rs:300-301 — the tab-close `div().on_mouse_down(..)`
  wrapper (55): it stops the mouse-down reaching the tab so pressing × does not
  select the tab first; it looks redundant next to the button's own
  `cx.stop_propagation()` in `on_click`. :5-9 the `gpui::{..}` import list.
- crates/app/src/ui/request/path.rs:143 and :168, auth.rs:356, settings.rs:162 —
  `.id("…-editor")` before `.overflow_y_scrollbar()`; 52 found the `Scrollable`
  wrapper sets its own id, so these do nothing.
- crates/app/src/actions.rs:54-57 — `key(index)` doc says "bound to `name`" but it
  takes an index into `KEYS`.
- crates/app/src/settings.rs:226 — `location_prefers_env_then_config_dir_then_none`;
  its first case is the portable marker, not an env var.
- crates/app/src/palette.rs:308-317 — the `ranking` test asserts only the first
  two results for `sen`.
- crates/core/src/resolve/tests.rs:457 — a leftover `eprintln!("elapsed: ..")`.

## Acceptance
- [ ] root.rs: a one-line comment above the close-button wrapper says why the
      mouse-down is stopped (so × does not activate the tab); `MouseButton` is
      imported in the `gpui::{..}` list and written unqualified.
- [ ] The four `.id("path-editor")`, `.id("auth-editor")`, `.id("settings-editor")`
      calls (path.rs twice) are removed. If removing one fails to compile or
      changes scrolling, keep it, add a comment saying why, and say so in `## Notes`.
- [ ] actions.rs: the `key` doc comment describes the index into `KEYS`; code unchanged.
- [ ] settings.rs: the test is renamed to describe its real order (for example
      `location_prefers_portable_then_env_then_config_dir_then_none`, matching the
      cases it checks); its body is unchanged.
- [ ] palette.rs: the `sen` ranking assertion checks the full order of all four
      items, consistent with the comment's rule (prefix, contiguous mid-word,
      scattered); if the current order contradicts the comment, report it in
      `## Notes` rather than changing `palette.rs` outside the test module.
- [ ] resolve/tests.rs: the `eprintln!` is gone; `rg -n 'eprintln' crates/core/src`
      returns nothing.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo fmt --all --check` pass.
- [ ] Binary size: deletion-only in shipped code; no measurement required. Note in
      `## Notes` if `cargo build --release -p wisp-app` grows at all.
- [ ] Do not open a PR or push; commit on `task/69-app-core-nits` only.

## Out of scope
- Any behaviour change: dirty-tab close prompt, timeout validation in Settings,
  palette matching of multi-char lowercase letters (25), `bind_keys_indices_match_names`
  strength (28).
- crates/app/src/ui/response/** (task 66) and crates/storage/** (task 67).
- `overflow_hidden` on `resizable_panel()` calls — do not add it (61).
- docs/, PLAN.md, HISTORY*.md (scribe).

## Notes
- All four `.id("…-editor")` calls removed; compiled and clippy clean. Removing them left `InteractiveElement` unused in path.rs, auth.rs and settings.rs, so those imports were dropped (outside the listed edits, same files). Scrolling not checked visually.
- palette `sen` order is [1, 2, 3, 0] (Send request, Resend, Save token, scattered): consistent with the comment.
- `wisp-core --test engine a_20_mib_file_body_streams_with_content_length` failed once under a parallel full run and passed on rerun; looks flaky, not touched.
- Release build size not measured (deletion-only).

## Review round 1 — findings

Reviewed HEAD: a4385e1.

- [P3] crates/app/src/settings.rs:226 — the renamed test `location_prefers_portable_then_env_then_config_dir_then_none` still names an "env" case it never checks; `location()` (:42-51) has no env branch. Rename to `location_prefers_portable_then_config_dir_then_none`; body unchanged.
