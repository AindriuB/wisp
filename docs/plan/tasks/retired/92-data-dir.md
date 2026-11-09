# 92 — Add the per-user data dir and the collection ID

**Repo:** .
**Depends on:** 91
**Owns:**
- crates/storage/src/data_dir.rs
- crates/storage/src/collection.rs (new functions and their tests)

## Goal
Give `storage` one place that decides where per-user data lives (history now,
cookies in wave 2), following the same portable rule as `settings.toml`, and give
each collection a random ID, written once into `collection.toml`, that keys its
data there. Moving or renaming the collection folder keeps its ID.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#per-user-data-dir` and `#4-keychain-storage` (Collection ID)
- `crates/app/src/settings.rs:41-75` — the portable rule and OS dir lookup to mirror (do not edit it)
- `crates/storage/src/fs.rs:155-163` — `atomic_write_private` (0600 on unix)
- `crates/storage/src/collection.rs:137-196` — `read_doc`, `load/save_collection_defaults`; writes go through `toml_edit`
- `docs/architecture.md#settings-location`, `#boundaries-that-must-not-be-crossed`
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/92-data-dir`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-92`

## Contract
`data_dir.rs`:
- `pub fn location(exe_dir: &Path, portable_marker: bool, os_data_dir: Option<&Path>, override_dir: Option<&Path>) -> Option<PathBuf>` — pure. Order: absolute `override_dir`; else `<exe_dir>/data` when the marker exists; else `<os_data_dir>/wisp`.
- OS data dir: macOS `$HOME/Library/Application Support`; Windows `%LOCALAPPDATA%`; other unix `$XDG_DATA_HOME` if absolute, else `$HOME/.local/share`. `override_dir` is the env var `WISP_DATA_DIR` (absolute paths only; used by tests and by agents so they never write the user's real data dir).
- `pub struct DataDir { root: PathBuf }` with `DataDir::resolve() -> Option<DataDir>` (reads `current_exe`, the `portable` marker and the env), `DataDir::at(root)` (for tests), `root()`, `history_dir(&CollectionId) -> PathBuf` (`<root>/history/<id>`) and `cookies_file(&CollectionId) -> PathBuf` (`<root>/cookies/<id>.json`, for wave 2).
- `pub fn ensure_private_dir(path: &Path) -> Result<(), StorageError>` — creates missing directories with mode 0700 on unix (each component it creates), and refuses (`StorageError::Invalid`) a path whose final component is a symlink. On Windows it only creates.
- `DataDir` creates nothing at resolve time.

`collection.rs`:
- `pub struct CollectionId(String)` — `as_str()`, `Display`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Debug`. Valid IDs match `^[A-Za-z0-9_-]{8,64}$`, so an ID is always safe as one path component.
- `pub fn read_collection_id(root: &Path) -> Result<Option<CollectionId>, StorageError>` — reads top-level `id` from `collection.toml`. Missing key is `Ok(None)`; a non-string or invalid value is `StorageError::Invalid { key: "id", .. }`.
- `pub fn ensure_collection_id(root: &Path) -> Result<CollectionId, StorageError>` — returns the existing ID, or generates a UUID v4 in simple form (32 lowercase hex), inserts `id = "..."` through `toml_edit` and writes with `atomic_write`. Comments, key order and every other key survive. Never replaces an existing valid ID.

## Acceptance
- [ ] AC1 `location` returns the override when it is absolute, ignores a relative override, returns `<exe>/data` with the marker, and `<os>/wisp` otherwise; `None` when no dir is known.
- [ ] AC2 `history_dir` and `cookies_file` build exactly the paths in the Contract.
- [ ] AC3 On unix, `ensure_private_dir` creates nested dirs with mode 0700 and errors on a symlinked final component.
- [ ] AC4 `ensure_collection_id` on a `collection.toml` with comments and `format = 1` adds one `id` line, keeps the comments byte-for-byte, and a second call returns the same ID without writing (mtime and bytes unchanged).
- [ ] AC5 `read_collection_id` gives `Ok(None)` for no key, `Ok(Some)` for a valid key, and `Invalid { key: "id" }` for `id = 5`, `id = "../x"` and `id = "short"`.
- [ ] AC6 After `ensure_collection_id`, `save_collection_defaults` with changed vars keeps the `id` line, and `load_collection_defaults` still loads (an `id` key is not an error).
- [ ] AC7 A collection without `collection.toml` gets one holding only `format = 1` and `id` (the same shape `save_collection_defaults` would write).
- [ ] AC8 Nothing in this task writes outside the given root: no test touches the real OS data dir (tests use `DataDir::at` and `location`, never `resolve` with the real env).
- [ ] AC9 `fmt` and `clippy -D warnings` clean.

## Checks
- `data-dir-unit` command — AC1, AC2, AC3, AC4, AC5, AC6, AC7. cargo test writes no JUnit XML here, so the command asserts the count (at least 12 tests across the two filters).
  run: `n=$(cargo test -p wisp-storage --lib -- data_dir:: collection::tests::collection_id 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 12`
- `storage-suite` command — AC6 (no regression)
  run: `cargo test -p wisp-storage`
- `lint` command — AC9
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `no-real-dir` read — AC8
  how: no test calls `DataDir::resolve()` or reads `HOME`/`APPDATA`/`LOCALAPPDATA`/`XDG_*` other than through `location` arguments.

## Out of scope
- Using the data dir (task 93) or the cookie file (wave 2).
- Changing `crates/app/src/settings.rs`, including the macOS in-bundle portable residual (PLAN.md Residuals (73)).
- Two folders copied from one another sharing an ID; detection is not in 0.2.0.
- Adding `id` to `CollectionDefaults` in `wisp-core`.

## Notes
- Name the collection-ID tests with the prefix `collection_id` in `collection::tests` so the check filter finds them.
- Size: small (no new crates linked beyond `uuid`, already in the binary through `core`).

## Review round 1 — settled without code

Reviewed HEAD: 1b25b15. There were no P1/P2 code findings.
- [SPEC] docs/architecture.md:129 says "Identity is the file path. No UUIDs in files", which conflicts with the collection `id`. This is settled by the user's decision of 2026-11-09 (spec 2026-11-09-wisp-0-2-0.md:15, :71: a random ID in collection.toml). The scribe amends architecture.md at record: file paths stay the identity of requests and folders, and from 0.2.0 `collection.toml` carries a random collection `id`.

## Residual (follow-ups)
- [P3] `ensure_collection_id` (collection.rs:242-257) has a read-modify-write race: two instances adding an ID at once means the last rename wins. `save_collection_defaults` has the same window.
- [P3] `ensure_private_dir` (data_dir.rs:92-101): a trailing slash (e.g. `WISP_DATA_DIR="/x/link/"`) gets past the symlink refusal on macOS. Normalise before the lstat.
- [NOTE] Hand-written IDs that differ only in case collide on case-insensitive file systems. Consider requiring lowercase.
- [NOTE] An existing data dir wider than 0700 is not tightened (task 93 may care).

## Outcome
Merged via PR "92: per-user data dir and collection id". tester PASS (428 tests, no flakes), reviewer APPROVE (AC1-9). The residuals above moved to PLAN under "Follow-ups from task 92". architecture.md and the v1 spec were amended at record for the collection `id`.
