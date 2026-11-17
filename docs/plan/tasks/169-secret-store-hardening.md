# 169 — Secret store and binding hardening (2D, H3)

**Repo:** .
**Round:** 0
**Depends on:** 165
**Owns:**
- crates/storage/src/vault.rs
- crates/storage/src/vault_binding.rs
- crates/storage/src/keychain.rs
- crates/storage/src/environment.rs
- crates/storage/tests/vault.rs
- crates/storage/tests/keychain.rs
- crates/app/src/vault.rs (AC72: the no-data-dir fallback at :570-578; AC1: the `import_secrets_file`/`migrate_to_file` call sites only)

## Goal
Close hardening theme H3: secret imports and migrations never overwrite or leak
unasked, bindings trust only what they read from disk and fail closed, file
operations do not follow swapped-in symlinks, keychain items are local and
unambiguous, and keychain flows refuse to run without a per-user data dir.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" H3 row (:419) and the AC72 note (:426), invariants (:435: tests never touch the real keychain), Waves 2D (:470).
- docs/plan/tasks/99-hardening-0-2-0.md AC53 :104, AC54 :105, AC55 :106, AC56 :107, AC59 :110, AC66 :117, AC67 :118, AC68 :119, AC69 :120, AC72 :123 (line refs are 0.2.0; re-locate).
- crates/storage/src/vault.rs:600 `bind`, :627 `share_with_folder`, :780 `import_secrets_file`, :825 `migrate_to_file`; `MemoryStore`/`NoStores` :100-125.
- crates/storage/src/keychain.rs:88 `open`, :102 `open_with` (test backend).
- crates/storage/src/environment.rs:406 (the double `sp.exists()`; AC66).
- crates/app/src/vault.rs:570-578 builds `DataDir::at(temp/wisp-no-data-dir)` when there is no data dir (AC72).
- The keychain service default stays `wisp` (`wisp_service::secrets`, not owned): renaming it would orphan stored items.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 169 secret-store-hardening`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-169`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (AC53) `import_secrets_file` returns a typed conflict listing names whose stored values differ and changes nothing until called again with confirmation; a git-tracked source file is refused. `migrate_to_file` refuses (or returns a typed warning for) a git-tracked target. Tests: a differing value leaves the store unchanged until confirmed; a tracked file is refused (test creates a temp git repo, skipping with a printed message only if `git` is absent). The app does not confirm: on a conflict it imports nothing and its existing notice path names the differing secrets (names only).
- [ ] AC2 (AC54) The binding check reads the collection id from the root's `collection.toml`; a caller-supplied id that differs gives a fail-closed result. Test.
- [ ] AC3 (AC55) A root path that is not valid UTF-8 fails closed; no registry key uses `to_string_lossy`. Unix test: two distinct non-UTF-8 roots never share a binding.
- [ ] AC4 (AC56) Binding-marker and secrets-file reads and writes open without following a final symlink (O_NOFOLLOW on unix, or a verified handle), including the `collection.toml` check-then-write. Tests swap a symlink in after the check and show the operation fails.
- [ ] AC5 (AC59) The copy check uses `try_exists`, fails closed on every error except NotFound, and compares the marker nonce. Tests: a metadata error fails closed; a genuine move succeeds.
- [ ] AC6 (AC66) The secrets-path existence is evaluated once and that result used for both decisions; a test shows a file present at the second point is still checked.
- [ ] AC7 (AC67) `new_root` is canonicalised once and verify and bind (and `share_with_folder`) run under one lock. Test: swapping the path between check and bind refuses or binds the checked folder.
- [ ] AC8 (AC68) Windows keychain items use Local persistence (not Enterprise/roaming); a `cfg(windows)` unit test or a code citation in the PR shows the setting.
- [ ] AC9 (AC69) `keychain::open` rejects an empty service and a service containing `.`, returning an inert store with a typed reason; the Linux Ambiguous outcome is documented in the doc comment. Tests cover `""`, `"."`, `"a.b"` and accept `"wisp"`.
- [ ] AC10 (AC72) With no per-user data dir, keychain bind, move, import and load-from-store flows refuse with a typed error; the shared `wisp-no-data-dir` temp path is gone from `crates/app/src/vault.rs`. Test: a load with no data dir of a keychain-mode environment returns the refusal, not secrets.
- [ ] AC11 Tests use `MemoryStore`, `NoStores` or `open_with` test backends only; no file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `vault-h3` command — AC1, AC2, AC3, AC4, AC5, AC6, AC7, AC10
  run: `cargo test -p wisp-storage --locked h3_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (prefix the new tests with `h3_`.)
- `keychain-h3` command — AC9
  run: `cargo test -p wisp-storage --locked --test keychain service 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `no-shared-tmp` command — AC10
  run: `! rg -n 'wisp-no-data-dir' crates`
- `scope` command — AC11
  run: `git diff --exit-code main...HEAD -- crates/core crates/cli crates/service crates/app/src/workspace.rs crates/storage/src/history.rs crates/storage/src/import crates/storage/src/data_dir.rs crates/storage/src/fs.rs`
- `suite-and-lint` command — AC11
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `windows-local` read — AC8
  how: the reviewer finds the Local persistence setting in `keychain.rs`.

## Out of scope
- Renaming the keychain service; `run --keychain`'s exit 4 on refusal (166 maps any store failure to 4).
- A GUI confirm dialog for import conflicts (follow-up); any GUI wording beyond the notice text for typed errors.
