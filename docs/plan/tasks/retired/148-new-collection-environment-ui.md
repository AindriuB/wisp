# 148 — Create a collection or an environment from the app (A5)

**Repo:** .
**Depends on:** 139, 143 (both edit files this task edits: `ui/mod.rs` after 139, `ui/root.rs` after 143)
**Owns:**
- crates/app/src/actions.rs (two new actions; no key bindings)
- crates/app/src/palette.rs (two `CommandId` variants and `ALL`)
- crates/app/src/ui/palette.rs (`action_for` arms only)
- crates/app/src/ui/root.rs (two `on_action` listeners and their handlers only)
- crates/app/src/ui/sidebar.rs (the empty state, :881-890, and the environment select's row only)
- crates/app/src/ui/create.rs (new: the shared create flows)
- crates/app/src/ui/mod.rs (insert-only: `pub mod create;`)

## Goal
A user with no collection can start one in the app, and a user with a
collection can add an environment, without touching files by hand. Both go
through the storage functions task 134 added. Two palette commands always; two
sidebar buttons as a separate, cuttable commit.

## Context
- docs/plan/roadmap.md register A5 and decision D-A (first cut if the size soft line trips, starting with the sidebar buttons and keeping the palette actions)
- crates/storage/src/ops.rs:498-512 `create_collection(dir)` (creates `dir` if its parent exists, refuses an existing `collection.toml`), :515 `create_environment(root, name)` (name allowlist via `environment::check_name`)
- crates/app/src/ui/root.rs:426-445 `on_open_collection` (folder prompt, `Workspace::open_collection`, notice on error) — follow it
- crates/app/src/workspace.rs `collection_root` (:1300), `active_collection_ix` (:1295), `set_environment` (:1733), `open_collection` (:1643) — public; do not edit workspace.rs
- crates/app/src/palette.rs:7-80 `CommandId`, `ALL`; crates/app/src/ui/palette.rs:27-48 exhaustive `action_for`
- crates/app/src/theme.rs — theme tokens only
- Worktree: `./.claude/scripts/wt-new.sh . 148 new-collection-environment-ui`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-148`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 Palette command "New collection…" prompts for a folder (directories, may create), calls `create_collection`, then `open_collection` on it; an existing `collection.toml` gives the notice "Already a collection: opened it" and opens it; any other error is a notice and nothing is opened.
- [ ] AC2 Palette command "New environment…" (only listed when a collection is active) asks for a name in a single-line input, calls `create_environment` on the active collection's root, then makes it the active environment; an invalid or existing name shows the storage error's message inline and creates nothing.
- [ ] AC3 The name check and the outcome-to-notice mapping are pure functions in `ui/create.rs`, table-tested (valid, empty, existing, invalid characters, collection already present).
- [ ] AC4 In a separate commit: the sidebar empty state shows "New collection…" beside "Open collection…", and the environment select offers "New environment…"; both dispatch the AC1/AC2 actions. Reverting that commit alone leaves the palette commands working (read check).
- [ ] AC5 No key binding added; no `workspace.rs` change; theme tokens only; full suite, fmt and clippy pass.

## Checks
- `create-flows` command — AC3
  run: `cargo test -p wisp-app --bin wisp ui::create 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `palette` command — AC1, AC2
  run: `cargo test -p wisp-app --bin wisp palette 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `untouched` command — AC5
  run: `git diff --exit-code main...HEAD -- crates/app/src/workspace.rs crates/storage`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cuttable` read — AC4
  how: the sidebar commit touches only `ui/sidebar.rs`; `git revert` of it would build.

## Out of scope
- Storage changes (134 is merged; a defect is reported, not fixed).
- The guard palette command and `actions.rs`/`palette.rs` edits for it (wave 3, 3A, after this task).
- Templates or a starter request in a new collection.

## Review round 1 — findings
Reviewed HEAD 80e165a858bba36aa9d6a54186ff7ad55d57927d.
- [STYLE] crates/app/src/ui/create.rs:182 — the test name `env_name_table` is a noun phrase, not a sentence. docs/conventions.md:106 says "Test names read as sentences". The two tests next to it in the same module follow that rule: `collection_outcomes_map_to_notices` (create.rs:205) and `create_env_writes_once_and_refuses_the_rest` (create.rs:224). A fitting rename is `env_names_are_checked_for_empty_bad_characters_and_duplicates`.

## Wave verified — ffb045ff536b29b8cfdc854c5089ae4eb571aefa

## External review — findings
Key sha256:7f60c61304f7 · reviewed head 5e6d874a10fc82f48c0a5f41a5a4261dc27a90bc · round 2 (pasted by the user; static read-only review of PR #108)
* [P2] Restore focus when dismissing the environment dialog — `crates/app/src/ui/create.rs:155`, also line 130. Cancelling or successfully submitting removes the focused dialog without restoring workspace focus. Workspace shortcuts then stop working until the user clicks a focusable control. Capture the previous focus and restore it before both dismissal paths, as existing dialogs do.

## Wave verified — d416847efc0907133e56cc8f40a260d8ee4a94bd

## External review — findings
Key sha256:170513b0a094 · reviewed head d416847efc0907133e56cc8f40a260d8ee4a94bd · round 3 (pasted by the user; static read-only review of PR #108)
* [P2] Bind the environment dialog to its opening collection — `crates/app/src/ui/create.rs:125–133`. Open “New environment” in collection A, use Cmd/Ctrl+O to open B, then submit the still-open dialog: it creates and selects the environment in B. Capture the opening collection’s root and validate it before writing, as `ImportView` does.

## Wave verified — 4fb51e9991af886b5ce24561d923aa2d8fe6c743

## External review — findings
Key sha256:a8b4ef5ae0ee · reviewed head d416847efc0907133e56cc8f40a260d8ee4a94bd (sidebar.rs unchanged at current head 4fb51e9) · round 4 (pasted by the user; static read-only review of PR #108)
* [P2] Keep both empty-state buttons visible — `crates/app/src/ui/sidebar.rs:897–900`. The new horizontal layout places two nonshrinking buttons on one line. Their combined widths exceed the default 240 px sidebar, pushing “Open collection…” outside the visible area. Stack the buttons vertically or allow wrapping.

## Wave verified — d5aed9e8c3a42db1ccbc047c029c3459650ac85c
Cutting the sidebar work under D-A now means reverting d5aed9e then 2ae2e2b (both sidebar.rs only); reverting 2ae2e2b alone conflicts.

## Outcome
Merged via PR #108, verified at ffb045ff536b29b8cfdc854c5089ae4eb571aefa (tester PASS, reviewer APPROVE). The palette commands "New collection..." and "New environment..." and the two sidebar buttons (a separate, revertable commit) are in. Four review rounds: round 1 (internal) was a test rename only; rounds 2-4 were external, each with one P2, fixed in d416847 (restore focus when the new-environment dialog is dismissed), 4fb51e9 (bind that dialog to the collection it was opened in) and d5aed9e (stack the sidebar empty-state buttons vertically). The branch head is d5aed9e. Cutting the sidebar work under D-A means reverting d5aed9e then 2ae2e2b. Follow-ups are in PLAN under "Follow-ups from task 148".
