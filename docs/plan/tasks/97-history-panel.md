# 97 — Add the history panel, Clear history and Resend

**Repo:** .
**Depends on:** 93, 96
**Owns:**
- crates/app/src/ui/history.rs (new)
- crates/app/src/ui/mod.rs (one `pub mod history;` line)
- crates/app/src/workspace.rs (history open, clear and resend methods)
- crates/app/src/tab.rs (a from-history marker on a tab)
- crates/app/src/settings.rs (one `history_bodies` setting)
- crates/app/src/ui/response/mod.rs (one "From history" line)
- crates/app/src/actions.rs, crates/app/src/palette.rs, crates/app/src/ui/palette.rs, crates/app/src/ui/root.rs (new commands and the panel slot only)

## Goal
A user can look back at what was sent: a history panel lists entries for the open
collection, opening one shows the stored request and response, Resend sends it
again after re-resolving against the current environment and says so, and
Clear history empties it. A setting stops response bodies being stored.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#2-history-storage`
- `crates/storage/src/history.rs` — `HistoryStore::{list, load, clear}`, `HistoryEntry`, `redacted`, `body_truncated` (task 93)
- `crates/app/src/workspace.rs` — history write path and Notice (task 93), `new_scratch_tab`, `send`, `scope_for`
- `crates/core/src/body.rs:152-280` — `BodyStore::writer()` to load stored bytes (at most 1 MiB) into a `BodyHandle`
- `crates/app/src/settings.rs:1-40,150-200` — settings fields and `toml_edit` save
- `crates/app/src/ui/root.rs` — overlay slot and command added by task 96 (same pattern)
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/97-history-panel`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-97`

## Contract
| Piece | Behaviour |
|---|---|
| Show history | Command "Show history" toggles a panel listing entries newest first: method, URL template, status or outcome ("Failed: <kind>", "Cancelled"), relative time. Loaded off the UI thread. Empty state "No history yet"; disabled state shows the reason from task 93 |
| Open entry | Opens a new tab holding the stored request and, read-only, the stored status, headers and body (loaded through `BodyStore::writer()`). The response pane shows "From history, <time>" and, when `body_truncated`, "Body truncated at 1 MiB"; when bodies were not stored, "Body not stored" |
| Resend | Sending a from-history tab re-resolves its request against the current environment through the normal `send` path. A Notice reads "Resent with the current environment (<env or 'no environment'>)" and, when the entry's `redacted` list is non-empty, "; redacted values were not stored: <names>". Unresolved variables block it with the existing message |
| Clear history | Command "Clear history" asks for confirmation naming the collection, then calls `clear` off the UI thread and empties the panel |
| Setting | `history_bodies` (default true) in settings.toml; command "Toggle storing response bodies in history"; when false, task 93's append gets no body |

## Acceptance
- [ ] AC1 Pure helpers (row label for each outcome, relative time, resend notice text with and without redacted names and environment) have unit tests.
- [ ] AC2 `history_bodies` round-trips through settings load/save with `toml_edit` (other keys and comments kept) and defaults to true when absent.
- [ ] AC3 Opening an entry creates a tab whose response shows the stored status, headers and body; the stored bytes are read off the UI thread.
- [ ] AC4 Resend goes through the existing `send` and resolve path (no stored resolved request is sent), and emits the Notice in the Contract.
- [ ] AC5 Clear history requires confirmation and runs `clear` off the UI thread.
- [ ] AC6 With `history_bodies = false`, a new entry has `body_stored = false` and no `.bin` file (test at the workspace helper level or storage level).
- [ ] AC7 `fmt`, `clippy -D warnings` and the workspace test suite pass.

## Checks
- `history-ui-unit` command — AC1, AC2, AC6. cargo test writes no JUnit XML here; asserts at least 8 passing.
  run: `n=$(cargo test -p wisp-app -- ui::history settings::tests::history 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 8`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `wiring` read — AC3, AC4, AC5
  how: list/load/clear run in `background_spawn`; Resend calls the existing `send`; Clear has a confirm prompt; `workspace.rs` gains only history methods.
