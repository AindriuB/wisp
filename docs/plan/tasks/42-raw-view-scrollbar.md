# 42 — Add a scrollbar and End/Home keys to the response body view

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
existing Raw prefetch so pages keep loading until the cap line appears. Per the 2026-11-02 amendment below, End and ⌘↓ jump to the end (loading up to the cap in Raw), and Home and ⌘↑ jump to the top.

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
- Why the scrollbar alone is not enough: the list length is `raw.lines.len() + 2` and grows by roughly 1,500 lines for each 64 KiB page loaded. Dragging the thumb to the bottom reaches the end of what is *loaded*. Prefetch then adds a page, the list grows, and the visible range is no longer within 200 lines of the end, so prefetch stops until the user drags again. Reaching the 4 MB cap could take about 64 drags.
- **Amendment (user decision, 2026-11-02): scrollbar plus keys.** This replaces the earlier open design question about "Jump to end".
  - In the body view, End and ⌘↓ load pages up to the 4 MB display cap (Raw) or the last line (Pretty), then scroll to the bottom.
  - Home and ⌘↑ scroll to the top.
  - The keys are handled inside body.rs only. Use a `FocusHandle` on the body list with `.track_focus` plus `.on_key_down`, or action listeners on that element. Do not add new bindings in `crates/app/src/actions.rs` or anywhere else outside `Owns`.
  - Clicking the body list focuses it.
  - Because the handler is on the body list's own element, keys only reach it while the list, or something inside it, has focus. They therefore never fire while the URL input or another input has focus.
- Patterns for the keys:
  - `crates/app/src/ui/sidebar.rs:334-351, 861-866` (`key_context` + `track_focus` + `on_key_down` matching `ev.keystroke.key`) and `crates/app/src/ui/response/headers.rs:122-123`.
  - Scrolling: `VirtualListScrollHandle::scroll_to_item` / `scroll_to_bottom` (`gpui-base-0.8.1/src/virtual_list.rs:107-126`). These are deferred to the next layout, so call `scroll_to_bottom` after the item count has grown.
  - "Load until cap then jump" needs `load_more` to be re-triggered each time a page lands while a jump is pending. A flag such as `pending_jump_end` works, cleared once `raw.done` or `raw.capped` is set, at which point it scrolls to the bottom. The page-landing callback is at body.rs:299-321 and `finish_raw` at :326-337. The decision itself, what to do next given loading/done/capped/pending state, should be a free function so it can be tested without GPUI.
  - Task 38's focus reclaim (`crates/app/src/ui/root.rs:60`, `should_reclaim`) returns focus to the root when a focused element unmounts. Switching between Raw and Pretty remounts the list (different ids), so the body focus handle should live on `BodyPane` (body.rs:115), not be recreated per view.
- docs/conventions.md#acceptance-criteria-discipline: the criteria below freeze when the first attempt starts.
- Binary budget, from docs/plan/PLAN.md:26: 18,965,216 B of 20,971,520 B, about 2 MB headroom (task 35, commit add9842).

## Acceptance
- [ ] Only `crates/app/src/ui/response/body.rs` is modified (`git diff --stat main` shows one file). `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] `lines_view` attaches a vertical scrollbar driven by `self.scroll`, the same `VirtualListScrollHandle` passed to `.track_scroll`, so the Raw and Pretty lists both render it.
- [ ] The body list has a `FocusHandle` owned by the body view struct. Clicking the list focuses it. End and ⌘↓, and Home and ⌘↑, are handled by `on_key_down` or action listeners on that element in body.rs. `git diff main -- crates/app/src/actions.rs` is empty.
- [ ] The "load until cap then jump" decision is a free function in body.rs. Its GPUI-free `#[test]` covers each of these:
  - (a) a jump pending, not loading, not done: load the next page;
  - (b) a jump pending, a page in flight: wait;
  - (c) a jump pending, and `done` or `capped`: scroll to the bottom and clear the pending flag;
  - (d) Pretty on screen: scroll to the bottom at once, with no raw load;
  - (e) no jump pending: do nothing.
- [ ] Any other new scroll or prefetch decision logic is likewise a free function with a GPUI-free `#[test]` in `body.rs`'s `mod tests`.
- [ ] The existing test `raw_view_prefetches_even_when_pretty_is_ready` and the other `body.rs` tests pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo test --workspace` passes. The report gives the pass count and the commit it was measured on.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-42 cargo build --release -p wisp-app`, then `RENDERER=hardware scripts/measure/measure.sh /Users/Andrew/workspace/wisp/target/wt-42/release/wisp <out.json>`. The report gives the `status`, the binary size in bytes and the delta against 18,965,216 B. The size stays at or below 20,971,520 B.
- [ ] Orchestrator hand check: load a 250,000-line text response in Raw. The scrollbar is visible. Dragging the thumb to the end repeatedly makes the line "Showing the first 4 MB — Save to read the rest" appear. The report gives the last body line number shown above it and roughly how many drags it took.
- [ ] Orchestrator hand check: a pretty-printed JSON body in Pretty shows the scrollbar and can be dragged end to end.
- [ ] Orchestrator hand check, 250,000-line body in Raw:
  - Click the body, then press End once: the cap line appears with no further input. Repeat with ⌘↓ on a fresh response.
  - Press Home: line 1 is at the top. Repeat with ⌘↑.
- [ ] Orchestrator hand check: in Pretty, End and ⌘↓ show the last line, and Home and ⌘↑ show line 1.
- [ ] Orchestrator hand check: with the URL input focused, and separately with another input focused (for example a KvTable cell), End and Home act on the input's text cursor. The body list does not scroll or load.

## Out of scope
- Scrollbars for headers, cookies, KvTable or the sidebar tree (`headers.rs`, `cookies.rs`, `kv.rs`, `sidebar.rs`). File a follow-up task.
- A visible "Jump to end" button. The keys are the affordance.
- Global keybindings or changes to `crates/app/src/actions.rs`, `root.rs` or the keymap. If on-element handling cannot work, stop and report.
- PageUp/PageDown or other navigation keys.
- Changing `PAGE_SIZE`, `RAW_DISPLAY_CAP` or `PREFETCH_LINES`, or the paging code in `crates/app/src/pager.rs`.
- A horizontal scrollbar or line wrapping for long lines.
