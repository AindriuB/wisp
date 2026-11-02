# 42 — Add a draggable vertical scrollbar to the response body view

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/response/body.rs

## Goal
Long response bodies in the Raw view can only be moved through with the mouse
wheel, about 1.6 lines per tick. That is why the 4 MB display-cap line (about
95,000 lines down in a 250,000-line body) could not be reached in the task 41
hand check. Give the body list, Raw and Pretty alike, a visible scrollbar
whose thumb can be dragged, and make dragging to the end keep firing the
existing Raw prefetch so pages keep loading until the cap line appears.

## Context
- docs/plan/PLAN.md:77-79: "Response viewer (33)", the open P3 this closes.
- docs/plan/HISTORY.md, heading `## 2026-11-02 — M2 verification gaps hand-checked`: the hand check that left the cap line unreached.
- crates/app/src/ui/response/body.rs:375-423: `lines_view`. One `v_virtual_list` serves both Raw (`"raw-lines"`) and Pretty (`"pretty-lines"`) and is already wired with `.track_scroll(&self.scroll)`, a `gpui_component::VirtualListScrollHandle` (body.rs:124, 149). Pretty therefore has the same gap, and a fix in `lines_view` covers both views.
- crates/app/src/ui/response/body.rs:545-559: both views are wrapped in `div().flex_1()`, and `.min_h_0()` is applied at :559. A scrollbar overlay needs that wrapper to have a bounded height.
- crates/app/src/ui/response/body.rs:40-53, 278-324: `should_prefetch` (`PREFETCH_LINES = 200`, :27) runs inside the list's render closure using `range.end`. `load_more` lets one page (`PAGE_SIZE` 64 KiB, `crates/app/src/pager.rs:3`) be in flight at a time and stops at `RAW_DISPLAY_CAP` 4 MB (pager.rs:4). The tests at body.rs:656-700 cover it.
- gpui-component 0.8.1 (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.8.1/`):
  - `src/virtual_list.rs:9-20` has a test asserting `VirtualListScrollHandle: ScrollbarHandle`.
  - `src/scroll/scrollable.rs:16-35`: `ScrollableElement::vertical_scrollbar(&handle)` adds a scrollbar layer as a child.
  - `src/list/list.rs:621` shows the alternative, `Scrollbar::vertical(&handle).viewport_from_layout()` as a child.
  - The implementer should pick whichever applies to a `v_virtual_list` and say which. Both are in the crate already, so no new dependency is needed.
- Other scrolling panes, recorded here and **not** fixed in this task (none are in owned files):
  - `crates/app/src/ui/response/headers.rs:87-91` and `crates/app/src/ui/response/cookies.rs:98` use plain `.overflow_y_scroll()` with no scrollbar.
  - `crates/app/src/ui/kv.rs:366` (KvTable above `VIRTUALISE_ABOVE` rows) is a `v_virtual_list` with no scroll handle and no scrollbar.
  - `crates/app/src/ui/sidebar.rs:819` (sidebar tree) is `.overflow_y_scroll()` with no scrollbar.
  - All have the same gap. They are candidates for a follow-up task.
- **Design question, do not decide here:** the list length is `raw.lines.len() + 2` and grows by roughly 1,500 lines for each 64 KiB page loaded. Dragging the thumb to the bottom reaches the end of what is *loaded*. Prefetch then adds a page, the list grows, and the visible range is no longer within 200 lines of the end, so prefetch stops until the user drags again. Reaching a 4 MB cap could take about 64 drags. If the hand check finds that unacceptable, a "Jump to end" control or ⌘↓ (keep loading while pinned to the bottom) is the candidate fix. Report the observed number of drags. Do not build either here unless the scrollbar alone provably cannot reach the cap line.
- docs/conventions.md#acceptance-criteria-discipline: the criteria below freeze when the first attempt starts.
- Binary budget, from docs/plan/PLAN.md:26: 18,965,216 B of 20,971,520 B, about 2 MB headroom (task 35, commit add9842).

## Acceptance
- [ ] Only `crates/app/src/ui/response/body.rs` is modified (`git diff --stat main` shows one file). `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] `lines_view` attaches a vertical scrollbar driven by `self.scroll`, the same `VirtualListScrollHandle` passed to `.track_scroll`, so the Raw and Pretty lists both render it.
- [ ] Any new scroll or prefetch decision logic, such as a predicate for when to load the next page while pinned to the bottom, is a free function with a GPUI-free `#[test]` in `body.rs`'s `mod tests`. If no new logic is added, the report says so.
- [ ] The existing test `raw_view_prefetches_even_when_pretty_is_ready` and the other `body.rs` tests pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo test --workspace` passes. The report gives the pass count and the commit it was measured on.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo build --release -p wisp-app`, then `RENDERER=hardware scripts/measure/measure.sh /Users/Andrew/workspace/wisp/target/wt-42/release/wisp <out.json>`. The report gives the `status`, the binary size in bytes and the delta against 18,965,216 B. The size stays at or below 20,971,520 B.
- [ ] Orchestrator hand check: load a 250,000-line text response in Raw. The scrollbar is visible. Dragging the thumb to the end repeatedly makes the line "Showing the first 4 MB — Save to read the rest" appear. The report gives the last body line number shown above it and roughly how many drags it took.
- [ ] Orchestrator hand check: a pretty-printed JSON body in Pretty shows the scrollbar and can be dragged end to end.

## Out of scope
- Scrollbars for headers, cookies, KvTable or the sidebar tree (`headers.rs`, `cookies.rs`, `kv.rs`, `sidebar.rs`). File a follow-up task.
- A "Jump to end" button or ⌘↓ shortcut, unless the scrollbar provably cannot reach the cap line. In that case, stop and report rather than adding a keybinding outside owned files.
- Changing `PAGE_SIZE`, `RAW_DISPLAY_CAP` or `PREFETCH_LINES`, or the paging code in `crates/app/src/pager.rs`.
- A horizontal scrollbar or line wrapping for long lines.
