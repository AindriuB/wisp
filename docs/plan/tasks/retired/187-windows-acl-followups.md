# 187 — Close the Windows ACL follow-ups from task 171

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/storage/src/win_acl.rs
- crates/storage/src/data_dir.rs
- crates/storage/Cargo.toml (the `windows-sys` line only)

## Goal
Make the owner-only check honour access rights, and stop a failed restrict
from leaving a newly created data-dir ancestor with inherited ACLs. Also bring
the `windows-sys` dependency line in line with the conventions.

## Context
- docs/plan/PLAN.md "Follow-ups from task 171" (:43-50).
- crates/storage/src/win_acl.rs: `is_owner_only` :100-107, `restrict_to_owner` :152.
- crates/storage/src/data_dir.rs ~:140-160: create, then restrict.
- docs/conventions.md:83-84: `default-features = false` on dependencies.
- The code is `cfg(windows)` and is not built locally, so the Windows CI job is the proof.
- Worktree: `./.claude/scripts/wt-new.sh . 187 windows-acl-followups`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-187`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `is_owner_only` returns true only when the DACL's single ACE is the owner's and grants full file access (`FILE_ALL_ACCESS`, or a generic-all mapping). A single owner ACE with reduced rights is not owner-only, so `restrict_to_owner` is applied. `cfg(windows)` test `acl_reduced_owner_ace_is_not_owner_only`.
- [ ] AC2 If `restrict_to_owner` fails after the data-dir code created one or more directories, those directories are removed, deepest first, only the ones this call created and only if still empty, and the error is returned. A `#[cfg(test)]` failure seam is allowed. `cfg(windows)` test `acl_failed_restrict_removes_created_dirs`.
- [ ] AC3 The `windows-sys` entry in `crates/storage/Cargo.toml` has `default-features = false` with the same feature list, and `Cargo.lock` does not change.
- [ ] AC4 No file outside Owns changes. fmt, clippy and the workspace suite pass, Windows CI included.

## Checks
- `lock-unchanged` command — AC3
  run: `git diff --exit-code main...HEAD -- Cargo.lock && rg -q 'windows-sys = \{[^}]*default-features = false' crates/storage/Cargo.toml`
- `scope` command — AC4
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/win_acl.rs$' -e '^crates/storage/src/data_dir.rs$' -e '^crates/storage/Cargo.toml$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC4
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `win-ci` read — AC1, AC2
  how: the Windows CI job log for the PR head shows `acl_reduced_owner_ace_is_not_owner_only` and `acl_failed_restrict_removes_created_dirs` passing, not skipped. The reviewer checks that the cleanup can never remove a directory that existed before the call.

## Out of scope
- ReFS 128-bit file ids, `volume_flags` notes, and the "already owner-only skips restrict" test (Notes, left in PLAN).
- Any change to which directories are restricted.

## Wave verified — 679aa19c42213edef5f2b439ee63030dff036c75

## Outcome
Merged 2026-11-19 at 679aa19c42213edef5f2b439ee63030dff036c75 (tester PASS, reviewer APPROVE, no fix rounds). `is_owner_only` requires full file rights on the owner's single ACE, a failed `restrict_to_owner` removes the directories the call created, and `windows-sys` has `default-features = false`. Windows CI ran both new tests (job 135919944074, run 44626018466). Follow-ups are in `PLAN.md`.
