# 185 — Harden the binding registry and keychain worker (H3b)

**Repo:** .
**Round:** 0
**Depends on:** 186 (for the `tests/vault.rs` edit only; see Amendment 1)
**Owns:**
- crates/storage/src/vault_binding.rs
- crates/storage/src/keychain.rs
- crates/storage/tests/binding.rs (new)
- crates/storage/tests/keychain.rs
- crates/storage/tests/vault.rs — only two test fns, each changed to expect the refusal AC1 requires: `rebind_of_one_root_leaves_the_other_bound` (~:1883) and `h3_a_stale_marker_does_not_make_an_existing_root_look_like_the_bound_one` (~:2643, now expects `LooksLikeCopy`; Amendment 2). The rest of the file is 186's.

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
- [ ] AC1 (99-AC60) A rebind through the "moved" path is refused while any other live root (one that exists on disk) carries the same collection id, even when some bound root is gone. The error names that root (escaped) and points to "Give this copy its own id". Live means the root exists (`try_exists`, which fails closed on an error) and its `collection.toml` carries the id; a missing or stale marker does not make a root dead (supersedes 99-AC59's nonce comparison; Amendment 2). Tests `h3b_rebind_refused_while_other_root_live`, and `h3_a_stale_marker_does_not_make_an_existing_root_look_like_the_bound_one` in `tests/vault.rs` flipped to expect `LooksLikeCopy`.
- [ ] AC2 (99-AC62) No two roots' markers share an id: each marker key is derived per root, for example a keyed hash of the registry id and the canonical root. A row written before per-root keys (`key: None`) is re-keyed by the next successful binding `check` or bind, so no marker keeps the registry id: under the lock it already holds, `check` writes the marker (dropping the registry-id entry) and then the registry, in `bind_locked`'s order; a failed re-key write is logged and `check` still returns `Ok`. `bind_locked`'s early return does not apply to key-less rows. No registry `VERSION` bump. A marker that git tracks produces a warning the caller can surface; `tracked_marker_warning` bounds its `git` child with the same timeout pattern as `git_check_with` (crates/storage/src/vault.rs ~:617, :662; pattern only, that file is not edited). Tests `h3b_markers_share_no_id`, `h3b_tracked_marker_warns`, `h3b_legacy_row_rekeyed_on_check`.
- [ ] AC3 (99-AC63) A marker is read with a size cap taken from `lstat` and a cap on entries; going over either fails closed. Malformed entries and this registry's own superseded keys are dropped on write; a write never removes another registry's well-formed entry (the eviction loop is deleted), and a write that would exceed `MARKER_MAX_ENTRIES` fails closed. The roots bound to one id are capped at 64 (52-N2): `bind_locked` refuses one more root for that id, failing closed with a message. Tests `h3b_oversized_marker_fails_closed`, `h3b_malformed_entries_dropped_on_write`, `h3b_full_marker_refuses_write`, `h3b_root_list_capped`.
- [ ] AC4 (99-AC64) A linked-worktree entry whose folder was deleted by hand does not verify a folder later created at that path. A retargeted path component (a symlink) of a real entry does not verify another folder. Provenance uses evidence the folder's creator cannot set: on unix the worktree's `.git` file and the entry's `gitdir` file have status-change times (`st_ctime`) within 10 s of each other, and the same owner uid as the trusted common dir (99-AC64's "owner check"). A `.git` file whose mtime was copied with `touch -r` does not verify. Windows keeps the write-time window; its residual is recorded in task 197. Every existing one-click worktree test still passes unedited (see the `worktree-regression` check). Tests `h3b_recreated_worktree_not_verified` (reworked: sleep past the window before writing `.git`), `h3b_forged_mtime_worktree_not_verified` (`cfg(unix)`), `h3b_retargeted_component_refused`.
- [ ] AC5 (99-AC65) Worktree verification refuses a git common dir nested inside another bound collection root of any id, unless it is exactly that root's own `.git`. Test `h3b_planted_common_dir_refused`. Known residual: a planted `.git` in a bound main checkout that is also the repo root cannot be told from a real one; task 197 documents this trust assumption (99-AC65's "document the trust assumption" alternative).
- [ ] AC6 (99-AC70) Every secret copy the keychain worker holds (request payload, result buffer, any intermediate `String` or `Vec`) is zeroed after use, including on timeout and on panic recovery. The `get` job zeroes `BadDataFormat(Vec<u8>, _)` payloads as it does `BadEncoding` (keychain.rs ~:347-355).
- [ ] AC7 (99-AC71) Worktree verification refuses reserved DOS device names and names with a trailing dot or space, or does its I/O through the verbatim path form. `cfg(windows)` tests `h3b_win_com1_refused` and `h3b_win_trailing_dot_refused`.
- [ ] AC8 No file outside Owns changes. No new direct dependency. Every existing storage test passes unmodified, except the two `tests/vault.rs` fns Owns allows. Doc comments in vault_binding.rs (~:11-17, :159-160, :848-852) match the new behaviour. fmt, clippy and the workspace suite pass, Windows CI included.

