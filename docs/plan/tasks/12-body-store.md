# 12 — Add the body store with spill-to-disk and BodyHandle

**Repo:** .
**Depends on:** 10
**Owns:**
- crates/core/src/body.rs

## Goal
Response bodies stop being `Vec<u8>` in events. They become a plain-data
`BodyHandle` into a `BodyStore`. The store holds a body in memory up to a
configurable limit and spills it to a temp file once it grows past that. It
serves byte ranges, so the UI can page through the body, and it frees the body
when asked or when the store drops. Task 16 wires it into the engine.

## Context
- crates/core/src/body.rs — the empty module created by 10
- docs/architecture.md — "The Engine" (bodies) and "Boundaries" (temp files must be removed on close)
- docs/conventions.md#errors — `thiserror`, and no `unwrap` outside tests

## Shape (the contract consumed by 16 and M2)
- `BodyId(u64)` and `BodyHandle { id: BodyId, len: u64 }` derive `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`. Neither holds a lock or a file handle.
- `pub const DEFAULT_MEMORY_LIMIT: u64 = 10 * 1024 * 1024;`
- `BodyStore` is `Clone`, with shared state held internally. Its methods are `new(memory_limit: u64)`, `writer(&self) -> BodyWriter`, `read(&self, &BodyHandle, offset: u64, len: usize) -> Result<Vec<u8>, BodyError>`, `is_spilled(&self, &BodyHandle) -> bool` and `release(&self, &BodyHandle)`.
- `BodyWriter` has `write(&mut self, &[u8]) -> Result<(), BodyError>` and `finish(self) -> Result<BodyHandle, BodyError>`. Dropping it without calling `finish` discards its data and its temp file. That is the cancel path.
- `BodyError` (`thiserror`): `Io(String)`, `UnknownHandle(BodyId)`.
- Spill files live in a per-user private directory and are named `wisp-<pid>-<uuid v4 simple>.body` (see Amendment 2026-10-30, which supersedes the earlier fixed `wisp-<pid>-<id>.body` in the shared temp dir). The first write that pushes the total past the limit moves the buffered bytes to the file. Every later write appends to it.

## Acceptance
- [ ] Unit tests in `body.rs` show the following:
  - A 1 KiB body under a 4 KiB limit is not spilled, and `read(h, 0, 1024)` returns it unchanged.
  - A body written in 3 KiB chunks to a total of 10 KiB, with a 4 KiB limit, is spilled. Its temp file exists. `read` at offsets 0, 4095 and 9000 returns the right bytes.
  - A `read` past the end returns the available tail. A `read` at `offset >= len` returns an empty vec.
  - After `release`, the temp file no longer exists and `read` returns `UnknownHandle`.
  - Dropping a `BodyWriter` that has spilled, without calling `finish`, removes its temp file.
  - Dropping the last `BodyStore` clone removes every remaining temp file.
  - (unix) A spilled body's file has mode `0o600` and its directory has mode `0o700`, checked with `std::os::unix::fs::PermissionsExt` (`mode() & 0o777`).
  - Two independent `BodyStore`s in one process (same pid, so the pid-reuse case) each spill a body; the two file names differ, both names match `wisp-<pid>-<32 hex chars>.body`, and both reads return their own bytes.
  - (unix) Spilling into a pre-existing spill directory with group/other permission bits set (e.g. `0o755`), or one that is not a directory (a regular file or a symlink at that path), fails with `BodyError::Io` and creates no file there. The test points the store at a test-owned base directory rather than the real temp dir.
- [ ] No new dependencies, and `crates/core/Cargo.toml` is unchanged.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

### Amendment (2026-10-30)
**Decision (user, 2026-10-30).** Response-body spill files go into a per-user private directory, not the shared temp dir.
- Directory: `<std::env::temp_dir()>/wisp-<uid>` on unix; `<std::env::temp_dir()>/wisp` on Windows, where `%TEMP%` is already per-user.
- Unix: if the directory is absent, create it with mode `0o700` (`std::os::unix::fs::DirBuilderExt::mode`). If it exists, inspect it with `symlink_metadata` (do not follow symlinks): it must be a directory, owned by the current uid (`MetadataExt::uid`), with `mode() & 0o077 == 0`. Otherwise the spill fails with `BodyError::Io` and the directory is not used. No permission checks on Windows.
- Files: `wisp-<pid>-<uuid v4 simple>.body` (`Uuid::new_v4().simple()`), opened with `OpenOptions::create_new(true)` and on unix `OpenOptionsExt::mode(0o600)`.
- No new dependency. `uuid` with `v4` is already a dependency of `wisp-core`. `std` exposes no `getuid`, and `libc` is not a direct dependency, so do not add it. Get the current uid from the metadata of a file this process has just created: a `create_new` probe file in `temp_dir()` named with a uuid, removed straight after.
- The base directory must be injectable for tests, e.g. a crate-private `BodyStore::with_spill_base(memory_limit, base: PathBuf)` used by `new`. The public Shape signatures above do not change.
- Unchanged: the in-memory/spill threshold, `BodyHandle`/`BodyId`, `BodyWriter` drop/cancel, `release`, and cleanup on dropping the last store. The per-user directory is never removed. Only the store's own files are.

**Reason (code review of task 12).** The predictable name in shared `/tmp` caused three problems. A file left behind by a crash, plus pid reuse, made the first spill fail with `AlreadyExists`. Another local user could pre-create names and block downloads. Spill files were world-readable (`0644` under umask `022`), which exposed response bodies on multi-user Linux.

## Out of scope
- Engine integration, `EngineConfig` and events (16)
- Any limit on total memory across bodies, eviction, or compression
- UI paging

## Review round 1 — findings

Reviewed HEAD: 5c6fffe2750de56b069484fa7ebb5e077e7935dc

- [P1] crates/core/src/body.rs:75-77, 166-170 — the 2026-10-30 Amendment is not implemented: spill files still go into the shared `std::env::temp_dir()` with the predictable name `wisp-<pid>-<id>.body`, opened with default permissions.
  route: implementer — implement the Amendment exactly (per-user `wisp-<uid>` dir 0o700 checked with `symlink_metadata`/uid/`& 0o077`; `create_new` + `mode(0o600)`; `wisp-<pid>-<uuid simple>.body`; crate-private `with_spill_base`; its acceptance tests). Update the stale comment at body.rs:45 about ids preventing name collisions.
  input: multi-user Linux, umask 022 → `/tmp/wisp-<pid>-N.body` is 0644 and readable by other users; pre-created names block spills with AlreadyExists.
- [P3] crates/core/src/body.rs:164-176 — `write` adds to `self.len` before the spill file is opened/written; a failed spill leaves `len` counting bytes never stored.
  route: implementer — increment `len` only after the data is stored (or mark the writer failed so later write/finish return an error).
  input: limit 10, `write(&[0;5])` ok, `write(&[0;10])` fails on spill open, then `finish()` → handle len 15, store holds 5.
- [P3] (optional) crates/core/src/body.rs:121-127 — a `release` racing `read` between path clone and `File::open` returns `Io(NotFound)`; map NotFound to `UnknownHandle`.
  route: implementer
