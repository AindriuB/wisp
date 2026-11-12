# 113 — Sidebar collection switcher

**Repo:** `.`
**Depends on:** 112
**Size impact:** none expected (UI only, no new dependencies)
**Owns:**
- crates/app/src/ui/collections.rs (new)
- crates/app/src/ui/mod.rs
- crates/app/src/ui/sidebar.rs
- crates/app/src/ui/root.rs

## Goal
The sidebar shows which collection is active and lets the user switch to another open collection or close one, without the palette. The switcher is a new UI module that calls the `Workspace` API from task 112. It adds no workspace state.

## Context
- crates/app/src/ui/sidebar.rs:26, :240-305 — the sidebar header, environment picker and event subscription
- crates/app/src/ui/cookie_jar.rs and crates/app/src/ui/history.rs — the pattern for a panel module that reads the workspace
- Task 112's `active_collection()`, its open-list accessors, and its switch, close and confirm paths. Use them and do not reimplement them.

## Acceptance
- [ ] AC1 The sidebar header shows the active collection's name. With two or more open, it opens a list of the open collections in open order, with the path as a tooltip and a mark on the active one. Choosing one activates it.
- [ ] AC2 Each list row has a close control that uses task 112's close path, so dirty tabs still ask for confirmation.
  It calls 112's Close collection handler in `crates/app/src/ui/root.rs` (112 Amendment 1, item 3) and does not own a copy of it.
- [ ] AC3 With no collection open, the header shows today's empty state. With exactly one open, it shows the name and no list chevron.
- [ ] AC4 The list's row model is built by a pure function from the workspace's open list: names, paths and the active flag. A unit test covers 0, 1 and 3 collections, and a duplicate display name, which is told apart by its path.
- [ ] AC5 Hand check, macOS, with a distinct bundle id and a temp `WISP_DATA_DIR`, stopping only the launched pid. Open three collections, switch between them through the sidebar, and close one with a dirty tab (the prompt appears). Relaunch, and the open list and active collection are restored. The PR lists what was observed.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `app-unit` command — AC4
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-113; test "$(cargo test -p wisp-app --locked ui::collections -- --list 2>/dev/null | rg -c ': test$')" -ge 4 && cargo test -p wisp-app --locked`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-113; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-workspace` command — Goal
  run: `git diff --exit-code main...HEAD -- crates/app/src/workspace.rs Cargo.lock`
- `ui` read — AC1-AC3
  how: the switch and close paths call workspace methods, with no state duplicated in the view.
- `hand` read — AC5
  how: the PR lists each observed step.

## Out of scope
- Workspace model changes; a gap found in 112's API is a successor task.
- Drag reordering, renaming collections, and a recent-collections list.
- Keychain UI (task 115).
