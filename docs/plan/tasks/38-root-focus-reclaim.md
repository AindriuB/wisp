# 38 — Reclaim root focus when the focused element unmounts

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/root.rs

## Goal
When the focused element is unmounted (⌘T opens a new tab, switching tabs, OK or Cancel on the sidebar new/rename panel, the palette restoring focus to an input that is no longer rendered), focus currently lands on nothing and every root shortcut (⌘W, ⌘O, ⌘K, ⌘E and the rest) stops working until the user clicks. The root view must take focus back onto its own handle whenever focus would otherwise be outside the rendered window tree, without stealing focus from a live input, the palette or a sidebar panel.

## Context
- crates/app/src/ui/root.rs:79-80 — the root `FocusHandle` is created and focused once in `WorkspaceView::new`, never again
- crates/app/src/ui/root.rs:263-350 — `render` reads `w.active` and tracks `self.focus` on the root div; the place to detect an active-tab change or focus outside the tree (for example `self.focus.contains_focused(window, cx)` or `window.focused(cx)` returning `None`, applied through `window.defer`/`on_next_frame` so it runs against the frame that no longer contains the old element)
- crates/app/src/ui/sidebar.rs:392-400 — the panel input takes focus; when `pending` is cleared the input entity drops (root must cover this; do not edit the sidebar)
- crates/app/src/ui/palette.rs:85-95 — palette restores `prev_focus` on dismiss, which may name an unrendered input
- HISTORY 2026-11-02 "Command palette and environment switcher" — bindings dispatch before key-down listeners in gpui-pre 0.3.10; an overlay that takes focus must restore it
- No gpui test support is enabled in `wisp-app`; put the decision in a GPUI-free helper and unit-test that
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-38`

## Acceptance
- [ ] `root.rs` has a GPUI-free decision helper (for example `fn should_reclaim(prev_active: Option<usize>, active: Option<usize>, focus_in_tree: bool) -> bool`) with unit tests covering: active tab changed, focus already inside the tree with no tab change (no reclaim), focus on nothing (reclaim), and no tabs at all.
- [ ] The reclaim never fires while a rendered input inside the window holds focus: typing in the URL input, a KvTable cell, the open palette, or the sidebar rename input keeps focus there across re-renders (checked by the hand check below).
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-38 cargo test -p wisp-app` passes, and `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` (same target dir) are clean.
- [ ] Hand check (orchestrator, window frontmost, temp copy of `crates/storage/tests/fixtures/my-api`): ⌘T then ⌘W closes the new tab with no click; with two tabs, switch tabs by clicking the tab bar then ⌘K opens the palette; sidebar Rename then OK (or Enter) then ⌘O opens the open-collection prompt; ⌘K, Escape, then ⌘W works; focusing the URL input and typing still types.
- [ ] `git diff --stat main` lists only `crates/app/src/ui/root.rs`.

## Out of scope
- Any change to `sidebar.rs`, `palette.rs`, `url_bar.rs`, `workspace.rs` or `actions.rs`. If a root-only fix cannot cover one of the listed cases, stop and report which case and why.
- Moving focus into the new tab's URL input on ⌘T (a feature, not this fix).
- The Escape-binding question in PLAN (Escape inside the URL input during a send) — task 41 checks it.
