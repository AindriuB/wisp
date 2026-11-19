# 186 — Close the secret-file safety follow-ups from task 169

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/storage/src/environment.rs
- crates/storage/src/vault.rs
- crates/storage/tests/vault.rs

## Goal
Close four 0.4.0 follow-ups:
- an id write must not change `collection.toml`'s mode;
- a FIFO swapped in for a secrets file must not hang a read;
- the git-tracked check must be bounded and fail closed;
- moved-name detection must not follow symlinks.

Also fix the `t1` flake in the vault suite.

## Context
- docs/plan/PLAN.md "Follow-ups from task 169" (:81-91), and docs/plan/known-flakes.md, second entry.
- docs/plan/specs/2026-11-19-wisp-0-5-0.md, open question 8: this brief assumes the recommendation (fail closed inside a work tree). The orchestrator amends AC3 if the user answers otherwise.
- crates/storage/src/environment.rs: `open_nofollow` :150, `write_nofollow` :193, `gains_moved_name` :462.
- crates/storage/src/vault.rs:609 `is_git_tracked`.
- crates/storage/tests/vault.rs:2725 is the flaky test.
- Processes: kill only the child you spawned, by its handle; never `pkill`.
- Worktree: `./.claude/scripts/wt-new.sh . 186 secret-file-safety`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-186`. Foreground commands only.

## Acceptance
- [ ] AC1 `write_nofollow` replacing an existing file keeps that file's permission mode on Unix. A new file gets today's mode. Test `sfs_write_keeps_existing_mode`: a 0644 file stays 0644.
- [ ] AC2 `open_nofollow` opens with `O_NONBLOCK` on Unix and refuses a non-regular file after open (fstat), returning an error promptly. Test `sfs_fifo_refused_without_hang` (Unix, `mkfifo`). The test fails by error or by a 10 s watchdog, never by hanging CI.
- [ ] AC3 `is_git_tracked`:
  - makes no `git` call when the path has no `.git` ancestor up to the collection root;
  - otherwise runs `git` with a 5 s bound, and on timeout kills only that child;
  - treats a timeout, a spawn failure or a non-zero exit inside a work tree as tracked;
  - the refusal message says the check could not run.

  Tests `sfs_git_missing_outside_repo_allows` and `sfs_git_failure_inside_repo_refuses` (the latter uses a `PATH` seam or a fake `git` script).
- [ ] AC4 `gains_moved_name` reads the other environment file without following a symlink. A symlinked file is treated as not readable. Test `sfs_moved_name_ignores_symlink`.
- [ ] AC5 `h3_load_without_a_data_dir_refuses_a_keychain_environment` uses the secret value `zz-hunter` in place of `t1` and asserts on that value. Nothing else in that test changes.
- [ ] AC6 No file outside Owns changes. No new dependency. Every existing test passes, with only the AC5 edit. fmt, clippy and the workspace suite pass.

## Checks
- `sfs-tests` command — AC1, AC2, AC3, AC4
  run: `cargo test -p wisp-storage --locked --test vault sfs_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([5-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `flake-value` command — AC5
  run: `rg -q 'zz-hunter' crates/storage/tests/vault.rs && cargo test -p wisp-storage --locked --test vault h3_load_without_a_data_dir_refuses_a_keychain_environment 2>&1 | grep -E 'test result: ok\. 1 passed'`
- `scope` command — AC6
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/environment.rs$' -e '^crates/storage/src/vault.rs$' -e '^crates/storage/tests/vault.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `fail-closed` read — AC3
  how: the reviewer checks that every error path inside a work tree is treated as tracked, and that the spawned child is killed by its handle only.

## Out of scope
- Binding markers and the keychain worker (task 185).
- The Windows file-identity check in `open_nofollow` (left in PLAN).
- Changing which files count as secrets files.
