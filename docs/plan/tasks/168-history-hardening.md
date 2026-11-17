# 168 — History hardening and a read-only history reader (2C, H2)

**Repo:** .
**Round:** 0
**Depends on:** 161, 165
**Owns:**
- crates/storage/src/history.rs
- crates/core/src/redact.rs
- crates/storage/tests/history.rs

## Goal
Close H2: history masking is linear, stored request bodies are capped, the
credential-name heuristic stops matching ordinary words, and response header
names no longer share the request headers' `redacted` namespace. Add
`HistoryStore::open_read_only` so `snapshot` can list recent entries without
migrating, locking or writing anything.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" H2 row (:418), Feature 5 `recent` (:372-374), Waves 2C (:469).
- docs/plan/tasks/99-hardening-0-2-0.md AC12 :53, AC15 :56, AC49 :62 (line refs are 0.2.0; `mask_bare_refs` is now `history.rs:540`).
- docs/plan/PLAN.md:264 (62-n1), :265 (62-n2).
- crates/storage/src/history.rs:25 `BODY_CAP`, :540 `mask_bare_refs`, :871 `open` (migrates, creates lock and marker files), :993 `lock_file`, :1035-1048 append cap, :1214 `list`.
- crates/core/src/redact.rs:5-35 fragments, `is_credential_name`, `contains_credential_fragment`; users: `storage/src/import/mod.rs`, `storage/src/history.rs`, `core/src/engine.rs`, `core/src/resolve/report.rs`.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 168 history-hardening`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-168`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (AC49) `mask_bare_refs` scans forward over variable-grammar characters only and then requires `}}`: one linear pass with no per-`{{` allocation. Test: a 1 MiB JSON body of repeated `{{` is stored (no wall-clock assert).
- [ ] AC2 (AC15) Restoring masked refs is a single pass. Test: 10,000 refs in a 1 MiB body round-trip correctly (linearity proved by reading the code).
- [ ] AC3 (AC12) The request body in `.req.json` is cut to `BODY_CAP` with a `truncated` flag (serde default false). Test: a 2 × `BODY_CAP` request body is stored cut with `truncated: true`; an old `.req.json` without the flag loads.
- [ ] AC4 (62-n1) Credential fragments match at name-token boundaries (tokens split on non-alphanumerics and lower-to-upper case changes). Table test: `X-Bypass-Cache`, `X-Assignment-Id`, `design`, `signal`, `compass` are not credential names or fragments; every row of the existing `redact.rs` tests still holds, and `apikey`, `accessToken`, `authToken`, `x-api-key`, `client_secret`, `DPoP` are credential names.
- [ ] AC5 (62-n2) Redacted response header names no longer land in the names the app treats as redacted request parts (`HistoryEntry.redacted`, read on resend at `crates/app/src/workspace.rs:1042`, `:1226`); they are kept apart in an additive, serde-default form. Test: a request with an empty-valued `X-Foo` header and a response with a redacted `X-Foo` header gives an entry whose request-side list has no `X-Foo`; a 0.3.0 index line still loads. The app's existing resend tests pass unchanged.
- [ ] AC6 `HistoryStore::open_read_only(dir) -> Option<ReadOnlyHistory>` with `list(limit)` never migrates, takes no lock and never creates, renames or writes a file. With no current scrub marker it returns an empty list and a "history not yet migrated; open Wisp once" note. Tests: (a) an unmigrated dir gives the note and its listing, sizes and mtimes are identical before and after; (b) a migrated store lists its newest entries up to `limit` and nothing in the dir changes.
- [ ] AC7 A test outside Owns that fails because of AC4 is not edited: stop and report it. No file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `history-h2` command — AC1, AC2, AC3, AC5, AC6
  run: `cargo test -p wisp-storage --locked --test history h2_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([7-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (prefix the new tests in `tests/history.rs` with `h2_`.)
- `redact` command — AC4
  run: `cargo test -p wisp-core --lib --locked redact:: 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC7
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/history.rs$' -e '^crates/core/src/redact.rs$' -e '^crates/storage/tests/history.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `linear` read — AC1, AC2, AC6
  how: the reviewer confirms both passes are single forward scans and that `open_read_only` calls no write, rename, create or lock API.

## Out of scope
- The `delivery` field (161); `snapshot` itself (173); importer behaviour (170), even where AC4 changes which names importers treat as secret.
