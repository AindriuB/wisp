# 14 — Add the collection file watcher

**Repo:** .
**Depends on:** 10
**Owns:**
- crates/storage/src/watcher.rs

## Goal
Add a watcher over an open collection folder that emits `Changed { path }`
whenever a relevant file is created, modified, removed or renamed. It feeds M2's
"reload silently if clean, banner if dirty" behaviour. Events arrive on an
executor-agnostic channel, the same style as the engine's.

## Context
- crates/storage/src/lib.rs — `TEMP_PREFIX` and `STATE_DIR` (from 10)
- crates/core/src/engine.rs:16 — the `async_channel` receiver style to match
- docs/plan/specs/2026-10-29-http-client-v1.md — section 3 ("a watcher emitting `Changed(path)`") and section 4 (file watcher behaviour)

## Shape
- `Changed { path: PathBuf }`, where `path` is relative to the collection root. It derives `Debug, Clone, PartialEq, Eq`.
- `Watcher::start(root: &Path) -> Result<(Watcher, async_channel::Receiver<Changed>), StorageError>`. The root is canonicalised first, because macOS temp dirs are reached through a symlink. Dropping the `Watcher` stops watching and closes the channel.
- **Filtering.** Ignore paths under `.git/` or `STATE_DIR`, any file whose name starts with `TEMP_PREFIX`, and directories themselves. A rename reports the destination and, when notify provides it, the source.
- **Coalescing.** Repeated events for the same path within 100 ms produce one `Changed`. The coalescing happens on the watcher's own thread, never in the caller.

## Acceptance
- [ ] Each of these tests runs in a `tempfile` directory and waits on the channel with a bounded deadline of at most 5 s. None uses a fixed sleep to wait for the result.
  - Creating `users/get-user.toml` yields `Changed { path: "users/get-user.toml" }`.
  - Modifying the file yields one `Changed` for it, even though the write produces several raw events.
  - Deleting the file yields `Changed` for it.
  - Writing `.wisp/history.jsonl` and `.wisp-tmp-x` yields no `Changed` before a later sentinel file's `Changed` arrives.
  - After the `Watcher` is dropped, the receiver reports closed.
- [ ] `crates/storage/Cargo.toml` is unchanged, and notify features are as set by 10.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Telling the app's own saves apart from external edits. M2 compares content.
- Reload logic and banners (M2)
- Watching more than one collection