## Checks
- `h3b-tests` command — AC1, AC2, AC3, AC4, AC5 (12 on unix: the 8 original tests plus `h3b_forged_mtime_worktree_not_verified`, `h3b_legacy_row_rekeyed_on_check`, `h3b_full_marker_refuses_write`, `h3b_root_list_capped`)
  run: `cargo test -p wisp-storage --locked --test binding h3b_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[2-9]|[2-9][0-9]|[1-9][0-9]{2,}) passed; 0 failed; 0 ignored'`
- `worktree-regression` command — AC1, AC4 (the six one-click worktree tests unedited, plus the two flipped fns)
  run: `cargo test -p wisp-storage --locked --test vault -- --exact git_worktree_of_bound_repo_is_a_one_click_checkout victim_worktree_entry_with_back_pointer_to_another_folder_is_unverified symlink_reaching_a_real_worktree_layout_is_unverified bound_worktree_with_symlinked_gitdir_is_never_read new_worktree_of_a_bound_main_checkout_still_verifies_with_a_bound_worktree h3_additional_checkout_binds_the_canonical_worktree rebind_of_one_root_leaves_the_other_bound h3_a_stale_marker_does_not_make_an_existing_root_look_like_the_bound_one 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. 8 passed; 0 failed; 0 ignored' && cargo test -p wisp-app --locked keychain_same_secrets_here_for_a_verified_worktree_and_not_an_unverified_folder 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. 1 passed; 0 failed; 0 ignored'`
- `rekey-and-p3` read — AC2, AC6, AC8
  how: the reviewer confirms `check`'s re-key writes marker then registry under the held lock and logs (not returns) a write failure; `tracked_marker_warning` kills its `git` child on timeout like `git_check_with`; the `get` job wipes `BadDataFormat` bytes; the cited vault_binding.rs doc comments describe the new behaviour.
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
- Follow-up, not this task: a Windows owner and ChangeTime check for AC4 (owner via `GetNamedSecurityInfoW`, no new dependency).
- Follow-up, not this task: cleaning stale foreign marker lines (52-50). This task never removes another registry's well-formed entry.

## Wave stopped — step 1 — BLOCKED: spec change (AC1 vs tests/vault.rs:1883 rebind_of_one_root_leaves_the_other_bound, owned by 186; AC5 narrowed to "nested in another bound root unless it is that root's own .git")

## Amendment 1 (2026-11-19)
User decision ("Yes, as recommended") after the implementer stopped BLOCKED.
- AC1 stands as written (spec Q6). It contradicted the 0.4.0 test `tests/vault.rs` `rebind_of_one_root_leaves_the_other_bound`, owned by 186. 185 now owns that one test fn, changes it to expect AC1's refusal, and depends on 186 for that edit.
- AC5 narrowed: the old rule ("a common dir inside any collection root") refused the standard layout where the collection root is the repo root. The new rule refuses a common dir nested inside another bound root unless it is that root's own `.git`; the planted-`.git`-at-repo-root case is a documented residual (task 197).

## Wave resumed — user accepted Amendment 1 ("Yes, as recommended"); re-fanout after 186 merges (branch updated from main, then flip the one vault.rs test)

## Wave stopped — step 2 — orchestrator findings at aeb6c14: (1) AC4 stale-entry guard is an mtime heuristic the folder owner can forge (99-AC64 named "not prunable" or "owner check"); (2) AC2 has no migration for pre-0.5.0 rows keyed by the registry id; (3) "live" root = nonce match vs "exists and carries the id" (AC1/spec Q6 vs 99-AC59); (4) the 52-N2 cap meaning (marker eviction of foreign keys). Design-pass trigger hit (second authorisation finding in vault_binding).

