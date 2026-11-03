# 57 — Stream File and Multipart request bodies; move spill writes off the async worker

**Repo:** `.`
**Depends on:** 56
**Owns:**
- crates/core/src/engine.rs
- crates/core/src/body.rs (only if the change needs it; keep its public API)
- crates/core/tests/engine.rs
- crates/core/Cargo.toml (`[dependencies]` feature lists only)
- Cargo.toml (the `reqwest` and `tokio` lines in `[workspace.dependencies]` only)
- Cargo.lock

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-57`.

**Binary-size impact uncertain.** reqwest's `stream` feature and tokio's `fs` feature may be needed. `futures-util` and `tokio-util` are already in Cargo.lock, but the delta has not been measured.

## Goal
M3 (v1 hardening). Closes the PLAN.md carry-forward "Engine blocking I/O (16)". Today `File` and `Multipart` bodies are read fully into memory before sending, and `BodyWriter::write` does blocking spill-file writes on the async worker. Large uploads should stream from disk. Response-body spill writes must not block a tokio worker.

## Context
- crates/core/src/engine.rs:279-289 `read_file` (whole file into a `Vec`), :305-336 `build_multipart`, :383-395 the body match in `perform`, :419-440 the `writer.write` / `writer.finish` loop.
- crates/core/src/engine.rs:164-189 — the runtime is always `new_multi_thread` with 1-2 workers (`clamp_threads`), so `block_in_place` is available. Using `spawn_blocking` is also acceptable.
- crates/core/src/body.rs:235-300 — `BodyWriter::write`/`finish`; memory up to the limit, then the spill file.
- crates/core/tests/engine.rs:89 (spill), :116 (missing body file → `BodyFile` naming the path), :132 (multipart boundary and both parts) — they must keep passing unchanged in substance.
- Cargo.toml:18 `reqwest = { version = "0.13.5", default-features = false, features = ["rustls-no-provider", "http2"] }`; :21 tokio features `rt-multi-thread, sync, time, macros`.
- docs/conventions.md#dependencies — `default-features = false`, justify the size. TLS stays rustls-only.

## Acceptance
- [ ] If a `File` body or a whole `Multipart` body is ≤ 1 MiB, it is still sent from memory, so a 307/308 redirect still resends it. A new test pins this: a server that answers 307 to a second path receives the same file body there.
- [ ] Larger bodies are streamed from disk in chunks of at most 64 KiB, with an explicit `Content-Length` equal to the file size (File) or the exact multipart total (Multipart), never `Transfer-Encoding: chunked`. New tests send a 20 MiB File body and a Multipart body holding a 20 MiB file part. The server checks `Content-Length` and the sha256 of the bytes received.
- [ ] A missing or unreadable body file still fails with `ErrorKind::BodyFile` naming the path, before any connection is made. The existing test passes.
- [ ] `rg -n 'std::fs::read\(' crates/core/src/engine.rs` is empty.
- [ ] Spill-file writes do not run directly on a tokio worker. Every call that can write to the spill file (`write` once spilling, `finish`) runs inside `block_in_place` or `spawn_blocking`. A short comment states which, and why.
- [ ] Public API unchanged: `git diff main -- crates/app crates/storage` is empty and `wisp-app` builds.
- [ ] `cargo tree -p wisp-app -e normal | rg -i 'openssl|native-tls|aws-lc'` prints nothing. Any package Cargo.lock gains is listed in the report with a reason.
- [ ] Release binary (`target/wt-57/release/wisp`): report the size and its delta from the task-56 merge. If the delta exceeds 200,000 B, or the total exceeds 20,971,520 B, stop and report BLOCKED, naming the features that caused it.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean. The new 20 MiB tests pass 5 consecutive runs.

## Out of scope
- Upload progress events, or any change to the `Event`/`Command` types.
- `ClientCache` locking, root parsing and `install_crypto_provider` (task 56).
- The body-store test residuals (uid check, real temp dir, non-unix spill dir).
- Redirect following for streamed bodies over 1 MiB (reqwest cannot replay a stream). Note it in the report for the scribe.

## Orchestrator decision (2026-11-02)

User decision: special files whose metadata does not give the real size are an accepted residual. These are /proc-style entries that report size 0, FIFOs and /dev/stdin. Today they are sent empty or refused with "not a regular file". The clear "not a regular file" error stays as it is. Rejected: a read-to-EOF fallback.

## Review round 1 — findings

- [P3] crates/core/src/engine.rs:519-524, :428-430 — a user-set Content-Length or Transfer-Encoding header is passed through alongside the engine's own Content-Length. hyper then switches a streamed body to chunked, which breaks the "never `Transfer-Encoding: chunked`" acceptance item. Input: POST with header `Content-Length: 5` and a 20 MiB `PreparedBody::File`. That gives two Content-Length values that disagree; hyper's `content_length_parse_all` returns None and `set_length` sends chunked. Fix: when the engine sets the body length, skip user `content-length` and `transfer-encoding` headers, the same way `content-type` is already skipped for multipart at :519-522. Add a test.
- [P3] crates/core/src/engine.rs:378 — the comment "Two chunks in flight bounds memory at about 128 KiB" understates it. The real bound is about 256 KiB: two queued chunks, the sender's 64 KiB buffer and the chunk hyper is writing. Fix the figure or drop it.
- [NOTE, optional] engine.rs:393 — `ErrorKind::Interrupted` on `file.read` aborts the upload instead of retrying.
