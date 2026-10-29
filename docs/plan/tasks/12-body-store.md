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
- Spill files are created with `create_new` in `std::env::temp_dir()` and named `wisp-<pid>-<id>.body`. The first write that pushes the total past the limit moves the buffered bytes to the file. Every later write appends to it.

## Acceptance
- [ ] Unit tests in `body.rs` show the following:
  - A 1 KiB body under a 4 KiB limit is not spilled, and `read(h, 0, 1024)` returns it unchanged.
  - A body written in 3 KiB chunks to a total of 10 KiB, with a 4 KiB limit, is spilled. Its temp file exists. `read` at offsets 0, 4095 and 9000 returns the right bytes.
  - A `read` past the end returns the available tail. A `read` at `offset >= len` returns an empty vec.
  - After `release`, the temp file no longer exists and `read` returns `UnknownHandle`.
  - Dropping a `BodyWriter` that has spilled, without calling `finish`, removes its temp file.
  - Dropping the last `BodyStore` clone removes every remaining temp file.
- [ ] No new dependencies, and `crates/core/Cargo.toml` is unchanged.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Engine integration, `EngineConfig` and events (16)
- Any limit on total memory across bodies, eviction, or compression
- UI paging
