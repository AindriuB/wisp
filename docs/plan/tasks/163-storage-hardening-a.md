# 163 — Storage hardening A: id race, private dir, cookie parse error (1D, H5, AC51)

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/storage/src/collection.rs
- crates/storage/src/data_dir.rs
- crates/storage/src/cookies.rs
- crates/storage/tests/cookies.rs
- crates/storage/tests/create.rs

## Goal
Close hardening theme H5 (AC4, AC5, AC7) and the storage half of AC51: the
collection id and defaults cannot race each other, the private-dir check cannot
be bypassed by a trailing slash and tightens a too-wide dir, and a corrupt cookie
file's error never carries file content.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" table (:416-426; the AC4 note :426: no cross-process lock needed in 0.4.0), Waves 1D (:460).
- docs/plan/tasks/99-hardening-0-2-0.md AC4 :43, AC5 :44, AC7 :46, AC51 :98 (line references there are 0.2.0; re-locate).
- crates/storage/src/collection.rs:177 `save_collection_defaults`, :236 `read_collection_id`, :248 `ensure_collection_id`.
- crates/storage/src/data_dir.rs:91 `ensure_private_dir`.
- crates/storage/src/cookies.rs:197 `load` (the JSON parse error path).
- The app's call site for the cookie error (`workspace.rs:3581-3586`) is 161's; this task only changes what the storage error says.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 163 storage-hardening-a`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-163`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (AC4) `ensure_collection_id` and `save_collection_defaults` serialise their read-modify-write under one in-process lock keyed by the canonical root. Test: for each of 50 fresh roots, two threads released by one barrier call `ensure_collection_id`; both return the same id and `collection.toml` holds it.
- [ ] AC2 (AC4) Test: `ensure_collection_id` and `save_collection_defaults` racing on one root leave both the id and the new defaults in `collection.toml`.
- [ ] AC3 (AC5) `ensure_private_dir` strips trailing separators before the `symlink_metadata` check. Unix test: `link/` pointing at a directory is refused; `link` is refused as before.
- [ ] AC4 (AC7) On unix, an existing directory wider than 0700 is set to 0700 by `ensure_private_dir`; a test creates it 0755 and reads back 0700. A path that exists but is not a directory is an error.
- [ ] AC5 (AC51 storage half) A cookie file that fails to parse gives an error whose text is a fixed message plus line and column only (for example "cookie file is not valid JSON (line 3, column 7)"), never serde's message or any file byte. Test: a file with `CANARY_COOKIE` inside a broken value gives an error whose `Display` and `Debug` contain neither the canary nor any quoted file text.
- [ ] AC6 No public signature changes in the three files; no file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `storage-h5` command — AC1, AC2, AC3, AC4
  run: `cargo test -p wisp-storage --locked h5_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([4-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (prefix the AC1-AC4 test names with `h5_`.)
- `cookie-error` command — AC5
  run: `cargo test -p wisp-storage --locked --test cookies parse_error 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `scope` command — AC6
  run: `git diff --exit-code main...HEAD -- crates/app crates/core crates/cli crates/storage/src/history.rs crates/storage/src/fs.rs`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Out of scope
- A cross-process id lock (spec :426); the Windows ACL and `same_file_entry` (171, which also edits `data_dir.rs` after this task).
- The workspace's cookie log and notice (161).
