# 43 — Add draggable vertical scrollbars to the response Headers and Cookies panes

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/response/headers.rs
- crates/app/src/ui/response/cookies.rs

## Goal
The response Headers and Cookies tabs scroll only by mouse wheel, with no
visible bar. Give each row list a visible vertical scrollbar whose thumb can
be dragged, using the same gpui-component 0.8.1 mechanism task 42 used for the
body view. Wheel scrolling, cell selection and ⌘C copy stay as they are.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Raw and Pretty scrollbar with End/Home jump keys`: the follow-up this closes (headers and cookies part).
- crates/app/src/ui/response/body.rs:559-568: the task 42 pattern. A `relative` wrapper with an id holds the scrolling content and calls `.vertical_scrollbar(&handle)` (`gpui_component::scroll::ScrollableElement`), with the same handle passed to `.track_scroll`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.8.1/src/scroll/scrollable.rs:16-62`: `vertical_scrollbar(&H)` adds a scrollbar layer child. `overflow_y_scrollbar()` wraps the element itself as the scroll area and overlays a bar. Its default id is the caller's source location, so pass `.id(...)` if you use it. Either is acceptable. The report says which one was used.
- crates/app/src/ui/response/headers.rs:40-45, 70-91: `HeadersPane` (one per request tab, built at `crates/app/src/ui/response/mod.rs:88`). Its root is a `flex_col size_full`. The `headers-rows` div at :87-91 is `overflow_y_scroll()` but has no `flex_1`/`min_h_0`, so it may not have a bounded height. A scrollbar needs one.
- crates/app/src/ui/response/cookies.rs:38-41, 63-110: `CookiesPane` (one per tab, mod.rs:89). The `cookies-rows` div at :91-98 already has `flex_1 min_h_0 overflow_y_scroll`, below a fixed column header row.
- crates/app/src/ui/response/mod.rs:191: the pane sits in `div().flex_1().min_h_0()`, which gives it a definite height.
- If you hold an explicit handle, use a `gpui::ScrollHandle` field on each pane struct, so each tab keeps its own scroll position.
- Binary budget: 18,998,304 B of 20,971,520 B (docs/plan/PLAN.md, "Raw scrollbar and jump keys (42)").

## Acceptance
- [ ] `git diff --stat main` lists only `crates/app/src/ui/response/headers.rs` and `crates/app/src/ui/response/cookies.rs`. `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] The headers row container and the cookies row container each render a gpui-component vertical scrollbar, through `vertical_scrollbar` with a tracked `ScrollHandle` or through `overflow_y_scrollbar()`. No plain `.overflow_y_scroll()` without a bar is left in either file (`rg -n 'overflow_y_scroll\(\)' crates/app/src/ui/response/headers.rs crates/app/src/ui/response/cookies.rs` shows no bare uses).
- [ ] The headers row container has a bounded height inside the pane (`flex_1` plus `min_h_0`, or equivalent), so 60 headers overflow the pane rather than the window.
- [ ] No key handling is added or changed. `on_key_down` and the action handlers in `headers.rs` are unchanged in the diff.
- [ ] Existing tests in both files pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-43 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-43 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-43 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-43 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 18,998,304 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check: a response with 60 headers. In the Headers tab a vertical scrollbar is visible, dragging the thumb to the bottom shows the last header, wheel scrolling still works, and clicking a cell then pressing ⌘C still copies it.
- [ ] Orchestrator hand check: a response with 60 `Set-Cookie` headers. In the Cookies tab the scrollbar is visible, the column header row stays fixed, dragging reaches the last cookie, and wheel scrolling still works.

## Out of scope
- A horizontal scrollbar for wide cookie columns, and column resizing.
- Switching headers to gpui-component `Table`, or keyboard cell navigation (see the PLAN decision under "Response viewer (33)").
- Clearing header selection on view-change notifies (an open PLAN optional item).
- Any other pane: body (`response/body.rs`), `response/mod.rs`, the sidebar, KvTable.
