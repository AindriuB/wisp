# 96 — Add the Import dialog

**Repo:** .
**Depends on:** 94
**Owns:**
- crates/app/src/ui/import.rs (new)
- crates/app/src/ui/mod.rs (one `pub mod import;` line)
- crates/app/src/actions.rs (one action, its registration and binding list entry)
- crates/app/src/palette.rs (one `CommandId` and its label)
- crates/app/src/ui/palette.rs (dispatch arm for the new command only)
- crates/app/src/ui/root.rs (the dialog's overlay slot and `on_action` only)

## Goal
A user can bring requests in from curl, Postman v2.1 or OpenAPI 3 through an
"Import..." command, see what will be created and every warning, and write it
into the open collection. The dialog does not edit `workspace.rs`; the watcher
refreshes the tree after the write.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#1-importers-storageimportcurl-postman-openapi`
- `crates/storage/src/import/` — `detect`, `parse`, `write`, `ImportResult`, `WriteReport` (task 94)
- `crates/app/src/ui/root.rs:82,153-160,438-521` — palette overlay pattern, `DismissEvent`, `on_action` chain
- `crates/app/src/palette.rs:10-70`, `crates/app/src/actions.rs:5-30,110-130` — command and action registration
- `crates/app/src/workspace.rs:25-60` — `Workspace.collection` (`tree.root`) is public: read it, do not edit the file
- PLAN.md "Palette and environment switcher (34)" lesson: overlays handle Escape with `on_action` on their own div and restore focus on dismiss; "Auth and body editors (32)" lesson: `InputState::set_value` emits no Change event
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/96-import-dialog`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-96`

## Contract
| Step | Behaviour |
|---|---|
| Open | Command "Import..." in the command bar (no default key binding). With no collection open, the dialog shows "Open a collection first" and no Import button |
| Source | Two choices: "Paste curl" (multi-line input) or "Choose file..." (`prompt_for_paths`, one file; format from `detect`, else an error line naming the supported formats) |
| Preview | Parse runs off the UI thread. Shows the source name, counts (requests, folders, environments) and every warning (scrollable list) |
| Target | Folder name, default the import's `name` slugged (curl: root of the collection, no folder); secrets environment name, default "imported", shown only when the result has secrets |
| Import | Calls `write` off the UI thread; then shows the `WriteReport` (written counts, renames) and the warnings again, with a Close button |
| Errors | Parse and write errors show inline with the `ImportError` message; the dialog stays open |
| Dismiss | Escape or Close; focus returns to where it was |

## Acceptance
- [ ] AC1 "Import..." appears in the command bar and opens the dialog; the palette's existing commands are unchanged.
- [ ] AC2 Pure helpers in `ui/import.rs` (for example the preview summary and default folder name) have unit tests: counts for a nested result, warnings listed in order, slug of a name with spaces and slashes, curl targets the root.
- [ ] AC3 Parse and write run on the background executor; no `std::fs` or importer call runs in a render or event handler directly.
- [ ] AC4 With no collection open the dialog shows "Open a collection first" and offers no Import button.
- [ ] AC5 Escape closes the dialog via `on_action` on its own div and focus is restored.
- [ ] AC6 `crates/app/src/workspace.rs` is unchanged.
- [ ] AC7 `fmt`, `clippy -D warnings` and the app tests pass.

## Checks
- `import-ui-unit` command — AC2. cargo test writes no JUnit XML here; asserts at least 4 passing.
  run: `n=$(cargo test -p wisp-app -- ui::import 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 4`
- `workspace-untouched` command — AC6
  run: `git diff --exit-code main...HEAD -- crates/app/src/workspace.rs`
- `app-and-lint` command — AC7
  run: `cargo test -p wisp-app && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `dialog-shape` read — AC1, AC3, AC4, AC5
  how: the command is registered once; parse/write are inside `background_spawn`; the no-collection branch has no Import button; Escape is handled on the dialog div with focus restore.
- `hand-check` read — AC1, AC4, AC5
  how: the PR notes a run on macOS importing one curl command and the Postman fixture from `crates/storage/tests/fixtures/import/`, with the warnings shown; launched with `WISP_DATA_DIR` set to a temp dir, stopped by its own pid.

## Out of scope
- Importer logic (task 94); a bug there is reported, not fixed here.
- Drag-and-drop import, importing into a new collection, pasting curl into the URL bar.
- History UI (task 97), which reuses the same shared files after this lands.

## Notes
- Size: small (no new crates).
- Launching the app: record the pid you started and stop only that pid; never `pkill`/`killall` (the user may be running Wisp). A bundled build uses bundle id `io.github.aindriub.wisp.t96`.
