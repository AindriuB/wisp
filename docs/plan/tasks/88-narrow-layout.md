# 88 — Fit the sidebar, panes and Cookies scrollbar to narrow windows

**Repo:** .
**Depends on:** 83 (both edit crates/app/src/ui/root.rs)
**Owns:**
- crates/app/src/ui/root.rs (the layout block, about :340-385, and new tests)
- crates/app/src/ui/response/cookies.rs
- docs/plan/tasks/88-narrow-layout.md (the `## Notes` section only)

Branch `task/88-narrow-layout`, worktree `.worktrees/wisp/88-narrow-layout`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-88`. Start only after
83 is merged, then `git fetch origin && git rebase origin/main`. Commit on the
branch; do not push.

## Goal
Narrow windows still have three layout faults with clear fixes (PLAN.md,
**Residuals (61)** and **Residuals (53)**). At the 720 px minimum width the
sidebar can take 480 px. The request and response panes can be dragged down to
100 px. Below 584 px, the Cookies rows' vertical scrollbar sits off-screen until
you scroll sideways. Fix those three. Two leftovers stay accepted: the cut
"Expires / Max-Age" text at 600 px and the macOS-only Flags measurement. The
request-header table squeezed by many inherited headers is left too: fixing it
would change the spec's "inherited list keeps natural height" rule, so it waits
for the user.

## Context
- crates/app/src/ui/root.rs:348-356 — the request/response `h_resizable`/`v_resizable` group with plain `resizable_panel()`s (default minimum `PANEL_MIN_SIZE`, gpui-base-0.8.1/src/resizable/panel.rs:265; `size_range` :290).
- crates/app/src/ui/root.rs:377-384 — the sidebar panel, `size(px(240.))`, `size_range(px(120.)..px(480.))`.
- crates/app/src/main.rs:37-38 — the 720×480 minimum window (read only).
- crates/app/src/ui/response/cookies.rs:75-86 (`MIN_WIDTHS`, `GUTTER`, `min_table_width`), :143-160 (the vertical `overflow_y_scrollbar` sits inside the `cookies-scroll` horizontal scroller).
- PLAN.md Residuals (61): **do not** re-add `overflow_hidden` to the `resizable_panel()` calls (it clips the splitter's grab band).

## Contract
- Sidebar maximum = `clamp(window_width − 480, 120, 480)` logical px, recomputed every render from the window's viewport width, as a pure `fn sidebar_max(window_width: f32) -> f32`. At 720 px it is 240. The minimum stays 120 and the default stays 240.
- Request and response panels: minimum 240 px each in the horizontal split, 140 px each in the vertical split. Use the named constants `MIN_PANE_WIDTH` and `MIN_PANE_HEIGHT`.
- Cookies: at any pane width, the rows' vertical scrollbar is in view at the pane's right edge without scrolling sideways. The header stays aligned with its columns when scrolled sideways.

## Acceptance
- [ ] AC1 Tests `sidebar_max_is_240_at_the_minimum_window`, `sidebar_max_caps_at_480_on_wide_windows` and `sidebar_max_never_drops_below_120` pass.
- [ ] AC2 The sidebar panel's `size_range` uses `sidebar_max(viewport width)`. The two split panels use `MIN_PANE_WIDTH` and `MIN_PANE_HEIGHT` for the active axis. No `overflow_hidden` is added to any `resizable_panel()`.
- [ ] AC3 In cookies.rs, the vertical scrollbar is no longer a child of the horizontal scroll container. A test `min_table_width_still_covers_columns_and_gutter` pins `min_table_width()` to the current 584 px, or to the new value if the gutter moves, with the reason in a comment. The existing cookies tests pass.
- [ ] AC4 `## Notes` gives the user's hand-check steps at a 720×480 window on macOS: drag the sidebar wide (stops at 240); drag each split to its minimum (240 wide, or 140 high in vertical layout); open Cookies on a response with many cookies in a pane under 584 px (the vertical scrollbar is visible). It also lists the leftovers not fixed (Goal) and why.
- [ ] AC5 If the implementer launches the app to look, it runs a copy bundled with the id `io.github.aindriub.wisp.test88` (or as a bare binary), stops only the pid it started, and never uses pkill or killall.
- [ ] AC6 `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean; `cargo test -p wisp-app` passes.

## Checks
- `layout-unit` command — AC1, AC3. cargo test does not write JUnit XML here, so the command asserts the pass count itself (≥ 4).
  run: `cargo test -p wisp-app -- sidebar_max_ min_table_width_still 2>&1 | grep -E '^test result: ok\. ([4-9]|[1-9][0-9]+) passed'`
- `app-tests` command — AC6
  run: `cargo test -p wisp-app`
- `lint` command — AC6
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `layout` read — AC2, AC3, AC4, AC5
  how: the diff wires `sidebar_max` and the pane constants, moves the Cookies scrollbar out of the x-scroller, adds no `overflow_hidden` to a panel, and Notes has the steps and the leftovers.

## Out of scope
- The inherited-headers squeeze (headers.rs; needs a user decision on the spec).
- The "Expires / Max-Age" cut at 600 px, the Flags font measurement, and changing the 720×480 minimum.
- Prompt and close-tab code in root.rs (task 83, already merged).

## Notes
