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
- [ ] AC3 `is_git_tracked` (spec open question 8):
  - "inside a work tree" means the secrets file's directory is inside any git work tree, however far above the collection root the repo root is. The implementer decides it either by walking ancestors up to the filesystem root looking for `.git`, or by `git rev-parse --is-inside-work-tree`;
  - makes no tracked-file `git` call outside any work tree;
  - inside a work tree runs `git` with a 5 s bound, and on timeout kills only that child;
  - treats a timeout, a spawn failure or a non-zero exit inside a work tree as tracked. If `git rev-parse` is the deciding method, its own failure counts the same way;
  - the refusal message says the check could not run.

  Tests:
  - `sfs_git_missing_outside_repo_allows`;
  - `sfs_git_failure_inside_repo_refuses`, using a `PATH` seam or a fake `git` script;
  - `sfs_nested_collection_tracked_secrets_refuses`: `<tmp>` is `git init`ed, the collection root is `<tmp>/coll`, and `coll/environments/dev.secrets.toml` is added with `git add -f`. The test asserts the secrets file is refused as tracked.
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

## Amendment 1 (2026-11-19)
AC3 now defines "inside a work tree" as the secrets file's directory being inside any git work tree, even when the repo root is above the collection root. The implementer can decide this by walking ancestors to the filesystem root for `.git` or with `git rev-parse --is-inside-work-tree`. The fail-closed rule does not change. The test `sfs_nested_collection_tracked_secrets_refuses` is added.
Reason: code review round 1, P1. The old wording ("no `.git` ancestor up to the collection root") contradicted spec Q8 (docs/plan/specs/2026-11-19-wisp-0-5-0.md:398). It let a tracked secrets file through when the collection was a subfolder of a repo.

## Review round 1 — findings

Reviewed HEAD: 58d10273ecf1faa2303c7f8e4e1fb111a74af760

- [P1] crates/storage/src/vault.rs:620-636 — `has_git_ancestor` stops looking at the collection root. When the collection is a subfolder of a git repo, the git check is skipped completely, so a git-tracked secrets file is now accepted. Before this branch, `git -C root ls-files` searched upward and refused it.
  route: orchestrator — the AC3 wording at docs/plan/tasks/186-secret-file-safety.md:33 ("no `.git` ancestor up to the collection root") is what causes this. The fix (search above the root, or ask `git rev-parse` whether the path is in a work tree before treating it as "outside") means amending AC3. [Orchestrator: AC3 amended to match spec Q8 — see Amendment 1.]
  input: `<tmp>/.git` from `git init`, collection root `<tmp>/coll`, and `coll/environments/dev.secrets.toml` added with `git add -f`. Then call `import_secrets_file(<tmp>/coll, "dev", …)` or `migrate_to_file(...)`.  →  `has_git_ancestor` checks `coll/environments/.git` and `coll/.git`, reaches `dir == root` at :626 and returns false. `git_check` returns `NotTracked`, so secrets are read from the tracked file or written into it. On main the same input gave `TrackedSecretsFile`.
- [P2] crates/storage/src/environment.rs:225,274 — on Windows the `keep_mode` parameter is only used inside the `#[cfg(unix)]` `existing_mode` statement, so it counts as unused there. The Windows CI clippy job (`.github/workflows/ci.yml:19,69`, `-D warnings`) fails on `unused_variables`. On Windows the `.and_then(|()| { Ok(()) })` closure body can also trigger `clippy::bind_instead_of_map`.
  route: implementer
  input: `cargo clippy --workspace --all-targets --locked -- -D warnings` on windows-latest  →  error: unused variable `keep_mode`.
