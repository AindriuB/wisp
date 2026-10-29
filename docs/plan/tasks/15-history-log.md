# 15 — Add the append-only request history log

**Repo:** .
**Depends on:** 10
**Owns:**
- crates/storage/src/history.rs

## Goal
Add the v1 history log: one line of request metadata, plus a truncated response
body preview, per finished request. It is written inside the collection folder,
which keeps the portability rule, and it is kept out of git. There is no UI in
v1. The log must never contain secret values.

## Context
- crates/storage/src/lib.rs — `STATE_DIR` (from 10)
- crates/core/src/model.rs — `Method`, `ErrorKind` (store `ErrorKind` as an opaque serde value and do not match on its variants, because 16 edits that enum concurrently)
- docs/architecture.md — "Boundaries" (secrets never in history; nothing written outside the collection)
- docs/conventions.md#logging

## Shape
- `HistoryEntry { at: SystemTime, request: Option<String>, method: Method, url: String, outcome: Outcome, status: Option<u16>, ttfb: Option<Duration>, total: Option<Duration>, size: Option<u64>, preview: String }`, deriving serde, `Debug`, `Clone` and `PartialEq`
  - `request` is the request file path relative to the collection root.
  - `url` is the request's URL template before resolution, so `{{secret}}` stays literal. The caller passes `Request.url`, never `PreparedRequest.url`.
- `Outcome`: `Completed | Failed { kind: ErrorKind } | Cancelled`. It carries no error message, because reqwest messages embed the resolved URL.
- `pub const PREVIEW_LIMIT: usize = 1024;` and `HistoryEntry::preview_from(bytes: &[u8]) -> String`, which decodes lossy UTF-8, cuts on a char boundary and keeps at most `PREVIEW_LIMIT` bytes.
- `HistoryLog::open(root) -> HistoryLog` does no I/O. `append(&self, &HistoryEntry) -> Result<(), StorageError>` creates `<root>/.wisp/` and `<root>/.wisp/.gitignore` (containing `*`) on first use, then appends one JSON line to `.wisp/history.jsonl`. `read_all(&self) -> Result<Vec<HistoryEntry>, StorageError>` skips malformed lines.
- **Rotation.** When `history.jsonl` exceeds 4 MiB before an append, it is renamed to `history.1.jsonl`, replacing any older one.

## Acceptance
- [ ] Unit tests in `history.rs`, using `tempfile` directories, show the following:
  - Two appends followed by `read_all` return both entries in order.
  - `.wisp/.gitignore` exists and contains `*`.
  - Opening a log without appending creates nothing on disk.
  - `preview_from` on 5000 bytes of `é` returns at most 1024 bytes of valid UTF-8.
  - An entry with `url = "{{baseUrl}}/x?k={{token}}"` is written with the template, not a resolved URL.
  - A file pre-filled past 4 MiB rotates on the next append, leaving `history.1.jsonl` plus a new one-line `history.jsonl`.
  - A malformed line in the middle is skipped by `read_all`.
- [ ] No header value, auth field or `Environment` value is part of `HistoryEntry`.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Calling `append` from the app (M2)
- A history UI or query API beyond `read_all`
- Root `.gitignore` edits