- `hand-check` read — AC3, AC4, AC5
  how: the PR notes a macOS run with `WISP_DATA_DIR` set to a temp dir: send two requests (one failing), open each from the panel, Resend one after switching environment, toggle the body setting, Clear history; the app was stopped by its own pid.

## Out of scope
- History storage format, caps or redaction (task 93); report bugs there, do not fix them here.
- Timings in the panel (wave 2), search or filtering, per-entry delete.
- Multiple collections (wave 3).

## Notes
- Size: small (no new crates).
- Launching the app: record the pid you started and stop only that pid; never `pkill`/`killall`. A bundled build uses bundle id `io.github.aindriub.wisp.t97`.

## Amendment 1

User decisions 2026-11-10. These add AC8–AC10; earlier ACs keep their IDs.

- **Resend scope.** A tab opened from history carries the entry's original request path (`HistoryEntry.request`) in its history origin. Resend uses it only as the scope path for resolving inherited defaults (folder auth, headers, vars). The tab stays pathless, so Save still creates a new request. If that path no longer exists, Resend falls back to collection defaults plus the environment and says so in the Resend notice.
- **Body display.** An opened entry's response body is shown read-only through a file-backed `BodyHandle` on the stored body file (spec `docs/plan/specs/2026-11-09-wisp-0-2-0.md:52`), not copied through `BodyStore::writer()`. This supersedes the Context line for `crates/core/src/body.rs:152-280`. Releasing the tab must not delete the history body file.
- **No id on read-only use.** Opening the panel, listing, opening an entry and Clear history never mint a collection id. With no `id` in `collection.toml` the panel shows "No history yet", Clear has nothing to clear, and nothing is written. The id is still created by the first send's append.

Acceptance:
- [ ] AC8 With `folder.toml` setting bearer auth `{{token}}` and a request whose auth is inherit, Resend from that request's history tab sends `Authorization: Bearer <token value>`; with the original path removed, Resend uses collection defaults plus env and the notice says the original request was not found. Test `resend_from_history_inherits_folder_auth` (plus `resend_from_history_missing_path_falls_back`).
- [ ] AC9 Opening an entry with a stored body yields a file-backed `BodyHandle` on the history `.bin` file with no `BodyStore::writer()` call in the open path; after the tab's body is released the `.bin` file still exists. Test `open_history_entry_body_is_file_backed`.
- [ ] AC10 Opening the panel (list), opening an entry and Clear history on a collection with no `id` in `collection.toml` leave `collection.toml` byte-identical and show "No history yet". Test `history_read_only_use_mints_no_id`.

Checks:
- `amendment-1-tests` command — AC8, AC9, AC10. cargo test writes no JUnit XML here; asserts at least 4 passing.
  run: `n=$(cargo test -p wisp-app -- resend_from_history_ open_history_entry_body_is_file_backed history_read_only_use_mints_no_id 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 4`
- `amendment-1-shape` read — AC8, AC9
  how: Resend passes the origin's request path to `scope_for`, not `None`, and the tab's own path stays `None`; the history open path builds the body handle from the file and has no `writer()` call; `HistoryJob::open` no longer calls `ensure_collection_id`.

## Review round 1 — findings

Reviewed HEAD 0c19362.

- [P2] `crates/app/src/workspace.rs:526` Resend uses `scope_for(None)`, dropping folder defaults — resolved by Amendment 1 (Resend scope).
- [SPEC] `crates/app/src/workspace.rs:807` body copied via `writer()` — resolved by Amendment 1 (Body display).
- [P3] `crates/app/src/workspace.rs:79` `HistoryJob::open` calls `ensure_collection_id` on read-only use — resolved by Amendment 1 (No id on read-only use).
- [P3, optional] `crates/app/src/ui/history.rs:143` the panel reloads only on `HistoryChanged`; `open_collection` doesn't emit it, so an open panel lists the old collection's entries and clicking a row errors "not found". Emit `HistoryChanged` (or reload) on collection open.
