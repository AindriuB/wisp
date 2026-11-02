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

## Amendment (orchestrator, 2026-11-02)
Code review found that the ancestor-only skip rule (acceptance item 3) lets symlinks that fan out and rejoin make `scan_dir` (crates/storage/src/collection.rs:264-273) visit about 2^(N+2) folders. The shape is a "diamond": folders l0..lN, each holding two symlinks `a` and `b` -> `../l{i+1}`. On that input `open_collection` can hang or run out of memory.

User decision: keep the ancestor rule, and add a fixed folder budget for the whole walk, as a named const (e.g. `MAX_FOLDERS = 10_000`). Once the walk passes it, open fails with a clear, user-readable error that names the limit. Under the budget, a linked folder still appears everywhere it is linked.

Rejected:
- Walking each canonical dir once. A folder linked from two places would show only in the first.
- A depth cap. A wide diamond under the cap is still huge.

Added acceptance:
- [ ] A named const caps the folders visited in one walk. Passing it makes `open_collection` return Err, and the error message names the limit.
- [ ] A `#[cfg(unix)]` test builds a symlink diamond deep enough to pass the budget and asserts that open fails fast with the budget error, not a hang.
- [ ] The existing cycle and walk tests still pass.

## Review round 1 — findings
- [P2] crates/storage/src/collection.rs:264-273 — after the change, scan_dir only skips a directory that is one of its own ancestors, so nothing bounds how many times one directory is walked. Symlinks that fan out and rejoin (a "diamond") make the walk, and the `folders` map, grow exponentially. The old whole-walk `visited` set prevented this. Input: a collection whose root holds l0..lN, each l{i} containing symlinks a -> ../l{i+1} and b -> ../l{i+1}. l{i} is reached 2^(i+1)-1 times, so ~2^(N+2) folder visits, each with its own canonicalize and `folders` entry; at N=30 open_collection hangs or runs out of memory. Settled by the amendment above.
- [NOTE, optional] crates/storage/src/request.rs:221 — the `.toml` suffix check is a plain string match. On Windows, `other.toml.` or `other.toml ` passes it, but the filesystem resolves it to other.toml. Same kind of alias as the out-of-scope ::$DATA items.
- [NOTE, optional] crates/storage/src/request.rs:224-226 — the older "is the request file itself" name check is now unreachable, because the suffix check catches every case it did. Harmless.

## Review round 2 — findings

Reviewed HEAD: 31ff2472009263d223914a608adabae2ac5bc76c

- [P3, optional but requested] crates/storage/src/collection.rs:512-536 — the diamond test takes ~10 s against its own 20 s timeout (≈2x headroom); it walks down ~30 chained symlinks and each visit runs canonicalize and load_folder_defaults. On a runner ~2x slower it fails with "open_collection did not return: Timeout" while the budget is correct. Fix: use ~15 layers instead of 30 (2^15 > 10000, so the budget is still hit, and each visit resolves half as many links). route: implementer — test shape only.
