# 46 — Give KvTable virtual rows full-width name and value columns

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/kv.rs

## Goal
Above 50 rows KvTable switches to its `v_virtual_list` branch, and there the
rows draw with zero-width name and value columns: only the checkbox and the ×
show, and the trailing empty row's inputs are invisible, so rows past 50 cannot
be edited. Find the cause and fix it so a virtual row looks and behaves the
same as a plain row.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Scrollbars on the sidebar, response Headers and Cookies, and KvTable`: where the defect was found (task 45 hand check).
- docs/plan/PLAN.md, "Pane-scrollbar defects", "P2, KvTable virtual rows".
- crates/app/src/ui/kv.rs:279-358: `render_row`. The row is `flex flex_row` with fixed-width checkbox, note and delete slots and two `div().flex_1()` input cells (:311, :325). The row itself has no width set.
- crates/app/src/ui/kv.rs:363-403: `Render for KvTable`. The virtual branch passes `size(px(1.), px(ROW_HEIGHT))` as every item size (:368). The plain branch wraps each row in `flex_none` inside a `flex_col` and renders correctly.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.8.1/src/virtual_list.rs`: :133 says only the height of an item size is used and width is inferred. :320-335 measures one item with `layout_as_root` at `MinContent` height and the list width (or `MinContent` with no list width). :513-517 sets the content width to the longest item size's width (here 1 px). :370-378 gives each vertical item the content bounds' width. Likely cause, unconfirmed: items are laid out at 1 px or min-content width, so `flex_1` cells with no intrinsic width get zero. Confirm before fixing.
- crates/app/src/ui/response/body.rs:347 and :522: Raw also uses `px(1.)` widths, but its text lines have intrinsic width, which is why it looks fine.
- docs/plan/PLAN.md, "KvTable (29)": the accepted residual that Tab cannot reach rows outside the viewport. It stays accepted.
- Binary budget: 19,014,912 B of 20,971,520 B.

## Acceptance
- [ ] `git diff --stat main` lists only `crates/app/src/ui/kv.rs`. `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] The report names the confirmed cause in one or two sentences, citing `virtual_list.rs:line` for the gpui-base behaviour it depends on.
- [ ] `VIRTUALISE_ABOVE` and `ROW_HEIGHT` are unchanged. The checkbox and delete button keep `tab_stop(false)`. The KvTable model, events and key handling are unchanged in the diff.
- [ ] The plain branch (50 rows or fewer) renders as on main: any change to `render_row` that both branches share is called out in the report.
- [ ] Existing tests in `kv.rs` pass unchanged.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-46 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-46 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-46 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-46 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 19,014,912 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check: a request with 80 query params. In Params every visible row shows its name and value text at the same column widths as a 10-param request. Dragging the scrollbar to the bottom shows param 80 and the trailing empty row with visible inputs. Editing the value of param 70 updates the URL, and typing a name in the trailing row adds row 81.
- [ ] Orchestrator hand check: with the 80-param request, clicking a visible row's name then pressing Tab goes name, value, next row's name (checkbox and × skipped), as with plain rows.
- [ ] Orchestrator hand check: a request with 10 params still renders and edits as on main.

## Out of scope
- Scroll-to-focus for Tab past the viewport (accepted residual), and any key handling.
- The virtualisation threshold, the O(n) `locate`, and the scrollbar wiring task 45 added.
- `request/*.rs` layout, including the inherited-headers squeeze and the Path, Auth, Settings, Multipart and File overflow residuals.
