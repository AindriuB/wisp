# 58 — Unify the storage method parser, check cycles by ancestor, reject request files as sidecars

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/storage/src/collection.rs
- crates/storage/src/request.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-58`.

## Goal
M3 (v1 hardening). Closes three PLAN.md storage carry-forwards. First, two method parsers disagree on case: the tree accepts `post`, but `load_request` rejects it. Second, the visited set spans the whole walk, so a symlink to a sibling folder shows under whichever name `read_dir` returns first. Third, a sidecar may name a sibling request `.toml` file, and save then overwrites that request.

## Context
- crates/storage/src/collection.rs:225-235 `parse_method` (case-insensitive) and crates/storage/src/request.rs:77-88 `parse_method` (exact match).
- crates/storage/src/collection.rs:253-330 `scan_dir` with `visited: &mut HashSet<PathBuf>` (:259, :277-281) and :345 in `open_collection`.
- crates/storage/src/request.rs:206-240 `sidecar_path` and its rejection rules, used at :252, :326 and :455.
- PLAN.md "Request files (17)" and "Collection tree (18)".
- docs/conventions.md#tests — temp dirs, sentence names. Symlink tests are `#[cfg(unix)]`.

## Acceptance
- [ ] There is one `pub(crate)` method parser, used by both modules. It is case-insensitive: `method = "post"` loads as `Method::Post` through both `open_collection` and `load_request` (test).
- [ ] Loading a `method = "post"` file and saving it with no edit leaves the file byte-identical (test).
- [ ] A directory is skipped only when its canonical path equals one of its ancestors' canonical paths in the current descent. A symlink to a sibling folder now appears under its own name as well as the sibling's (test), and a self-referential symlink cycle still terminates and is skipped (test).
- [ ] `sidecar_path` rejects a name ending in `.toml` (case-insensitive) with the existing `invalid(.., "body.file", ..)` error, on load and on save. A test shows that saving a request whose sidecar is `other.toml` leaves `other.toml` byte-identical and returns Err.
- [ ] `cargo test -p wisp-storage` passes, including `tests/folder_order.rs` and `tests/resolve_integration.rs` unchanged. `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
- [ ] `git diff main -- crates/storage/src/lib.rs crates/storage/src/ops.rs crates/app` is empty.

## Out of scope
- A broken `folder.toml` failing the whole open. It needs a sidebar decision on how an invalid folder is shown and deleted; file a successor task if wanted.
- Directory symlinks pointing outside the root, `scope_for` with a non-canonical root, hard links, `::$DATA` aliases, the sidecar check-then-use race, and the drive-prefix check.
- A `FromStr` for `Method` in `wisp-core`.
