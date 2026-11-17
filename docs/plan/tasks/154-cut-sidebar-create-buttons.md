# 154 — Cut task 148's sidebar empty-state buttons

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/sidebar.rs (the two reverts only)

## Goal
User decision 2026-11-17: take the first cut from the size plan (Amendment 1,
roadmap D-A). Remove the sidebar "New collection…" button and the environment
select's "New environment…" entry that task 148 added, and keep the palette
commands that do the same.

## Context
- The sidebar work is two commits, both `crates/app/src/ui/sidebar.rs` only: 2ae2e2b (buttons and the `NEW_ENV` select entry, `sync_env`/`choose_env`) and d5aed9e (stack the buttons vertically). Revert d5aed9e first, then 2ae2e2b; reverting 2ae2e2b alone conflicts. No later commit on main touches sidebar.rs (`git log d5aed9e..main -- crates/app/src/ui/sidebar.rs` is empty at 55d55ba).
- Dependents checked by the planner at 55d55ba: `NEW_ENV` and `"sidebar-new"` appear only in sidebar.rs (:28, :331, :347, :902) and no test in sidebar.rs (:972 on) uses them. The palette commands live in `palette.rs:14-15,68-69,92-93,141`, `ui/palette.rs:34-35`, `ui/root.rs:465-520` and `ui/create.rs`; none references the sidebar.
- Task 155 edits `actions.rs`, `palette.rs`, `ui/palette.rs`, `ui/root.rs`, `ui/url_bar.rs`, `workspace.rs` in parallel; this task must not touch them.
- Worktree: `./.claude/scripts/wt-new.sh . 154 cut-sidebar-create-buttons`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-154`. Run every command in the foreground; never start a background command; never use `pkill` or `killall` (the user may be running Wisp).

## Acceptance
- [ ] AC1 The branch carries two revert commits, `git revert --no-edit d5aed9e` then `git revert --no-edit 2ae2e2b`, with no other change to their content.
- [ ] AC2 After the reverts, `crates/app/src/ui/sidebar.rs` equals its content at 2ae2e2b^ (the tree before 148's sidebar commit).
- [ ] AC3 The palette still offers "New collection…" always and "New environment…" with an active collection (existing `palette` tests pass unchanged).
- [ ] AC4 Only `crates/app/src/ui/sidebar.rs` changes; the full suite, `cargo fmt --check` and clippy pass.

## Checks
- `sidebar-restored` command — AC2
  run: `git diff --exit-code 2ae2e2b^ HEAD -- crates/app/src/ui/sidebar.rs`
- `only-sidebar` command — AC4
  run: `git diff --name-only main...HEAD | grep -vx crates/app/src/ui/sidebar.rs | wc -l | grep -x 0`
- `palette` command — AC3
  run: `cargo test -p wisp-app --bin wisp palette 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC4
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `two-reverts` read — AC1
  how: `git log main..HEAD` shows exactly the two revert commits, in that order.

## Out of scope
- Any change to the palette commands, `ui/create.rs` or the PLAN follow-ups from 148 (`root.rs:481-487`, `create.rs:51`).
- Further cuts (`suggest_confirm`, Checks detail, Explain shadow list): not cut; the gate is raised (task 156).
- Docs: task 157 (README) and task 153 (spec amendment).
