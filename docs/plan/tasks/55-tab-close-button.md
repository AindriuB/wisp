# 55 — Add a close button to each request tab

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/ui/root.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-55`.

## Goal
M3 (v1 hardening). Closes the PLAN.md gap under "Workspace shell (28)": tabs have no close button, only ⌘W. Each tab gets a small × that closes that tab, not necessarily the active one, with the same behaviour as ⌘W.

## Context
- crates/app/src/ui/root.rs:290-308 — the `TabBar` and the `Tab::new().label(..)` per tab; the gpui-component 0.8.1 `Tab::suffix(impl IntoElement)` exists (tab/tab.rs:693).
- crates/app/src/ui/root.rs:393-399 — the `CloseTab` action calls `Workspace::close_tab(ix, cx)`, which is pub (workspace.rs:384) and handles cancel, body release and active index.
- `IconName::Close` is already embedded (crates/app/assets/icons/close.svg) and used at kv.rs:349 and request/body.rs:587. The icon-walk test in crates/app/src/assets.rs:125 fails if a new, unembedded `IconName` is used. Use `Close` only.
- PLAN.md "Palette and environment switcher (34)" lesson: key bindings dispatch before listeners, and an overlay that takes focus must restore it.

## Acceptance
- [ ] Each tab shows an `IconName::Close` ghost/xsmall button as its suffix. Clicking it calls `close_tab` with that tab's index. The click does not also activate the tab (propagation stopped).
- [ ] `git diff main -- crates/app/src/assets.rs crates/app/assets crates/app/src/workspace.rs` is empty.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Release binary size (`target/wt-55/release/wisp`) is reported with its delta from 19,114,864 B (d6a8bbb) and is ≤ 20,971,520 B.
- [ ] Hand check (orchestrator): with three tabs open and the second active, clicking × on the third closes it and the second stays active. Clicking × on the active tab activates its neighbour as ⌘W does. Clicking × on a tab that is sending shows no response anywhere afterwards. The URL input keeps working after a close (focus is not lost).

## Out of scope
- A confirmation prompt when closing a dirty tab. ⌘W closes without one today; changing that is a separate decision.
- Middle-click to close, tab reordering, or a close-others menu.
- Any change to `Workspace::close_tab`.
