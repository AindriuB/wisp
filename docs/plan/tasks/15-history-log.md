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

## Review round 1 — findings

Reviewed HEAD: 2ecf88b2c626afea9d3bee5b2464094c63df952f

- [P2] crates/storage/src/history.rs:111 — `read_all` reads the whole log with `fs::read_to_string`, so one line with invalid UTF-8 makes it fail with `Io { InvalidData }` for the entire log, when the task says it "skips malformed lines".
  route: implementer — read the file as bytes, split on `b'\n'`, and `serde_json::from_slice` each line.
  input: append `/1`, then raw bytes `\xff\xfe\n`, then `/2`, then `read_all()` → `Err(Io { kind: InvalidData })` instead of `Ok` with 2 entries. Reachable after a crash mid-`write_all` that cuts a non-ASCII preview.
- [P3] (optional) crates/storage/src/history.rs:203 (`preview_is_bounded_and_valid`) — passes even with the char-boundary loop removed (`é` is 2 bytes, 1024 is even).
  route: implementer — add a case with a 3-byte char such as `"€".repeat(500)`.
  input: delete lines 52-54 (the `while` loop) → the test still passes.
- [P3] (optional) crates/storage/src/history.rs:50 — `preview_from` decodes the whole body with `from_utf8_lossy(bytes).into_owned()` before cutting it.
  route: implementer — decode only `&bytes[..bytes.len().min(PREVIEW_LIMIT)]` and keep the boundary cut.
  input: a 100 MB binary body → a temporary String of up to ~300 MB.
- [STYLE] crates/storage/src/history.rs:142,152,191,177,203 — test names are labels, not sentences: `appends_in_order`, `writes_gitignore`, `rotates_past_limit`, `url_template_kept`, `skips_malformed_line`.
  route: implementer
  rule: docs/conventions.md "Tests": "Test names read as sentences".
- [P3] (optional) crates/storage/src/history.rs:100-105 — if the log ends with a torn line without a trailing newline, the next appended entry joins onto it and is lost as malformed. Check for a trailing newline before appending.
  route: implementer
