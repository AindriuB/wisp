# 34 — Build the ⌘K palette with quick-open and the ⌘E environment switcher

**Repo:** .
**Depends on:** 21, 25, 28
**Owns:**
- crates/app/src/ui/palette.rs

## Goal
One overlay view serves both shortcuts. ⌘K opens it over requests and commands (quick-open plus command palette); ⌘E opens it in environment mode. Items come from `palette::items`, ranking from `palette::filter`, and choosing an item opens the request, switches the environment, or dispatches the command's action.

## Context
- crates/app/src/ui/palette.rs — frozen constructor from task 21 (`PaletteView::new(workspace, mode, ..)`, emits `DismissEvent`; the root view opens and closes it, task 28)
- crates/app/src/palette.rs — `items`, `filter`, `CommandId`, `PaletteKind` (task 25)
- crates/app/src/actions.rs — the actions each `CommandId` maps to
- Task 28 touched only the keystroke strings in `actions::bind_keys` (`cmd-`/`ctrl-` per platform); the `CommandId` → action mapping is still this task's.
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/command/ — `Command`, `CommandState`, `CommandItem` (`filterable(false)` + `on_query`)
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-34`.

## Acceptance
- [ ] The view focuses its search input on open; typing re-runs `palette::filter` on every query change with a limit of 50 and shows label, detail and matched-character highlighting from `Match::positions`; the gpui-component command list is used with its own filtering turned off.
- [ ] Up/Down move the selection, Enter confirms, Escape and clicking outside emit `DismissEvent`.
- [ ] Confirming `Request(path)` calls `Workspace::open_request`; `Environment(name)` calls `set_environment` (a failure shows a notice); `Command(id)` dispatches the matching action from `actions.rs` (the mapping is one exhaustive `match` so a new `CommandId` fails to compile until mapped). The view dismisses itself after confirming.
- [ ] In `PaletteMode::Environments` the placeholder reads "Switch environment" and the active environment is pre-selected; with no collection open it shows a single disabled line "Open a collection first".
- [ ] Each command item shows its keybinding hint where one exists (`bind_keys`).
- [ ] Hand check (orchestrator, window frontmost) on the `my-api` fixture copy: ⌘K then `gu` ranks "Get user" first and Enter opens it; ⌘K then `theme` runs Toggle theme; ⌘E lists `No environment` and `dev`, and choosing `dev` changes the URL bar's resolved hover values.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- Ranking logic (task 25).
- Recent-items or persisted palette history.
- Full-text search inside request bodies.
