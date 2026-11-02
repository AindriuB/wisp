# 53 — Fit the Cookies table and the failed-status line in narrow panes

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/ui/response/cookies.rs
- crates/app/src/ui/response/mod.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-53`.

## Goal
M3 (v1 hardening). Closes three PLAN.md residuals from tasks 46-49. First, the Cookies vertical scrollbar overlays the right edge of the Flags column. Second, the Cookies columns have no minimum width, so panes under about 245 px reduce short columns to "…". Third, the status-line error text is clipped at the pane edge. Also does the optional task-49 cosmetic cleanup in the same file.

## Context
- crates/app/src/ui/response/cookies.rs:59-71 — `WIDTHS` and `cell` (basis, grow, shrink, `min_w_0`, truncate).
- crates/app/src/ui/response/cookies.rs:73-123 — header row and the `cookies-rows` column with `overflow_y_scrollbar()`; `let widths = WIDTHS;` at :96.
- crates/app/src/ui/response/mod.rs:144-159 — the `Phase::Failed` row: `error_words` plus the full `message` in a muted div with no truncation or wrap.
- crates/app/src/ui/response/headers.rs:79-125 — the Headers table, for consistency only (not owned).

## Acceptance
- [ ] Every Cookies column has a minimum width (a `MIN_WIDTHS` constant or equivalent). When the pane is narrower than their sum, the header and rows scroll horizontally together, and no column renders narrower than its minimum.
- [ ] The rows' vertical scrollbar no longer covers Flags text. The header and rows reserve the same right gutter, so the columns stay aligned.
- [ ] `WIDTHS` sits beside `cell`, with the column-name list (task-49 cosmetic), and the `let widths = WIDTHS;` alias is gone.
- [ ] In the Failed status line, the message truncates with "…" on one line, and hovering the row shows the full message in a tooltip. `error_words` output is never truncated.
- [ ] A unit test covers any pure helper added (for example, the sum of the minimum widths).
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Release binary size (`target/wt-53/release/wisp`) is reported with its delta from 19,114,864 B (d6a8bbb) and is ≤ 20,971,520 B.
- [ ] Hand check (orchestrator): with a response carrying two Set-Cookie headers and the response pane at about 200 px and about 600 px wide, Flags is fully readable and not covered at 600 px; at 200 px the table scrolls sideways with no "…"-only short columns. A DNS failure on a long host name shows a one-line message with "…" and the full text on hover.

## Out of scope
- The Headers sub-tab layout and selection behaviour.
- Cookie parsing (`crates/app/src/cookies.rs`) and any cookie jar.
- Other status-line states (Completed, Sending, Cancelled), apart from sharing a row helper.
