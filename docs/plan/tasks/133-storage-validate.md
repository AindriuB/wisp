# 133 — Add the tolerant collection validator in storage

**Repo:** .
**Depends on:** 131
**Owns:**
- crates/storage/src/validate.rs (new)
- crates/storage/src/lib.rs (one `pub mod validate;` line)
- crates/storage/src/collection.rs (visibility changes and extraction of scan helpers only; `open_collection` behaviour unchanged)
- crates/storage/tests/validate.rs (new)

## Goal
`open_collection` stops at the first bad `folder.toml`. Add a read-only walker
that returns every finding in a collection (path, key, message, severity) so
`wisp-cli validate` (2D) and later the GUI can list all problems at once,
including malformed and unsupported assertions from task 131.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Feature 4" `validate` bullet; "Redaction and read-only rules" items 4-5
- crates/storage/src/collection.rs:328-430 `scan_dir` (confinement, symlinks, `Node::Invalid`, visit cap); :145 `read_format`; :283 `load_folder_defaults`; :432 `open_collection`
- crates/storage/src/error.rs:7-35 `StorageError`; crates/storage/src/codec.rs:42-56 parse errors carry line and short message only
- `core::assert::Assertion::Unrecognised` (task 131)
- Worktree `wt-new.sh . 133 storage-validate`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-133`

## Acceptance
- [ ] AC1 `pub fn validate(root: &Path) -> Report` where `Report { findings: Vec<Finding>, truncated: bool }` and `Finding { path: PathBuf (relative to root), key: Option<String>, message: String, severity: Severity }`, `Severity { Error, Warning }`. It never returns early on a finding; an unreadable root is a single `Error` finding.
- [ ] AC2 It checks `collection.toml` (including unsupported `format`), every `folder.toml`, every request file through `load_request`, every environment through `load_environment`, and reports each `Node::Invalid` and each symlink escaping the root, with the same confinement and visit cap as `scan_dir`.
- [ ] AC3 Assertions: `Unrecognised` with reason malformed is an `Error`; unsupported kind/op is a `Warning`; a `{{` in an expected value is a `Warning`; more than 50 assertions is a `Warning`.
- [ ] AC4 Fixture test (built in a tempdir by the test): a collection with a broken folder, a broken request, an invalid assertion, an unknown assertion kind, a keychain-mode environment and a symlink escape (unix-gated) yields all findings in one call, with the expected severities.
- [ ] AC5 Read-only: the test makes the fixture read-only, runs `validate`, and asserts the file tree and every mtime are unchanged. `validate.rs` calls no `ensure_collection_id`, `save_*`, `create_*`, `atomic_write*`, `ops::*`, no history store and no secret store.
- [ ] AC6 Canary: a secrets-file value, a plain variable value and an `Authorization` header literal with unique markers appear in no finding's `Debug` output.
- [ ] AC7 `open_collection` behaviour is unchanged: its existing tests pass unmodified.

## Checks
- `storage-validate` command — AC1-AC6
  run: `cargo test -p wisp-storage --test validate`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `read-only-source` command — AC5
  run: `! rg -n 'ensure_collection_id|atomic_write|save_|create_|ops::|keychain' crates/storage/src/validate.rs`
- `collection-diff` read — AC2, AC7
  how: `collection.rs` diff is visibility or a behaviour-preserving extraction shared by `scan_dir` and `validate`.

## Out of scope
- CLI output, exit codes, DTOs: 2D.
- `environment.rs`, `vault.rs` (task 132); `ops.rs` (task 134); `codec.rs`, `request.rs` (task 131).

## Review round 1 — findings

Reviewed HEAD 7db89b3.

- [P2] crates/storage/src/validate.rs:45-57 (used at :193, :254) — `Report::error` drops the StorageError's own path and files the finding under the file being visited, so errors in a secrets file or sidecar body are reported against the wrong file with wrong line numbers. Inputs: (1) environments/dev.toml = `[vars]\na = "1"\n`, environments/dev.secrets.toml = `\n\n[[[\n` -> finding path dev.toml "line 3: invalid key"; (2) a.toml with `[body] type="text" file="missing.txt"`, no missing.txt -> path a.toml "cannot read: entity not found". Fix: use the error's path with the root stripped when it is inside root; otherwise keep the current path.
- [P2] validate.rs:264-271 — `Mode::Unknown` falls into the `Ok(_)` arm and is reported as a harmless Warning ("secrets are held in a secret store"), while the app fails to load it (vault.rs:471 FailReason::UnknownStoreKind). Input: environments/typo.toml = `secrets = "keychian"\n`. Fix (orchestrator decision: severity Error, matching vault.rs): a separate Unknown arm with its own Error finding on key `secrets`, never echoing the mode string.
- [P3, follow-up] validate.rs:10-11/:204 MAX_ASSERTIONS copied instead of importing wisp_core::assert::MAX_ASSERTIONS; warning text hides that entries after the 50th are skipped; :212 compares literal "malformed" instead of ParseError::Malformed.reason().
