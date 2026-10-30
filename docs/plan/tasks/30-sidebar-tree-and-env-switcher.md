# 30 — Build the collection sidebar tree with environment switcher and file operations

**Repo:** .
**Depends on:** 21, 23, 28
**Owns:**
- crates/app/src/ui/sidebar.rs

## Goal
The sidebar shows the environment switcher on top and the open collection's ordered tree below. Clicking a request opens (or focuses) its tab; a context menu creates, renames, moves and deletes requests and folders through `wisp_storage::ops`, then tells the workspace so open tabs follow.

## Context
- crates/app/src/ui/sidebar.rs — frozen constructor from task 21
- crates/app/src/workspace.rs — `open_request`, `set_environment`, `reload_tree`, `on_node_moved`, `on_node_deleted`, `WorkspaceEvent` (task 28)
- crates/storage/src/collection.rs:17-50 — `Node` (`Folder`, `Request`, `Invalid`)
- `wisp_storage::ops` — task 23
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/tree.rs, src/select.rs, src/menu/, src/dialog/
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-30`.

## Acceptance
- [ ] The top row is a select listing `No environment` and every name in `OpenCollection::environments`, showing the active one; choosing one calls `set_environment`; a failure shows the error as a notice and keeps the previous selection. A `⌘K` hint button opens the palette (dispatches `OpenPalette`).
- [ ] With no collection open, the sidebar shows an "Open collection…" button dispatching `OpenCollection`; with `OpenCollection::error` set, the error is shown above the tree.
- [ ] The tree renders `CollectionTree::nodes` in order with folders collapsible (expansion state survives `CollectionChanged` reloads, keyed by path); request rows show the method in a fixed-width coloured label and the name; the row of the active tab is highlighted; `Node::Invalid` rows show the file name with a warning marker and the error as a tooltip and do not open.
- [ ] Clicking a request row calls `open_request(path)`; Enter on the focused row does the same; arrow keys move the selection.
- [ ] Context menu on a folder: New request, New folder, Rename, Move to…, Delete; on a request: Rename, Move to…, Delete; on the empty area: New request, New folder (at the root). Rename and New use an inline input or dialog prefilled with the current name; Move to… lists every folder plus the root; Delete asks for confirmation naming the item and, for a folder, its item count.
- [ ] Each operation calls the matching `wisp_storage::ops` function, then `on_node_moved` / `on_node_deleted` as appropriate and `reload_tree`; a `StorageError` is shown as a notice and the tree is left as it was. New request opens the created request in a tab.
- [ ] Hand check (orchestrator, window frontmost) on a temp copy of the `my-api` fixture: tree order matches `folder.toml`'s `order`; create, rename, move and delete each change the files on disk as shown by `git status`/`ls` in the copy; renaming a request with an open tab keeps the tab and its unsaved draft; switching environment changes the resolved URL shown once task 31 lands.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- Drag-and-drop moves (Move to… only in v1).
- Editing environments or collection/folder defaults (auth, headers, vars) from the UI.
- Multiple open collections.
