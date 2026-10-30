# 23 — Add create, rename, move and delete operations for collection nodes

**Repo:** .
**Depends on:** none
**Owns:**
- crates/storage/src/ops.rs (new)
- crates/storage/src/lib.rs (insert `pub mod ops;` only)
- crates/storage/src/error.rs (insert new variants only)

## Goal
The sidebar's create/rename/move/delete must map to files. Storage gets one module that performs these operations on request files (with their sidecar bodies) and folders, keeps `order = [...]` lists in `folder.toml`/`collection.toml` consistent, and never overwrites or escapes the collection root.

## Context
- crates/storage/src/collection.rs:159-230 — `load/save_folder_defaults`, `load/save_collection_defaults` (the `order` field)
- crates/storage/src/request.rs:91-485 — `load_request`/`save_request`, sidecar name and identity rules
- crates/storage/src/fs.rs:113-120 — `atomic_write`
- crates/storage/src/error.rs:7-30 — `StorageError`
- PLAN carry-forward (17): a sidecar may name a sibling request file; renames here must not clobber it
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-23`.

## Acceptance
- [ ] `wisp_storage::ops` exposes, all paths relative to `root` and all returning `Result<_, StorageError>`:
  `pub fn slug(name: &str) -> String`;
  `pub fn create_request(root: &Path, dir: &Path, name: &str) -> Result<PathBuf>` (writes a request with `name`, `method = "GET"`, `url = ""` to `<dir>/<slug>.toml`, appending `-2`, `-3`… when taken);
  `pub fn create_folder(root: &Path, parent: &Path, name: &str) -> Result<PathBuf>`;
  `pub fn rename_request(root: &Path, rel: &Path, new_name: &str) -> Result<PathBuf>` (sets `name` in place via `toml_edit`, renames the file to the new slug, renames a sidecar body along with it and rewrites `body.file`);
  `pub fn rename_folder(root: &Path, rel: &Path, new_name: &str) -> Result<PathBuf>`;
  `pub fn move_node(root: &Path, rel: &Path, new_parent: &Path) -> Result<PathBuf>` (request plus sidecar, or folder);
  `pub fn delete_node(root: &Path, rel: &Path) -> Result<()>` (request plus sidecar, or folder recursively).
- [ ] Rename, move and delete remove the old name from the parent's `order` list (`folder.toml`, or `collection.toml` at the root) and rename replaces it in place; comments in those files survive (test compares the untouched lines byte for byte).
- [ ] No operation overwrites an existing file or directory: a target collision returns a new `StorageError::Exists { path }` and leaves the disk unchanged (tested for request, sidecar and folder collisions).
- [ ] Operations reject, with `StorageError::Invalid`, any `rel`/`dir`/`new_parent` that is absolute, contains `..`, resolves outside `root`, or names the root itself, `environments/`, `.wisp/` or `collection.toml`; moving a folder into itself or a descendant is rejected.
- [ ] A rename whose new sidecar name equals an existing sibling request file fails with `Exists` (covers the PLAN 17 clobber case).
- [ ] Every operation's effect is visible to `open_collection` in a test (the tree shows the new name/location, the old one is gone).
- [ ] Tests use temp directories; `cargo test -p wisp-storage` passes with all tests present at `49da271` plus the new ones.
- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- Moving or deleting environments, or editing environment files.
- A trash/undo facility (delete is permanent; the UI confirms first).
- The other storage carry-forwards in PLAN (method parser unification, symlink visit order, broken `folder.toml` failing the open).
- Anything in `crates/app` or `crates/core`.
