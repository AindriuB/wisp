# 52 — Make the Path, Auth, Settings, Multipart and File sub-tabs scroll

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/ui/request/path.rs
- crates/app/src/ui/request/auth.rs
- crates/app/src/ui/request/settings.rs
- crates/app/src/ui/request/body.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-52`.

## Goal
M3 (v1 hardening). Closes the PLAN.md residual "Path, Auth, Settings, Multipart and File sub-tabs keep content-height wrappers and still overflow" (Pane-scrollbar defects, tasks 46-49). Each of these editors must fill the bounded editor area that `RequestPane` already gives it, and scroll vertically with a draggable scrollbar when its content is taller than the pane, as Params and Headers already do.

## Context
- crates/app/src/ui/request/mod.rs:106-120 — `RequestPane` wraps each editor in `div()` and gives it `.flex_1().min_h_0()`, so the area is bounded. Not owned; it must not change.
- crates/app/src/ui/request/headers.rs:188-195 and params.rs:133 — the shape that already works (`size_full`, `flex_1().min_h_0()` around the scrolling child).
- crates/app/src/ui/response/cookies.rs:101-123 — `.id(..)` plus `overflow_y_scrollbar()` (gpui-component `ScrollableElement`) on a bounded `flex_1().min_h_0()` column.
- crates/app/src/ui/request/path.rs:139, auth.rs:322, settings.rs:104 — the render roots.
- crates/app/src/ui/request/body.rs:536 `render_multipart`, :600 `render_file`, :634-645 `render` (Form already uses `flex_1().min_h_0()` at :643).
- PLAN.md "Pane-scrollbar defects": do not add `.flex()` to the `div().w_full()` wrapper around KvTable virtual rows (task 46).
- PLAN.md "Auth and body editors (32)": `InputState::set_value` emits no Change event. Do not restructure input wiring here.

## Acceptance
- [ ] The render roots of path.rs, auth.rs and settings.rs, and the Multipart and File branches of body.rs, each have an element with a stable `.id(..)` and `overflow_y_scrollbar()` (or the `vertical_scrollbar` layer used in the response body), inside a `size_full` / `flex_1().min_h_0()` chain.
- [ ] `git diff main -- crates/app/src/ui/request/mod.rs crates/app/src/ui/kv.rs` is empty.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Release binary size (`cargo build --release -p wisp-app`, then `target/wt-52/release/wisp`) is reported with its delta from 19,114,864 B (main at d6a8bbb). It is ≤ 20,971,520 B.
- [ ] Hand check (orchestrator, window frontmost, request pane about 200 px tall): Path with 12 path params, Auth set to Basic, Settings, Multipart with 15 parts and File each show a draggable scrollbar, and the last control can be scrolled into view and edited. With a tall pane, no scrollbar shows and the layout looks the same as on main.

## Out of scope
- Params and Headers (already scroll), the inherited-header list's natural height (the spec says it keeps it), and KvTable internals.
- Scroll-to-focus when tabbing into an off-screen control.
- The other body-editor residuals: `path.exists()` on every render, relative File paths checked against the cwd, a Choose… pick into a deleted row.
- The Settings timeout residual (`settings.rs:73-75`).
