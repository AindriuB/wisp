# 185 — Harden the binding registry and keychain worker (H3b)

**Repo:** .
**Round:** 0
**Depends on:** 186 (for the `tests/vault.rs` edit only; see Amendment 1)
**Owns:**
- crates/storage/src/vault_binding.rs
- crates/storage/src/keychain.rs
- crates/storage/tests/binding.rs (new)
- crates/storage/tests/keychain.rs
- crates/storage/tests/vault.rs — only the test fn `rebind_of_one_root_leaves_the_other_bound` (~:1883): change it to expect the refusal AC1 requires. The rest of the file is 186's.

## Goal
Close the H3b binding-registry ACs from task 99. The ACs:
- stop a copy from being bound through the "moved" prompt;
- stop markers from sharing one registry id;
- fail closed on oversized markers;
- refuse stale or planted worktree anchors and Windows device names;
- zeroize the keychain worker's secret copies.

## Context
- docs/plan/tasks/99-hardening-0-2-0.md:113-124: the text of AC60, AC62-AC65, AC70 and AC71. The spec's "Feature 6" corrects the stale line numbers.
- docs/plan/specs/2026-11-19-wisp-0-5-0.md, open question 6: this brief assumes the recommendation for AC60. The orchestrator amends AC1 if the user answers otherwise.
- crates/storage/src/vault_binding.rs:
  - markers :229-292;
  - git and worktree verification :421-653 (`exact_worktree_gitdir` :510, `common_dir` :537, `anchor_common_dir` :610);
  - `rebind_confirmed` :682.
- crates/storage/src/keychain.rs:211-290: the `Worker`.
- `zeroize` is in `Cargo.lock` only transitively. Do not add a direct dependency; zero buffers with volatile writes, or stop and report if that is not enough.
- crates/storage/tests/vault.rs is task 186's except `rebind_of_one_root_leaves_the_other_bound` (see Owns). 186 must merge first; update this branch from main before editing that test. Put new binding tests in `tests/binding.rs`. If any other existing test in `tests/vault.rs` must change, stop and report.
- Worktree: `./.claude/scripts/wt-new.sh . 185 binding-registry-h3b`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-185`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (99-AC60) A rebind through the "moved" path is refused while any other live root (one that exists on disk) carries the same collection id, even when some bound root is gone. The error names that root (escaped) and points to "Give this copy its own id". Test `h3b_rebind_refused_while_other_root_live`.
- [ ] AC2 (99-AC62) No two roots' markers share an id: each marker key is derived per root, for example a keyed hash of the registry id and the canonical root. A marker that git tracks produces a warning the caller can surface. Tests `h3b_markers_share_no_id`, `h3b_tracked_marker_warns`.
- [ ] AC3 (99-AC63) A marker is read with a size cap taken from `lstat` and a cap on entries; going over either fails closed. Malformed entries, and lines for vanished registries (with the 52-N2 cap), are dropped on write. Tests `h3b_oversized_marker_fails_closed`, `h3b_malformed_entries_dropped_on_write`.
- [ ] AC4 (99-AC64) A linked-worktree entry whose folder was deleted by hand does not verify a folder later created at that path. A retargeted path component (a symlink) of a real entry does not verify another folder. Tests `h3b_recreated_worktree_not_verified`, `h3b_retargeted_component_refused`.
- [ ] AC5 (99-AC65) Worktree verification refuses a git common dir nested inside another bound collection root of any id, unless it is exactly that root's own `.git`. Test `h3b_planted_common_dir_refused`. Known residual: a planted `.git` in a bound main checkout that is also the repo root cannot be told from a real one; task 197 documents this trust assumption (99-AC65's "document the trust assumption" alternative).
- [ ] AC6 (99-AC70) Every secret copy the keychain worker holds (request payload, result buffer, any intermediate `String` or `Vec`) is zeroed after use, including on timeout and on panic recovery.
- [ ] AC7 (99-AC71) Worktree verification refuses reserved DOS device names and names with a trailing dot or space, or does its I/O through the verbatim path form. `cfg(windows)` tests `h3b_win_com1_refused` and `h3b_win_trailing_dot_refused`.
- [ ] AC8 No file outside Owns changes. No new direct dependency. Every existing storage test passes unmodified, except `rebind_of_one_root_leaves_the_other_bound` as Owns allows. fmt, clippy and the workspace suite pass, Windows CI included.

## Checks
- `h3b-tests` command — AC1, AC2, AC3, AC4, AC5
  run: `cargo test -p wisp-storage --locked --test binding h3b_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([8-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `h3b-windows` command — AC7 (runs on the Windows CI leg; it passes trivially elsewhere)
  run: `cargo test -p wisp-storage --locked --test binding h3b_win_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\.'`
- `no-dep` command — AC8
  run: `git diff --exit-code main...HEAD -- Cargo.lock crates/storage/Cargo.toml`
- `scope` command — AC8
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/vault_binding.rs$' -e '^crates/storage/src/keychain.rs$' -e '^crates/storage/tests/binding.rs$' -e '^crates/storage/tests/keychain.rs$' -e '^crates/storage/tests/vault.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `zeroize` read — AC6
  how: the reviewer traces each secret buffer through the worker and confirms that each one is zeroed on every exit path and cannot be optimised away.
- `win-ci` read — AC7
  how: the Windows CI job log for the PR head shows `h3b_win_com1_refused` and `h3b_win_trailing_dot_refused` passing, not skipped.

## Out of scope
- Task 186's file-safety items in `environment.rs` and `vault.rs`.
- Windows file-identity checks in `open_nofollow` (left in PLAN).
- Any change to `crates/app/src/vault.rs` prompts beyond what the new error text needs. If that file must change, stop and report.

## Wave stopped — step 1 — BLOCKED: spec change (AC1 vs tests/vault.rs:1883 rebind_of_one_root_leaves_the_other_bound, owned by 186; AC5 narrowed to "nested in another bound root unless it is that root's own .git")

## Amendment 1 (2026-11-19)
User decision ("Yes, as recommended") after the implementer stopped BLOCKED.
- AC1 stands as written (spec Q6). It contradicted the 0.4.0 test `tests/vault.rs` `rebind_of_one_root_leaves_the_other_bound`, owned by 186. 185 now owns that one test fn, changes it to expect AC1's refusal, and depends on 186 for that edit.
- AC5 narrowed: the old rule ("a common dir inside any collection root") refused the standard layout where the collection root is the repo root. The new rule refuses a common dir nested inside another bound root unless it is that root's own `.git`; the planted-`.git`-at-repo-root case is a documented residual (task 197).

## Wave resumed — user accepted Amendment 1 ("Yes, as recommended"); re-fanout after 186 merges (branch updated from main, then flip the one vault.rs test)
