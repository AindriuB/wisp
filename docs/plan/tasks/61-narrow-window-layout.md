# 61 — Set a minimum window size and make the URL row and Done status line fit narrow panes

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/main.rs
- crates/app/src/ui/root.rs
- crates/app/src/ui/url_bar.rs
- crates/app/src/ui/response/mod.rs

Runs alone in its wave. Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-61`.

## Goal
At a window about 360 px wide the layout breaks: the request pane's URL row (method, URL input, Send) spills into the response pane and Send draws over the status text, and the completed-status line is clipped instead of truncated. User decision (2026-11-03): give the window a minimum size of 720×480 px, and make the URL row clip and the Done status line truncate with "…" and a full-text tooltip, so nothing overlaps at the minimum size even with a splitter at its extremes. The request pane gets no horizontal scroll.

## Context
- crates/app/src/main.rs:55 — `cx.open_window(WindowOptions::default(), …)`; the only window. gpui-pre 0.3.10 `WindowOptions` has `window_min_size: Option<Size<Pixels>>` (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-pre-0.3.10/src/platform.rs:2637`).
- crates/app/src/ui/root.rs:333-349 — request column (`url_bar` over `request`) and response, in `h_resizable("split-h")` / `v_resizable("split-v")` with default `resizable_panel()`s. The default panel range is `PANEL_MIN_SIZE` (100 px) to `Pixels::MAX` (gpui-component `crates/ui/src/resizable/mod.rs:12`, `panel.rs:230,253`). Nothing on these panels or their child divs sets `min_w_0` or `overflow_hidden`, so content wider than a panel paints over its neighbour.
- crates/app/src/ui/root.rs:368-375 — the sidebar panel: 240 px, range 120-480 px. At a 720 px window and a 480 px sidebar, the main area is about 240 px. The request and response panels can each be dragged down to 100 px.
- crates/app/src/ui/url_bar.rs:366-385 — the URL row: method `Select` in a fixed `w(px(110.))` div, `Input` in `div().flex_1()` without `min_w_0`, then the Send/Cancel `Button`. Task 48 clipped only the highlight strip (`url_bar.rs:317-324`).
- crates/app/src/ui/response/mod.rs:116-143 — the `Phase::Done` branch: a `row()` holding the coloured `status_text(…)` and `"· {total} · {size}"`, with an optional TTFB tooltip. It has no `min_w_0`, `overflow_hidden` or `truncate`.
- crates/app/src/ui/response/mod.rs:144-170 — the `Phase::Failed` branch (task 53): the pattern to follow (`min_w_0` + `overflow_hidden` on the row, `truncate` on the flexible child, a tooltip with the full text).
- docs/architecture.md#budgets — binary ≤ 20,971,520 B, idle RSS ≤ 120 MiB (122,880 KiB), after one request ≤ 128 MiB (131,072 KiB), macOS arm64 hardware renderer.
- Task 60 figures (commit 0a8b5f8): binary 19,181,072 B, idle 68.8 MiB, after one request 73.9 MiB; 367 tests pass. 8bd778c changed only docs, so these still hold on main.
- scripts/measure/measure.sh — `RENDERER=hardware scripts/measure/measure.sh <binary> <output.json>`; it drives the `WISP_AUTOSEND_URL` hook.

## Acceptance
- [ ] `main.rs` passes `window_min_size: Some(size(px(720.), px(480.)))` to `open_window`, from a named constant or a small `fn window_options()`. A unit test in `main.rs` asserts the min size is 720×480. Hand check: the window cannot be dragged smaller than 720×480 px.
- [ ] URL row (`url_bar.rs`): the row has `min_w_0()` and `overflow_hidden()`. The method select and the Send/Cancel button are `flex_none()`. The input's wrapper is `flex_1().min_w_0()`. In `root.rs`, the request column and response wrappers (and the panels' direct children) have `min_w_0()` and `overflow_hidden()`, so no request-pane content can paint outside its panel. Hand check: at a 720 px window, with the sidebar at 120 px and at 480 px and the request/response splitter dragged fully left and fully right, the Send button and the URL input never draw over the response pane or its status line.
- [ ] Done status line (`response/mod.rs`): the row has `min_w_0()` and `overflow_hidden()`. The status text and the `· {total} · {size}` text truncate with "…" (`truncate()` on a `min_w_0` child) and do not wrap. The tooltip shows the full line, for example `500 Internal Server Error · 12 ms · 1.2 KB`, with the TTFB text on a second line when it is known. That full text comes from a pure helper with a unit test in `response/mod.rs`, covering one case with TTFB and one without.
- [ ] The `Phase::Failed` branch is unchanged (`git diff` shows no edits to `response/mod.rs:144-170` apart from moved line numbers).
- [ ] The request pane gets no horizontal scroll: no `overflow_x_scroll` or new `Scrollable` is added.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean. At least 367 tests pass plus the new ones (baseline at 0a8b5f8).
- [ ] `cargo build --release -p wisp-app`, then `RENDERER=hardware scripts/measure/measure.sh target/wt-61/release/wisp <scratch>/m.json` reports status "measured", with binary ≤ 20,971,520 B, idle ≤ 122,880 KiB and after-request ≤ 131,072 KiB, so the `WISP_AUTOSEND_URL` hook still works. The report gives the three figures and their deltas from task 60. `scripts/measure/results/` is unchanged.
- [ ] Hand check, **run by the user** (the orchestrator's mouse clicks do not register), on a temp copy of the `my-api` fixture. The implementer writes the steps into the report:
  1. Resize the window down: it stops at 720×480.
  2. At 720 px, drag the request/response splitter fully both ways and the sidebar splitter to both ends: the URL row never overlaps the response pane.
  3. Send to a local server that returns 500 (for example a three-line `python3` `BaseHTTPRequestHandler`). The line reads `500 Internal Server Error · … · …`, is truncated with "…" when the pane is narrow, and hovering shows the full text.
  4. Send to `http://127.0.0.1:1` (Connection refused). The failed line truncates with "…" and hovering shows the full message. This closes the hand check left unverified since task 53.
- [ ] Any other defect found is filed as a successor task, not fixed here.

## Out of scope
- Horizontal scroll for the request pane (rejected by the user).
- Changing panel `size_range`s or the sidebar range, unless clipping alone cannot stop the overlap. If so, stop and report rather than guess a figure.
- Other status phases (Sending, Receiving, Cancelled) and the `Phase::Failed` branch.
- The highlight strip and problems list under the URL row (task 48 already clips the strip).
- Cookies, Headers and Body sub-pane layout, sidebar and tab-bar overflow.
- Per-platform behaviour of `window_min_size` on Linux and Windows (verify on macOS arm64 only).
- Updating PLAN.md, HISTORY or docs (scribe's job).
