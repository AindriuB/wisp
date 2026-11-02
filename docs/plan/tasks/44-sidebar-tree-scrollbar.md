# 44 — Add a draggable vertical scrollbar to the sidebar tree

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/sidebar.rs

## Goal
The sidebar collection tree scrolls only by mouse wheel, with no visible bar.
Give the tree a visible vertical scrollbar whose thumb can be dragged, using
the gpui-component 0.8.1 mechanism task 42 used. The header (environment
select, ⌘K button) and the pending rename/move panel stay fixed above it.
Wheel scrolling, the context menu and keyboard handling are unchanged.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Raw and Pretty scrollbar with End/Home jump keys`: the follow-up this closes (sidebar part).
- crates/app/src/ui/response/body.rs:559-568: the task 42 pattern. A `relative` wrapper holds the scrolling content and calls `.vertical_scrollbar(&handle)`, with the same handle passed to `.track_scroll`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.8.1/src/scroll/scrollable.rs:16-62`: `vertical_scrollbar` and `overflow_y_scrollbar()` (whose default id is the caller location, so pass `.id(...)`). Either is acceptable. The report says which.
- crates/app/src/ui/sidebar.rs:211-220: `Sidebar` struct. A `gpui::ScrollHandle` field fits here if you hold one.
- crates/app/src/ui/sidebar.rs:819: `div().id("sidebar-tree").flex_1().overflow_y_scroll().py_1()`. It has no `min_h_0`, so in the `flex_col` root (:862-872) it may grow to its content instead of being bounded.
- crates/app/src/ui/sidebar.rs:841-859: `body.context_menu(...)` wraps the tree after it is built. The scrollbar must be attached before or inside that wrap so right-click on the tree still opens "New request" / "New folder".
- crates/app/src/ui/sidebar.rs:862-866: the root carries `track_focus` and `on_key_down`. Leave both unchanged.
- Binary budget: 18,998,304 B of 20,971,520 B.

## Acceptance
- [ ] `git diff --stat main` lists only `crates/app/src/ui/sidebar.rs`. `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] The tree container renders a gpui-component vertical scrollbar, through `vertical_scrollbar` with a tracked `ScrollHandle` or through `overflow_y_scrollbar()`, and has a bounded height (`flex_1` plus `min_h_0` or equivalent). `rg -n 'overflow_y_scroll\(\)' crates/app/src/ui/sidebar.rs` shows no bare use without a bar.
- [ ] `on_key_down`, `track_focus` and the `context_menu` items are unchanged in the diff apart from moving with the restructured element.
- [ ] Existing `sidebar.rs` tests pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-44 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-44 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-44 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-44 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 18,998,304 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check: open a collection with 60 requests. A vertical scrollbar is visible beside the tree, and the environment select stays fixed at the top. Dragging the thumb to the bottom shows the last request, and wheel scrolling still works. Right-click on empty tree space still shows "New request" and "New folder".

## Out of scope
- Scroll-into-view of the selected or newly created node, and expanding a collapsed parent on create (an open PLAN optional item).
- The "Move … to…" target list in `render_pending` (sidebar.rs:760-790). It stays unscrolled even with many folders. Record it in the report if it overflows.
- Keyboard navigation changes, virtualising the tree, and any file other than `sidebar.rs`.
