# 59 — Cut history previews on a char boundary; list linked environments and save them in a safe order

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/storage/src/history.rs
- crates/storage/src/environment.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-59`.

## Goal
M3 (v1 hardening). Closes the PLAN.md storage carry-forwards "History (15)" and "Environments (19)". First, `preview_from` turns a 4-byte char that straddles the 1,024-byte limit into U+FFFD. Second, a symlinked `environments/dev.toml` loads but is not listed. Third, one `file_type` error aborts the whole listing. Fourth, the plain file is written before the secrets file, so a save that half-fails can lose a var that is moving from plain to secrets.

## Context
- crates/storage/src/history.rs:18 `PREVIEW_LIMIT`, :50-60 `preview_from`; test at :186.
- crates/storage/src/environment.rs:62-85 `list_environments` (`entry.file_type()` does not follow links, `?` on error at :76).
- crates/storage/src/environment.rs:151-178 `save_environment`: `atomic_write(plain)` then `atomic_write_private(secrets)`.
- docs/conventions.md#logging — log environment and file names only, never values.

## Acceptance
- [ ] `preview_from` decodes at most `PREVIEW_LIMIT + 3` bytes and cuts on a char boundary ≤ `PREVIEW_LIMIT`. Test: 1,022 ASCII bytes followed by a 4-byte char gives a preview of exactly those 1,022 bytes with no U+FFFD. The existing preview tests still pass.
- [ ] `list_environments` lists `<name>.toml` when it is a symlink to a regular file (`#[cfg(unix)]` test). If an entry's metadata cannot be read, that entry is skipped with a `tracing::warn!` naming the file, and the rest are still listed (test, for example with a dangling symlink).
- [ ] `save_environment` writes first the file that gains a var name compared with its current on-disk content, then the other. If neither gains a name, the order is plain then secrets. Tests use a `#[cfg(test)]` fault hook to fail the second write in both directions (plain to secrets and secrets to plain), then show `load_environment` still returns the moved var.
- [ ] Secrets are still written with `atomic_write_private`, and the `.gitignore` is still ensured before the secrets write.
- [ ] `cargo test -p wisp-storage`, `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
- [ ] `git diff main -- crates/storage/src/lib.rs crates/storage/src/codec.rs crates/storage/src/fs.rs crates/app` is empty.

## Out of scope
- Environment name rules (spaces, `COM0`/`LPT0`, case-folding collisions).
- History rotation races and any history UI (not in v1).
- Codec comment and spacing residuals.
