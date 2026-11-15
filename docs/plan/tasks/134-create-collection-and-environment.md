# 134 — Add storage functions to create a new collection and a new environment

**Repo:** .
**Depends on:** 129
**Owns:**
- crates/storage/src/ops.rs
- crates/storage/tests/create.rs (new)

## Goal
There is no in-app way to start a collection (roadmap A5, Amendment 1). Add
two storage functions the wave-2 sidebar and palette actions will call:
create `collection.toml` (format 1) in a chosen folder, and create an empty
`environments/<name>.toml`. Both refuse to overwrite anything and never write
outside the collection root.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Amendment 1" (A5); docs/plan/roadmap.md row A5
- crates/storage/src/ops.rs:1-4 module rule (no overwrite, never leave the root); :451-488 `create_request`/`create_folder` (the shape and error variants to follow)
- crates/storage/src/collection.rs:145 `read_format`; :432 `open_collection` (must accept the result)
- crates/storage/src/environment.rs:18-52 `plain_path`, `secrets_path`, `check_name` (`pub(crate)`, call them, do not edit)
- crates/storage/src/fs.rs:91-110 `create_new` temp pattern
- Worktree `wt-new.sh . 134 create-collection-and-environment`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-134`

## Acceptance
- [ ] AC1 `pub fn create_collection(dir: &Path) -> Result<PathBuf>` creates `dir` if missing (parent must exist) and writes `dir/collection.toml` containing `format = 1` and no id, returning its path. `open_collection(dir)` then succeeds with an empty tree.
- [ ] AC2 `create_collection` refuses, with the existing "already exists" error variant ops uses, when `dir/collection.toml` exists as a file, directory or symlink (dangling included); nothing on disk changes on refusal (test).
- [ ] AC3 `pub fn create_environment(root: &Path, name: &str) -> Result<PathBuf>` writes an empty `environments/<name>.toml`, creating `environments/` if missing, and `load_environment(root, name)` then returns an environment with that name and no variables; `list_environments` includes it.
- [ ] AC4 `create_environment` refuses when `<name>.toml` or `<name>.secrets.toml` exists, when `check_name` rejects the name (`..`, separators, reserved device names), and when `environments` is a symlink leaving the root (unix-gated test); nothing on disk changes on refusal.
- [ ] AC5 Both write the final file with create-new semantics, so a file appearing between any check and the write is not overwritten (no exists-check followed by a plain write).
- [ ] AC6 Neither function writes a collection id, a secrets file, a `.gitignore` or any file other than the one named; tests use tempdirs only.

## Checks
- `storage-create` command — AC1-AC4, AC6
  run: `cargo test -p wisp-storage --test create`
- `suite-and-lint` command — all
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `create-new` read — AC5
  how: the final file is opened with `create_new(true)` or the `fs.rs` temp-then-no-clobber pattern, not an exists-check followed by a write.

## Out of scope
- Sidebar empty state and palette actions: wave 2 A5 app task.
- `environment.rs`, `vault.rs` (task 132); `collection.rs`, `lib.rs`, `validate.rs` (task 133).
- Assigning a collection id (the first Send already does it).
