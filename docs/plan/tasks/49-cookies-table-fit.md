# 49 — Fit the response Cookies table to the pane width

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/response/cookies.rs

## Goal
The Cookies table uses six fixed-width columns (about 940 px plus gaps), so in
a side-by-side response pane Path, Expires and Flags and the vertical
scrollbar are cut off. Make the table fit the pane, with flexible columns that
truncate, or let it scroll horizontally, while the vertical scrollbar stays
visible at the pane's right edge.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Scrollbars on the sidebar, response Headers and Cookies, and KvTable`: where the defect was found.
- docs/plan/PLAN.md, "Pane-scrollbar defects", "P3, Cookies table width".
- crates/app/src/ui/response/cookies.rs:61-63: `cell(text, w)`, a fixed `w(px(w)) flex_none overflow_hidden` div.
- crates/app/src/ui/response/cookies.rs:65-113: `Render for CookiesPane`. `widths = [140., 200., 140., 100., 160., 200.]` (:84) are shared by the header row (:85-88) and the data rows (:96-108). The rows container `cookies-rows` is `flex_1 min_h_0` with `overflow_y_scrollbar()` (:112). The pane root is `flex_col size_full p_2`.
- crates/app/src/ui/response/headers.rs: the Headers pane, for comparison only (not owned).
- If you choose horizontal scrolling, gpui-component 0.8.1 `scroll/scrollable.rs` has the scrollbar helpers; the header row must scroll with the rows, and the vertical bar must not scroll out of view.
- Binary budget: 19,014,912 B of 20,971,520 B.

## Acceptance
- [ ] `git diff --stat main` lists only `crates/app/src/ui/response/cookies.rs`. `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] The report says which approach was taken (flexible columns with truncation, or horizontal scroll).
- [ ] Header cells and row cells share one column definition, so each header stays aligned with its column at any pane width.
- [ ] Text that does not fit a cell is clipped or ends in an ellipsis inside that cell and never paints into the next column.
- [ ] `parse_set_cookies`, `expiry`, `flags` and their tests are unchanged in the diff, and the vertical scrollbar wiring on `cookies-rows` is kept.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-49 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-49 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-49 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-49 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 19,014,912 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check: side-by-side layout with the response pane about 450 px wide, a response with 60 `Set-Cookie` headers including a 200-character value and all of Domain, Path, Max-Age, Secure, HttpOnly and SameSite. All six column headers are visible or reachable by horizontal scroll, the vertical scrollbar is visible at the right edge, and dragging it reaches the last cookie.
- [ ] Orchestrator hand check: stacked layout at full window width, the table reads as on main and the header row stays fixed while rows scroll.

## Out of scope
- Column resizing, sorting, cell selection or copy in the Cookies pane.
- The response Headers pane, body view and `response/mod.rs`.
- Cookie parsing changes.
