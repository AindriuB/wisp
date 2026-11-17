# 171 — Windows hardening: owner-only data dir, real 8.3 test, file identity (2F, H6)

**Repo:** .
**Round:** 0
**Depends on:** 163, 165, 166
**Owns:**
- crates/storage/src/fs.rs
- crates/storage/src/data_dir.rs
- crates/storage/src/win_acl.rs (new)
- crates/storage/src/lib.rs (insert-only: one `mod win_acl;` line)
- crates/storage/src/request.rs (AC77 only: the test `sidecar_rejects_8dot3_short_name_of_sibling_toml`)
- crates/storage/Cargo.toml (Windows-only `windows-sys` dependency and features)
- Cargo.lock (the `wisp-storage` entry only)
- .github/workflows/ci.yml (the Windows 8.3 setup step)

## Goal
Close hardening theme H6: on Windows the data dir and everything under it are
owner-only, the 8.3 short-name test either runs for real in CI or says loudly
that it skipped, and `same_file_entry` really detects hard links.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" H6 row (:422), Waves 2F (:472).
- docs/plan/tasks/99-hardening-0-2-0.md AC76 :127, AC77 :131, AC78 :132.
- crates/storage/src/fs.rs:36-48 `same_file_entry` (always false off unix), :52 `same_file_entry_any_toml`.
- crates/storage/src/data_dir.rs:91 `ensure_private_dir` (163 changed it first: trailing slash, 0700 tightening).
- crates/storage/src/request.rs:1661-1667 the 8.3 test that silently returns.
- `Cargo.lock` already holds `windows-sys` 0.48.0, 0.52.0 and 0.61.2: reuse one of them (GUESS: one fits); add no new version. This task depends on 166 only because both change `Cargo.lock`.
- .github/workflows/ci.yml:13-55 (the `check` job runs on `windows-latest`).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 171 windows-hardening`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-171`. Foreground commands only; never `pkill` or `killall`. Windows code is proved by the Windows CI job; locally run `cargo check --target x86_64-pc-windows-msvc -p wisp-storage` if the target is installed.

## Acceptance
- [ ] AC1 (AC76) On Windows, `ensure_private_dir` sets a protected DACL granting full control to the current user only (no inherited ACEs), with object- and container-inherit so new history, cookie and body files inherit it; existing dirs are tightened the same way. A `cfg(windows)` test reads the DACL back and finds exactly one allow ACE, for the current user's SID, and the protected flag set; a file created inside has the same single ACE.
- [ ] AC2 (AC77) The 8.3 test prints `SKIPPED: 8.3 short names are off on this volume` to stderr when it cannot run; `ci.yml`'s Windows leg enables 8.3 names on the runner's temp volume before `cargo test` (e.g. `fsutil 8dot3name set <drive> 0`) and sets an env var that makes the test fail instead of skip when 8.3 names are still off.
- [ ] AC3 (AC78) `same_file_entry` on Windows compares volume serial number and file index from `GetFileInformationByHandle` on handles opened with symlink-following semantics, matching the unix `metadata` call. `cfg(windows)` tests: a hard link is the same entry; two distinct files are not; a missing path is false.
- [ ] AC4 The Windows code lives in `win_acl.rs` and `fs.rs` behind `cfg(windows)`; unix behaviour is unchanged (unix tests pass unmodified). `windows-sys` is a `[target.'cfg(windows)'.dependencies]` entry at an already-locked version.
- [ ] AC5 The PR links the green Windows CI run and quotes the 8.3 test line showing it ran (not skipped).
- [ ] AC6 No file outside Owns changes; fmt, clippy and the workspace suite pass on all three CI legs.

## Checks
- `ci-green` command — AC1, AC2, AC3, AC5, AC6
  run: `gh pr checks <pr> --watch --fail-fast`
- `lock-scope` command — AC4
  run: `git diff main...HEAD -- Cargo.lock | grep '^[+-]' | grep -v '^+++\|^---' | grep -v 'windows-sys' ; test $? -eq 1`
- `scope` command — AC6
  run: `git diff --exit-code main...HEAD -- crates/core crates/app crates/cli crates/service crates/storage/src/history.rs crates/storage/src/vault.rs crates/storage/src/import`
- `suite-and-lint` command — AC4, AC6
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `windows-code` read — AC1, AC3, AC4
  how: the reviewer checks the DACL construction (protected, one owner ACE, inherit flags), the handle-based identity, and that unix code paths are untouched.

## Out of scope
- ACLs on paths outside the data dir (collections, settings next to a portable exe).
- Any other Windows-specific behaviour (signing, installer).
