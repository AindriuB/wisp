# 93 — Rewrite history into the per-user data dir

**Repo:** .
**Depends on:** 92, 95
**Owns:**
- crates/storage/src/history.rs (full rewrite)
- crates/storage/src/error.rs (insertions only)
- crates/storage/tests/history.rs (new)
- crates/app/src/workspace.rs (the history write path only: `Sent`, `append_history`, the terminal-event call site, and opening the store with the collection)

## Goal
History moves out of the collection into `history/<collection-id>/` in the
per-user data dir, stores the request (auth redacted) and the response body
(capped), keeps failed requests, and evicts by count and bytes. The old
`.wisp/history.jsonl` is no longer written or read and is left on disk.
The app writes entries through the new store; the panel, Clear and Resend UI
are task 97.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#2-history-storage`
- `crates/storage/src/history.rs:1-140` — current entry, torn-line recovery (`ends_mid_line`) to keep
- `crates/storage/src/data_dir.rs` — `DataDir::history_dir`, `ensure_private_dir` (task 92)
- `crates/storage/src/collection.rs` — `ensure_collection_id` (task 92)
- `crates/core/src/request.rs` — `Auth::redacted()` (task 95)
- `crates/app/src/workspace.rs:41-46,560-665` — `Sent`, terminal-event handling, `append_history`; `WorkspaceEvent::Notice` for banners
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/93-history-store`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-93`

## Contract
Layout under `<data>/history/<collection-id>/`: `index.jsonl` (one JSON line per entry) and `bodies/<entry-id>.bin` (response body) and `bodies/<entry-id>.req.json` (`{ "request": <wisp_core Request>, "response_headers": [..] }`). Dirs 0700, files 0600 on unix.

| Item | Rule |
|---|---|
| Entry id | 32 lowercase hex, random (`uuid` v4 simple) |
| Index line | `id, at (unix ms), request (collection-relative path or null), method, url (unresolved template), outcome (Completed / Failed{kind} / Cancelled), status, ttfb_ms, total_ms, size, body_stored (bool), body_truncated (bool), redacted (Vec of field/header names)`; unknown fields ignored, missing optional fields default, so wave 2 can add `timings` |
| Stored request | the unresolved draft with `auth = auth.redacted()`; header values for `Authorization`, `Proxy-Authorization`, `Cookie`, `X-Api-Key`, `X-Amz-Security-Token` and the ApiKey auth's header name (case-insensitive) go through the same rule (a lone `{{var}}` is kept, anything else becomes `""`); every emptied name is listed in `redacted` |
| Body cap | at most 1 MiB (1,048,576 B) stored; longer bodies are cut and `body_truncated = true` |
| Count cap | 500 entries; on append beyond it the oldest go first, files and index line together |
| Bytes cap | 100 MiB per collection over `bodies/`; oldest evicted until under |
| Failed | Failed and Cancelled entries are kept (no body file) |
| Corrupt line | skipped on read; a torn final line is repaired before the next append (as today) |
| Compaction | eviction rewrites `index.jsonl` with `atomic_write_private` |

API (`HistoryStore`): `open(data: &DataDir, id: &CollectionId) -> HistoryStore` (no I/O); `append(&self, NewEntry, body: Option<&[u8]>) -> Result<HistoryEntry, StorageError>`; `list(&self) -> Result<Vec<HistoryEntry>, _>` newest first; `load(&self, entry_id) -> Result<StoredEntry, _>` (request, response headers, body bytes); `clear(&self) -> Result<(), _>` (removes the collection's history dir). `NewEntry` takes the draft `Request` and redacts itself, so no caller can skip redaction.

App (`workspace.rs`): on terminal events, read at most 1 MiB + 1 B of the body from the `BodyStore` and append on the background executor (not the UI thread). Resolve `DataDir` once at startup; call `ensure_collection_id` on the first append for a collection. If the data dir is unknown, cannot be created, or `ensure_collection_id` fails, history is disabled for that collection and one `WorkspaceEvent::Notice` says "History is off: <reason>". An append failure (for example disk full) drops the entry and emits a Notice once per collection.

## Acceptance
- [ ] AC1 Appending a request whose auth is Bearer `"abc"` and headers `Authorization: secret`, `X-Api-Key: {{key}}` stores a `.req.json` containing neither `abc` nor `secret`, keeps `{{key}}`, and lists `auth.token` and `Authorization` in `redacted`.
- [ ] AC2 A 2 MiB body stores exactly 1,048,576 bytes with `body_truncated = true`; a 10-byte body stores 10 with `false`.
- [ ] AC3 After 505 appends, `list` returns 500 entries, newest first, and the 5 oldest have no files left in `bodies/`.
- [ ] AC4 When stored bodies would exceed 100 MiB the oldest entries are evicted until under (the test lowers the caps through a `#[doc(hidden)] pub fn HistoryStore::with_caps(..)`, since `tests/history.rs` cannot reach `pub(crate)` items).
- [ ] AC5 A Failed entry is kept with its `ErrorKind` and no body file; `load` returns it with an empty body.
- [ ] AC6 A garbage line and a torn final line are skipped by `list`; the next append writes on a new line.
- [ ] AC7 `clear` removes the collection's history dir and leaves other collections' dirs.
- [ ] AC8 On unix, the history dir, `bodies/` and every file are 0700/0600.
- [ ] AC9 An index line with an extra unknown field and one missing `ttfb_ms` both parse.
- [ ] AC10 The app no longer writes `.wisp/history.jsonl` (no reference to the old `HistoryLog`, `PREVIEW_LIMIT` or `.wisp/history` remains outside the watcher's ignore of `STATE_DIR`), and the store append runs off the UI thread.
- [ ] AC11 When history is disabled or an append fails, a `WorkspaceEvent::Notice` is emitted, once per collection.
- [ ] AC12 `fmt`, `clippy -D warnings` and the workspace test suite pass.

## Checks
- `history-store` command — AC1-AC9. cargo test writes no JUnit XML here; asserts at least 12 passing.
  run: `n=$(cargo test -p wisp-storage --test history 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 12`
- `no-old-log` command — AC10
  run: `! rg -n 'HistoryLog|PREVIEW_LIMIT|preview_from|history\.jsonl' crates/app/src crates/storage/src/history.rs`
- `suite-and-lint` command — AC12
  run: `cargo test --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `app-wiring` read — AC10, AC11
  how: the append is inside `cx.background_spawn` (or equivalent); the Notice is emitted once per collection; `workspace.rs` changes are confined to the history write path and store setup.

## Out of scope
- The history panel, Clear history action, Resend, and the "do not store response bodies" toggle (task 97).
- Phase timings in entries (wave 2).
- Deleting or migrating old `.wisp/history.jsonl` files.
- Two Wisp processes appending to one collection at once.

## Notes
- Size: small; `serde_json` and `uuid` are already linked.
- Tests use `DataDir::at(tempdir)`; never the real data dir. If you launch the app, set `WISP_DATA_DIR` to a temp dir, stop only the pid you started (no `pkill`/`killall`), bundle id `io.github.aindriub.wisp.t93` if bundled.
