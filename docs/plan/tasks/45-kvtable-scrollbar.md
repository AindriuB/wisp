# 45 — Give KvTable a bounded height and a draggable vertical scrollbar

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/kv.rs
- crates/app/src/ui/request/mod.rs
- crates/app/src/ui/request/params.rs
- crates/app/src/ui/request/headers.rs
- crates/app/src/ui/request/body.rs (render layout of the Form branch only)

## Goal
KvTable (Params, request Headers, Form body) has no visible scrollbar. Under
50 rows it is a plain column with no scroll container at all. Above 50 rows it
is a `v_virtual_list` with no scroll handle, and it only virtualises if its
container gives it a definite height. Make the request sub-tab area give the
table a bounded height, and make KvTable scroll itself in both branches with a
visible, draggable vertical scrollbar, using the gpui-component 0.8.1
mechanism task 42 used.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Raw and Pretty scrollbar with End/Home jump keys`: the follow-up this closes (KvTable part).
- crates/app/src/ui/response/body.rs:522-568: the task 42 pattern for a `v_virtual_list`. `.track_scroll(&self.scroll)` with a `gpui_component::VirtualListScrollHandle`, inside a `relative size_full` wrapper with an id that calls `.vertical_scrollbar(&self.scroll)`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.8.1/src/scroll/scrollable.rs:16-62`: `vertical_scrollbar`, and `overflow_y_scrollbar()` for a plain div (default id is the caller location, so pass `.id(...)`).
- crates/app/src/ui/kv.rs:15-17: `VIRTUALISE_ABOVE = 50`, `ROW_HEIGHT = 30`. :138-143: the `KvTable` struct, where the handles go (one `gpui::ScrollHandle` for the plain branch and one `VirtualListScrollHandle` for the virtual branch, or an equivalent). Each request tab has three `KvTable` entities, so per-entity fields keep the positions separate.
- crates/app/src/ui/kv.rs:361-383: `Render for KvTable`. The virtual branch is `div().size_full().child(v_virtual_list(...))` with no `track_scroll`. The plain branch is `div().flex().flex_col().w_full()` with no overflow.
- crates/app/src/ui/request/mod.rs:107-115: `RequestPane` renders `div().size_full().child(strip).child(editor)`. Div defaults to `Display::Block` (gpui-pre 0.3.10 `src/style.rs:774`), so the editor slot has content height, not a bounded one. It needs to become a `flex_col` with the editor slot `flex_1 min_h_0`.
- crates/app/src/ui/root.rs:321: the request pane sits in `div().flex_1().min_h_0()`.
- crates/app/src/ui/request/params.rs:131-135: `gpui::div().child(self.table.clone())`. The wrapper must pass the bounded height through (`size_full` or `flex_1 min_h_0`).
- crates/app/src/ui/request/headers.rs:163-191: the table is followed by an "Inherited" list. The table should take the remaining height (`flex_1 min_h_0`) and the inherited list should keep its natural height below it.
- crates/app/src/ui/request/body.rs:633-648: `BodyEditor` is a `flex_col gap_2 p_2` with the kind select on top. For `BodyKind::Form` the `self.form` table needs the remaining height. Other branches render as before.
- docs/plan/PLAN.md, "KvTable (29)": the accepted residual that Tab cannot reach unpainted rows above 50. It stays accepted here.
- Found, not included (each needs its own scroll container, which is not the same change): the Path sub-tab (`request/path.rs:138`) with many path params, the Multipart rows (`request/body.rs:536-600`), Auth and Settings. The text body `Editor` (`request/body.rs:533`, fixed `h(px(260.))`) is a gpui-component `Editor`, which brings its own scrollbar through `EditorScrollbarContext` in gpui-base 0.8.1. The ⌘K/⌘E palette uses gpui-component `Command`, which owns its own list.
- Binary budget: 18,998,304 B of 20,971,520 B.

## Acceptance
- [ ] `git diff --stat main` lists only files in the Owns set, and the `request/body.rs` hunks touch only `Render for BodyEditor` (form-branch layout). `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] `KvTable::render` attaches a gpui-component vertical scrollbar in both branches. The virtual branch calls `.track_scroll(&h)` on the `v_virtual_list` with a `VirtualListScrollHandle` field and `.vertical_scrollbar(&h)` on its wrapper. The plain branch is a scroll container with an id, tracked by a `ScrollHandle` field with a bar (or `overflow_y_scrollbar()` with an explicit id).
- [ ] `RequestPane::render` lays out the strip and the editor slot as a `flex_col`, with the slot `flex_1` plus `min_h_0`. The slot does not add `overflow_hidden`, so Path, Auth, Settings, Multipart and File render as they do on main.
- [ ] `VIRTUALISE_ABOVE`, `ROW_HEIGHT`, row rendering, Tab order (`tab_stop(false)` on checkbox and delete) and the KvTable model and events are unchanged in the diff.
- [ ] No key handling is added or changed in any owned file.
- [ ] Existing tests in `kv.rs`, `params.rs`, `request/headers.rs` and `request/body.rs` pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-45 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-45 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-45 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-45 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 18,998,304 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check, virtual branch: a request with 80 query params. The Params sub-tab shows a vertical scrollbar, dragging the thumb to the bottom shows param 80 and the trailing empty row, and wheel scrolling still works. Typing in a visible row still updates the URL.
- [ ] Orchestrator hand check, plain branch: a request with 40 params in a window short enough to overflow. The scrollbar is visible, dragging reaches the last row, and the table no longer draws over the response pane.
- [ ] Orchestrator hand check: the Headers sub-tab with 60 request headers and at least one inherited header. The table scrolls with a bar and the "Inherited" list stays visible below it. A Form body with 60 fields scrolls with a bar under the kind select.

## Out of scope
- Scroll-to-focus for Tab past the viewport (the accepted KvTable residual), and any key handling.
- Scroll containers for Path, Auth, Settings, Multipart or File (listed in Context). Record in the report whether any of them draws over the response pane after the layout change.
- The O(n) `locate` in `kv.rs`, the virtualisation threshold, and `request/url_bar` sync logic.
- Response panes and the sidebar (tasks 43 and 44).