## Amendment 2 (2026-11-20)
User decision ("every recommendation", 2026-11-20) on the architect design pass at aeb6c14, which answered the four step-2 findings above. AC1-AC4, AC6, AC8, Owns, Checks and Out of scope are amended in place.
- F1 (AC4): the mtime window was forgeable by whoever creates the folder (`touch -r`). On unix, provenance now rests on `st_ctime` (which the creator cannot set) of the `.git` file and the entry's `gitdir` file, plus the common dir's owner uid (99-AC64's "owner check"). Windows keeps the write-time window; its residual goes to task 197 and a follow-up. Caveat for the implementer: confirm in git's `builtin/worktree.c` that `git worktree add` writes the entry's `gitdir` and the worktree's `.git` within the 10 s window before relying on it.
- F2 (AC2): rows written before per-root keys kept the registry id in their markers, so AC2 held only for new binds. Lazy re-keying in `check` and `bind_locked` fixes old rows without a format bump.
- F3 (AC1): liveness by nonce match let a stale marker hide a live copy. Live is now "exists and carries the id", per spec decision 6 / AC60; this supersedes 99-AC59's nonce half. 185 takes one more `tests/vault.rs` fn, flipped to expect `LooksLikeCopy`, on the same single-fn terms as Amendment 1.
- F4 (AC3): the eviction loop could remove another registry's live entry. It is deleted; an over-cap write fails closed, and the real 52-N2 cap (64 roots per id) is enforced in `bind_locked`.
- The three implementer-route P3s from the 185 code review are folded in: `tracked_marker_warning` timeout (AC2), `BadDataFormat` wipe (AC6), stale vault_binding.rs docs (AC8).

## Wave resumed — user accepted Amendment 2 (design-pass recommendation); re-fanout

## Wave stopped — step 2 — orchestrator finding at cef8a28: AC4's premise that st_ctime is "evidence the folder's creator cannot set" is wrong where hard links to another user's files are allowed (macOS; Linux without fs.protected_hardlinks): user B can hard-link a live worktree's `.git` and the entry's `gitdir`, recreate the folder after the victim deletes it by hand, and both files then carry the victim's uid and a fresh shared ctime → one-click keychain access. Proposed fix (Amendment 3): also require the worktree folder itself to be owned by the common dir's uid (99-AC64's literal "the folder's owner is checked"); a directory cannot be hard-linked. Also in this review: STYLE `.expect("row present")` without proof comment (vault_binding.rs:454); P3 superseded keys of this registry's dead rows for the same root are not dropped (AC3; ~63 re-ids lock the root out); P3 (orchestrator) cross-registry marker write race now also reachable from check's re-key (needs a cross-registry lock; follow-up); NOTE the uid half has no test.

## Amendment 3 (2026-11-20)

Reason: Amendment 2's unix provenance check (AC4: `.git` file and `gitdir` entry `st_ctime` within 10 s, same uid as the trusted common dir) is forgeable where hard links to another user's files are allowed (macOS; Linux without `fs.protected_hardlinks`). Another local user hard-links a live worktree's `.git` and the entry's `gitdir`, waits for the victim to delete the worktree folder by hand in a shared folder such as `/Users/Shared`, recreates the folder, and gets one-click keychain access. A directory cannot be hard-linked, so the folder's own owner closes this (99-AC64's literal "the folder's owner is checked"). The user chose this fix on 2026-11-20.

Owns: unchanged. All new code and tests go in `crates/storage/src/vault_binding.rs` (including its `#[cfg(test)] mod tests`) and `crates/storage/tests/binding.rs`.

