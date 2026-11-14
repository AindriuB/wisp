# 120 — Refuse symlinked collection files on every read and write path

**Repo:** `.`
**Depends on:** none
**Size impact:** small (expected well under 20 KB on the Windows binary; no new dependency)
**Owns:**
- crates/storage/src/environment.rs (including its inline tests)
- crates/storage/src/vault.rs (including its inline tests)
- crates/storage/src/collection.rs (only `read_collection_id` and its inline tests)
- crates/storage/src/fs.rs (insertions only: a confined read helper if one is needed)
- crates/storage/tests/vault.rs
- crates/app/src/vault.rs (only `imported_names` and its call site at the ImportFile action, plus inline tests)

## Goal
A cloned or unzipped collection is untrusted content, and git can commit symlinks. Four paths still follow a symlink out of the collection: writing `environments/.gitignore`, reading a file-mode secrets file (load and "move to keychain"), reading `collection.toml` for the id, and listing the names of a secrets file before import. Each must refuse a symlinked or non-regular file before it opens it, as `fs::confined_regular_file` already does elsewhere. Carried from task 99 AC57, AC58, AC61, AC74.

## Context
- crates/storage/src/fs.rs:164-195 `confined_regular_file` (lstat every component; refuses links, non-dirs and non-regular files; missing is `Ok(false)`). crates/storage/src/vault.rs:711 `check_secrets_path` wraps it.
- crates/storage/src/environment.rs:165-180 `ensure_ignore_line` (used by `ensure_gitignore` and `ensure_marker_ignored`) reads and atomically rewrites `.gitignore`, following a link.
- crates/storage/src/environment.rs:110 (`load_environment`) and crates/storage/src/vault.rs:434-461 (`load_environment_with`, file mode) and :746-750 (`migrate_to_store`) read `<env>.secrets.toml` with no confinement check.
- crates/storage/src/collection.rs:236-242 `read_collection_id` reads through a link; `ensure_collection_id` (:245-247) already checks. Callers: crates/storage/src/vault.rs:637, crates/storage/src/vault_binding.rs:604, crates/app/src/vault.rs:579 and :961, crates/app/src/workspace.rs:253.
- crates/app/src/vault.rs:1184-1198 `imported_names` reads the secrets file before `import_secrets_file` runs its check; a link to `/dev/zero` or a FIFO blocks or exhausts memory.
- crates/storage/tests/vault.rs holds the existing cfg(unix) symlink refusal tests for the binding marker; follow them.

## Acceptance
- [ ] AC1 (99-AC57) `ensure_gitignore` and `ensure_marker_ignored` refuse, with `StorageError::Invalid` and no write, when `environments/.gitignore` is a symlink or not a regular file. A cfg(unix) test links `.gitignore` to a file outside the collection, saves an environment with secrets, and shows the save fails and the target's bytes are unchanged.
- [ ] AC2 (99-AC58) File-mode loads (`environment::load_environment` and `vault::load_environment_with`) and `vault::migrate_to_store` run the secrets-path confinement check before reading `<env>.secrets.toml`. A symlinked secrets file fails the load or migration with `Invalid` (fail closed: no secrets returned and nothing written to the store). A cfg(unix) test links `dev.secrets.toml` to an outside file holding `[vars] token = "SECRETVAL"` and shows the load errors and, for `migrate_to_store` against a `MemoryStore`, the store stays empty.
- [ ] AC3 (99-AC61) `read_collection_id` runs the same confinement check: a symlinked `collection.toml` returns an error and no id. A missing file still returns `Ok(None)`. A cfg(unix) test links `collection.toml` to another collection's file and shows the id is not returned.
- [ ] AC4 (99-AC74) The secrets-file names shown after an import are read only after the confinement check, so a symlinked or non-regular secrets file is never opened. `imported_names` either goes through a storage function that checks first or is replaced by names returned from the import. A cfg(unix) test shows that a secrets file that is a symlink yields no names and is not read (for example a link to a file outside the collection whose names do not appear).
- [ ] AC5 No public signature in crates/app/src/vault.rs that crates/app/src/workspace.rs calls is changed, and existing tests in crates/storage and crates/app pass.

## Checks
cargo writes no JUnit XML here, so these are command checks.
- `vault-it` command — AC1, AC2, AC3
  run: `cargo test -p wisp-storage --test vault`
  pass: exit 0, 0 failed, passed count at least the count on main 7ec7505 plus 3 (re-measure the base first; state both in the PR).
- `storage-suite` command — AC1-AC3, AC5
  run: `cargo test -p wisp-storage`
- `app-vault` command — AC4, AC5
  run: `cargo test -p wisp-app vault`
  pass: exit 0, 0 failed, and at least one new test for AC4.
- `lint` command — all
  run: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `check-before-open` read — AC1-AC4
  how: in every changed path the confinement check precedes the first open or read of the file.

## Standing constraints
- Worktree via `./.claude/scripts/wt-new.sh . 120 collection-file-confinement`, `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-120`.
- Secrets never logged or in docs; tests use the marker `SECRETVAL`.
- Tests never touch the real OS keychain: use `MemoryStore`, or a `wisp-test-<uuid>` service and delete only entries the test created.
- A temp `WISP_DATA_DIR` (or `DataDir::at(tempdir)`) for every test that needs a data dir.
- Never run `pkill` or `killall`.
- Windows: symlink tests are cfg(unix). Add no cfg(windows) branches; the changed code is platform-neutral, and it is first compiled and tested on Windows by the Windows CI job (54-CI), which lands after this task.

## Out of scope
- crates/storage/src/vault_binding.rs and crates/app/src/workspace.rs (not edited).
- 99-AC53 to AC56, AC59, AC60, AC62 to AC67 and AC72 (binding and TOCTOU items, deferred), including the double `sp.exists()` in `save_environment` (99-AC66).
- `ensure_collection_id` and `save_collection_defaults` locking (99-AC4).

## PR #63 review round 1 (HEAD 1749979)
- 63-1 (Medium, security) crates/storage/src/vault.rs:430 and environment.rs plain load: the plain `<env>.toml` was read through a symlink; `dev.toml` -> `prod.secrets.toml` loaded secrets as plain vars. FIXED: a symlinked or non-regular `<env>.toml` is refused (Invalid) on load and save.
- 63-2 (follow-up) ensure_ignore_line confines from environments/, not the root.
- 63-3 (follow-up) a refused .gitignore fails the save after <env>.toml was already written.
- 63-4 (follow-up) tests assert is_err rather than Invalid; ensure_marker_ignored and FIFO untested.
