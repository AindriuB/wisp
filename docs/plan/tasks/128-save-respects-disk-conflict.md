# 128 — Make Save refuse to overwrite a file that changed on disk

**Repo:** .
**Depends on:** none
**Class:** data-loss fix. This is not a small change under the user's rules:
it changes fail-closed behaviour, so it gets a code review. Size is negligible,
so there is no measure task.
**Owns:**
- crates/app/src/workspace.rs — only `Workspace::save` (~2840-2873) and new
  tests in `mod env_wiring_tests` (~4503-)
- crates/app/src/ui/root.rs — only `on_save` (~416-422), and only if AC4's
  notice needs it

## Goal
Plain Save (Ctrl-S, `SaveRequest` → `Root::on_save` → `Workspace::save`) never
reads the tab's `DiskState`. While the "Changed on disk" banner is up, it
silently overwrites the newer file on disk. Make Save refuse to write in that
state, so the banner's existing choices decide. Also make the no-collection
branch report that nothing was saved instead of returning `Ok(())`. User
decision 2026-11-14: fix before the 0.2.0 tag.

## Context
- crates/app/src/workspace.rs:2840-2873 — `Workspace::save`. Lines 2841-2844
  emit "Open a collection to save" and return `Ok(())` with nothing written.
- crates/app/src/workspace.rs:2110-2128 — `apply_disk_state`. It sets
  `DiskState::ChangedOnDisk` when `draft::on_disk_change` returns `ShowBanner`
  (crates/app/src/draft.rs:67), which happens when the draft is dirty and the
  disk differs.
- crates/app/src/workspace.rs:2132-2152 — the banner's existing actions:
  `reload_from_disk` (take disk, `InSync`) and `keep_mine` (keep draft, set
  `InSync`, write nothing). There is no separate overwrite action. After
  "Keep mine", the next Save writes. That is the explicit overwrite path.
- crates/app/src/ui/root.rs:546-549 — `banner()` renders "Changed on disk" with
  "Keep mine" and "Reload". root.rs:842-847 wires the `ReloadFromDisk` and
  `KeepMine` actions. root.rs:416-422 — `on_save` shows
  `"Save failed: {e:#}"` for any `Err`.
- crates/app/src/tab.rs:36-41 — `DiskState { InSync, ChangedOnDisk, DeletedOnDisk }`.
- crates/app/src/workspace.rs:4503-4560 — the `env_wiring_tests` harness
  (`fixture` on a temp dir with `MemoryStore`, `open`). Use it to open the
  collection, then `open_request` a request file. Simulate the external edit
  by writing the file and calling `apply_disk_state` (or `on_disk_changes`)
  directly. Do not depend on the file watcher's timing.
- 0.3.0's Save-and-close builds on this checked save. It must be able to tell
  "saved" from "not saved" from the `Result` alone, which is why AC1 and AC5
  return `Err`.
- docs/conventions.md#tests and #errors.

## Acceptance
- [ ] AC1 When the tab's `disk` is `DiskState::ChangedOnDisk`, `Workspace::save`
  writes nothing (no `save_request`, no `create_request`) and returns `Err`.
  The tab stays dirty, its `disk` stays `ChangedOnDisk` (the banner stays up),
  and `saved` and `path` are unchanged.
- [ ] AC2 After `keep_mine` on that tab, `Workspace::save` writes the draft to
  disk, and the file's contents equal the draft.
- [ ] AC3 For a tab whose `disk` is `InSync`, `Workspace::save` behaves as
  before. That includes new untitled tabs and the cleanup of a just-created
  file on a failed write. Its file on disk equals the draft afterwards.
- [ ] AC4 The user sees one notice for a refused save. The notice names the
  banner's existing choices, for example "Changed on disk: choose Keep mine to
  overwrite, or Reload". No new banner, button, action, dialog or keybinding is
  added. The `banner()` function in root.rs is unchanged.
- [ ] AC5 With no owning collection, `Workspace::save` returns `Err` with the
  message "Open a collection to save", and nothing is written. The user sees
  that message exactly once; the separate `emit` is dropped or
  `on_save`'s notice is not duplicated.
- [ ] AC6 New `#[gpui::test]`s in `env_wiring_tests`, each name starting with
  `save_disk_conflict_`:
  (a) external edit → `ChangedOnDisk` → save leaves the disk file
  byte-identical to the external edit (AC1);
  (b) the same, then `keep_mine` → save writes the draft (AC2);
  (c) a normal edit-and-save on an `InSync` tab writes (AC3).
- [ ] AC7 `DeletedOnDisk` handling is unchanged. A save there still writes
  the file again, as it does today.

## Checks
cargo writes no JUnit XML here (task 99 Checks), so test checks are commands.
- `conflict-tests` command — AC1, AC2, AC3, AC6
  run: `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-128 cargo test -p wisp-app --lib save_disk_conflict_ -- --test-threads=1`
  The output must report `3 passed` or more, and `0 ignored`. Zero matched tests
  fails this check.
- `suite` command — AC3, AC7
  run: `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-128 cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `scope` read — AC1, AC4, AC5, AC7
  how: the diff touches only `Workspace::save`, the new tests and, if needed,
  `Root::on_save`. The `DiskState` check comes before any write or
  `create_request`. `banner()`, `keep_mine`, `reload_from_disk` and
  `apply_disk_state` are unchanged.

## Out of scope
- Save-and-close, or any close or quit prompt (0.3.0).
- Changing `keep_mine`, `reload_from_disk`, `draft::on_disk_change`, or the
  banner's look or wording.
- Any guard on `DeletedOnDisk` saves.
- Session or collection-state saves on quit (`quit_plan`, workspace.rs ~3388-3500).
- A merge or diff view for conflicts.
- Anything under `docs/` (scribe).