New criteria (AC1-AC8 stand; AC9 adds to AC4, AC11 adds to AC3):
- [ ] AC9 (99-AC64, adds to AC4) On unix, a linked worktree verifies only if the worktree folder itself (the directory holding the `.git` file) is owned by the trusted common dir's uid, in addition to AC4's ctime and file-uid conditions. Fail closed: an owner that cannot be read, or a mismatch, means unverified. The owner lookup goes through a private seam, for example `fn worktree_owner_ok_with(folder: &Path, expected_uid: u32, owner_of: impl Fn(&Path) -> io::Result<u32>) -> bool`; production passes `std::fs::metadata(..)?.uid()` (or `symlink_metadata`, consistent with AC4's no-symlink rule). Windows is unchanged; its residual stays as recorded in task 197. Unit tests in vault_binding.rs's `#[cfg(test)]` module, `cfg(unix)`, unprivileged (no chown, no root): `h3b_foreign_owned_worktree_folder_not_verified` (owner lookup returns a uid other than the common dir's), `h3b_unreadable_worktree_owner_not_verified` (owner lookup returns an error).
- [ ] AC10 (STYLE) The `.expect("row present")` at crates/storage/src/vault_binding.rs:454 (task-branch line) carries a comment proving the row is present at that point, or is replaced by code that cannot panic. No other `expect`/`unwrap` in non-test code of vault_binding.rs lacks such a comment.
- [ ] AC11 (P3, adds to AC3) A marker write also drops this registry's keys belonging to its dead rows for the same root, so repeated re-ids of one root never fill the marker toward `MARKER_MAX_ENTRIES`. Other registries' well-formed entries are still never removed. Test `h3b_dead_row_keys_dropped_for_same_root` in `tests/binding.rs`: re-id one root more than `MARKER_MAX_ENTRIES` times and the next bind still succeeds, with one key of this registry left in the marker.

Changed and new checks (`h3b-tests` below replaces the original; all others stand):
- `h3b-tests` command — AC1, AC2, AC3, AC4, AC5, AC11 (13 on unix: the 12 above plus `h3b_dead_row_keys_dropped_for_same_root`)
  run: `cargo test -p wisp-storage --locked --test binding h3b_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[3-9]|[2-9][0-9]|[1-9][0-9]{2,}) passed; 0 failed; 0 ignored'`
- `h3b-owner-unit` command — AC9 (unix; 2 tests)
  run: `cargo test -p wisp-storage --locked --lib h3b_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `owner-check` read — AC9
  how: the reviewer confirms the production path calls the seam with the real folder owner on every unix verification path, before any one-click result; that a read error yields unverified; and that the seam is private and not reachable from outside the crate.
- `expect-proof` read — AC10
  how: the reviewer reads every non-test `expect`/`unwrap` in vault_binding.rs and finds a proof comment on each, the one at :454 included.

Follow-ups, not in this amendment:
- Cross-registry marker write race, now also reachable from `check`'s re-key; needs a cross-registry lock.
- The uid half of AC4's check (file uid against the common dir's uid) had no test before this amendment; AC9's tests cover only the folder owner.
- `LooksLikeCopy`'s `Display` prints `bound_root` unescaped (crates/storage/src/vault.rs:579, outside Owns).
- Stale rebind doc comment in crates/storage/src/vault.rs (~:808, outside Owns).
- `git worktree repair` makes worktrees read as unverified; record in task 197.

## Wave resumed — user chose Amendment 3 (2026-11-20)

## Supersession note (2026-11-20)

Amendment 1's narrowed AC5 (about :75 of this file) supersedes the spec's 99-AC65 "implement the refusal; do not only document it" (docs/plan/specs/2026-11-19-wisp-0-5-0.md:264) for one case: a planted `.git` at a bound repo-root checkout. The user decided that narrowing in Amendment 1. That case stays a documented residual; task 197 documents it (docs/plan/tasks/197-docs-0-5-0.md:53,70).

Follow-up for the scribe at record time: add a supersession note to spec :264, in the same form as the one at spec :164.

## Wave verified — bedafd74496fa86ef7d0d3facb4c945debdb940c
(tester PASS; reviewer APPROVE on AC1-AC6 and AC8-AC11; AC7 win-ci read needs the PR's Windows CI log — to be confirmed at step 5)

## Outcome

Recorded on the PR branch. Verified at bedafd74496fa86ef7d0d3facb4c945debdb940c: tester PASS, reviewer APPROVE on AC1-AC6 and AC8-AC11. AC7's Windows read (`h3b_win_com1_refused`, `h3b_win_trailing_dot_refused`) waits on the PR's Windows CI. Follow-ups are in PLAN under "Follow-ups from task 185". The spec :264 supersession note was added at record time.