- [P2] crates/storage/tests/vault.rs:2758 — `sfs_fifo_refused_without_hang` does not test O_NONBLOCK. The FIFO is in place before the call, so `check_secrets_path` (vault.rs:931) and the lstat in `open_nofollow` (environment.rs:170) refuse it before any `open`. The test passes with `open.custom_flags(libc_o_nonblock())` (environment.rs:181) removed. The hang AC2 targets only happens when a FIFO is swapped in after the lstat.
  route: implementer — add a unit test in the environment.rs `tests` module that uses the existing `set_test_hook` at `"open:after-lstat"` to swap a FIFO in (the same pattern as the symlink-swap tests near environment.rs:716/734), with the 10 s watchdog. Keep the integration test so the `sfs-tests` count still holds.
  input: delete environment.rs:181  →  the test still passes.
- [P2] crates/storage/tests/vault.rs:2810 — `sfs_moved_name_ignores_symlink` never reaches `gains_moved_name`. `sp.exists()` follows the link (environment.rs:593), so `touches_secrets` is true and `confined_regular_file` refuses the symlink at :596 first. The test would pass with the old symlink-following `load_vars` restored.
  route: implementer — add a unit test in environment.rs that calls `gains_moved_name(<symlink to a file holding host>, [host])` directly and asserts `false`.
  input: put `gains_moved_name` back to `if other.exists() { load_vars(other) … }`  →  the test still passes.
- [STYLE] crates/storage/src/environment.rs:210 — the `write_nofollow` doc comment still says it writes "(mode `0600` on unix)", but now that is only true for a new file or for `write_secret_nofollow`. The added line at :215 contradicts it.
  route: implementer

## Review round 2 — findings

Reviewed HEAD: 13264128d7fc7b4c3cd14ff8c18bd7cfbdb694ec

- [P2] crates/storage/src/environment.rs:281-293 — The Windows clippy failure from round 1 is only half fixed. The `#[allow(unused_variables)]` at :226 clears the `keep_mode` warning. But on Windows the first `.and_then(|()| { #[cfg(unix)] if let … ; Ok(()) })` closure becomes `{ Ok(()) }` once the cfg is stripped, so `clippy::bind_instead_of_map` still fails `-D warnings`.
  route: implementer
  input: `cargo clippy --workspace --all-targets --locked -- -D warnings` on windows-latest (.github/workflows/ci.yml:19,69)  →  error: "using `Result.and_then(|x| Ok(y))`, which is more succinctly expressed as `map(|x| y)`" at environment.rs:281.
  proof: Switching the two `#[cfg(unix)]` attributes in write_impl to `#[cfg(windows)]` in a scratch copy makes `cargo clippy -p wisp-storage --lib -- -D warnings` fail with exactly that error.
- [P2] crates/storage/src/vault.rs:621-626 — `has_git_ancestor` walks the path's ancestors as written, without resolving symlinks (`std::path::absolute` keeps symlinks). If a directory between the repo root and the collection is reached through a symlink, no `.git` is found. `git_check` then returns `NotTracked` without running git, even though git (which uses the real cwd) would report the file as tracked. On main, `git -C root ls-files` always ran and refused it, so this is a regression and a fail-open on the secrets guard.
  route: implementer — resolving symlinks (e.g. `std::fs::canonicalize` on the parent) before the ancestor walk is still the "walking ancestors up to the filesystem root" that Amendment 1 / AC3 allows.
  input: `<t>/repo` is `git init`ed. The collection is `<t>/repo/sub/coll`, and `sub/coll/environments/dev.secrets.toml` is added with `git add -f`. `<t>/link` is a symlink to `<t>/repo/sub`. Call `import_secrets_file(<t>/link/coll, "dev", …)`  →  returns `Ok(None)`, so the tracked file is accepted. The expected result is `TrackedSecretsFile`. `migrate_to_file` goes through the same `refuse_tracked`.
  proof: Ran as a scratch integration test at HEAD; it fails with `Ok(None)`. Add it as a real test.
- [STYLE] crates/storage/src/environment.rs:211 — The rewrapped `write_nofollow` doc comment has a 121-character line. The comment lines either side wrap at about 80.
  route: implementer
