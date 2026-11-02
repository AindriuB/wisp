# 48 — Clip the URL highlight strip to the request pane

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/url_bar.rs
- crates/app/src/ui/root.rs (the request-side wrapper div in the split content only, about :316-321)

## Goal
The variable-highlight strip under the URL input is a no-wrap row of segments
with no clipping, so a long URL draws across the splitter and over the
response pane in side-by-side layout. Keep it inside the request pane, either
clipped or truncated with an ellipsis.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Scrollbars on the sidebar, response Headers and Cookies, and KvTable`: where the defect was found.
- docs/plan/PLAN.md, "Pane-scrollbar defects", "P3, URL highlight strip".
- crates/app/src/ui/url_bar.rs:315-380: `Render for UrlBar`. The strip (:318) is `flex flex_row px_2 text_sm` with one `whitespace_nowrap` div per segment (:331-334), each with an id and an optional tooltip. The outer root is a `flex_col` with no width bound.
- crates/app/src/ui/root.rs:316-321: the request side is `div().size_full().flex().flex_col().child(v.url_bar.clone())...`, inside a `resizable_panel` (:327-330). Neither clips. Edit root.rs only if the strip cannot be bounded from inside `url_bar.rs`; the report says which was needed.
- Segment tooltips (resolved variable values, " (uses a secret)" hover) must keep working on segments that remain visible.

## Acceptance
- [ ] `git diff --stat main` lists only `crates/app/src/ui/url_bar.rs` and, if needed, `crates/app/src/ui/root.rs`. Any `root.rs` hunk is limited to the request-side wrapper div. `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] The strip container has a bounded width (`w_full`/`min_w_0` or equivalent) and `overflow_hidden`, or ends in an ellipsis, so its painted bounds never exceed the request pane.
- [ ] Segment colours, ids, tooltips, and the resync and problem logic in `url_bar.rs` are unchanged in the diff. Existing `url_bar.rs` tests pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-48 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-48 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-48 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-48 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 19,014,912 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check: side-by-side layout, a URL with a query string of at least 300 characters including a `{{var}}`. The strip stops at the splitter and nothing draws over the response pane. Dragging the splitter re-clips the strip. Hovering a visible variable segment still shows its tooltip.
- [ ] Orchestrator hand check: stacked layout with a short URL, the strip looks as on main.

## Out of scope
- Wrapping or horizontally scrolling the strip, and the resolve-problem list under it.
- The URL input, method select and Send button, and URL resync logic.
- The response pane and any other part of `root.rs` (split, tabs, focus reclaim).
