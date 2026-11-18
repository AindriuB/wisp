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

## Review round 1 — findings
Reviewed head: 604beaf21c4d82f4e257d4688a957cfd8edd72c6

- [P1] crates/storage/src/win_acl.rs:56 (also :104, :152, :153) — the build will fail on Windows. Every `LocalFree(x as isize)` call passes an `isize`, but in windows-sys 0.52.0 `LocalFree` takes `HLOCAL`, which is `*mut c_void` (windows-sys-0.52.0/src/Windows/Win32/Foundation/mod.rs:10239, fn at :8). Only `HANDLE` is `isize` in 0.52. That makes it a type mismatch (E0308), so wisp-storage, and with it the whole workspace, does not compile on the windows-latest leg.
  route: implementer — fix by passing `sid_str.cast()`, `sd`, `my_sid` (already `*mut c_void`), or `as HLOCAL` / `.cast()`.
  input: `cargo check --target x86_64-pc-windows-msvc -p wisp-storage`  →  error[E0308] mismatched types, expected `*mut c_void`, found `isize`

- [P2] crates/storage/src/data_dir.rs:127-131 — on Windows, `ensure_private_dir` now calls `SetNamedSecurityInfoW(DACL|PROTECTED_DACL)` on every call, not only when the dir is wider than wanted (the unix branch at :116-122 only chmods when the mode differs). Microsoft documents that SetNamedSecurityInfo pushes inheritable ACEs down to every existing child object, so each call rewrites security on the whole subtree. Hot paths call it: `HistoryStore::lock_file` (history.rs:1179), `append` (history.rs:1222) on every request sent, `cookies.rs:145/:317` on every cookie lock and save, and `vault_binding.rs:99` `lock_registry` on `data.root()`, which is the entire data dir.
  route: implementer — read the DACL first and call `restrict_to_owner` only when it is not already protected with the single owner ACE (or when the dir was just created). That matches the unix "tighten when wider" behaviour and AC1 ("existing dirs are tightened") without changing any specified behaviour.
  input: a data dir whose history holds 20 collections × 500 entries, then send one request  →  each `append` does two full-tree ACL propagations (about 20k files for the history root), plus a full data-dir propagation on every vault-registry lock.

- [STYLE] crates/storage/src/data_dir.rs:90 — the doc-comment line is 134 characters long; neighbouring doc comments wrap within 100 columns. The next line also still describes only the unix 0700 tightening.
  route: implementer

## Wave verified — 0889d6cc708521e09af10510ca8333304100d282

## Attempt 2 — failed
CI run 44295053451 on branch head 0889d6c (push CI, before any PR). Windows now compiles; ubuntu and macOS checks are green. Two failures, both in what this task changed:
1. `notices` — "Fail if THIRD-PARTY-NOTICES is out of date": the diff wants `+  - windows-sys 0.52.0` under the Apache-2.0 "Used by:" list (Apache-2.0 count 476 → 477). THIRD-PARTY-NOTICES was not regenerated after the Cargo.lock change (docs/conventions.md:87-90). Regenerate it the way the notices job does; that file is outside Owns, so this is the one addition allowed for it.
2. `check (windows-latest)` Test — `request::tests::sidecar_rejects_8dot3_short_name_of_sibling_toml` panicked at crates\storage\src\request.rs:1724:13: "8.3 short names are off on this volume but WISP_REQUIRE_8DOT3 is set". The env var reached the test (`WISP_REQUIRE_8DOT3: 1`), so the ci.yml setup step did not actually turn on 8.3 names for the volume the test's temp dir is on. Check which drive the test's temp dir is on (the runner's TEMP may not be C:, and `fsutil 8dot3name set` per volume vs registry-wide), that the step runs with admin rights, and that it applies before the temp dir is created. AC2/AC5 require the test to really run, so a skip is not acceptable.
Owns note: THIRD-PARTY-NOTICES added to Owns for attempt 2 by the orchestrator (generated file; conventions require it to follow Cargo.lock).

## Attempt 3 — failed
CI run 44296829231 on branch head 89bd3a4. Notices now pass; ubuntu and macOS green. `check (windows-latest)` still fails `request::tests::sidecar_rejects_8dot3_short_name_of_sibling_toml` at request.rs:1724 ("8.3 short names are off on this volume but WISP_REQUIRE_8DOT3 is set").
Cause, from the setup step's probe output: 8.3 names ARE on (`8dot3 name creation is ENABLED on "C:"`; the per-volume `set` now prints "Error -2147024592: Short name settings may not be changed on this volume due to the global registry setting", harmless since the registry is 0). `dir /x` of a fresh TEMP dir holding `b.toml` shows its short name as `B9388~1.TOM`, not `B~1.TOM`. Windows generated a hashed short name, so the test's probe `probe.path().join("B~1.TOM").exists()` is false and the test wrongly concludes 8.3 is off.
Fix: do not hard-code `B~1.TOM`. Read the real short name of the sibling `b.toml` (e.g. `GetShortPathNameW`, under the existing windows-sys features, or `FindFirstFileW`'s `cAlternateFileName`), treat "short name equals the long name / none" as 8.3 off, and pass that real short name to `alias_rejected`. Keep the SKIPPED/RAN lines (RAN should print the short name it checked) and the hard fail under WISP_REQUIRE_8DOT3. Make the ci.yml step tolerate the per-volume error now that the registry setting is applied (or drop the per-volume `set`), and keep it from failing the step.

## Wave verified — 23ac3cb6b0d393639e5e22e08b066ee00bf40c62

## Wave stopped — step 3 — new flake: run::tests::run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7
Branch push CI run 44299517411 at 23ac3cb: check (ubuntu-24.04) failed `crates/service/src/run.rs:985` ("exit 5", left `Completed`, right `Failed(Connect)`); the failed-job re-run passed with no change. Not in docs/plan/known-flakes.md, and 171 does not touch crates/service. All four jobs green after the re-run. Head unchanged; on acceptance, resume at /record.

## Wave resumed — user accepted the ubuntu flake as unrelated; resume at /record; scribe adds it to known-flakes.md

## Outcome
Recorded 2026-11-18 at 23ac3cb. Tester PASS and reviewer APPROVE on the code; AC5 and AC6 rest on CI run 44299517411 at that SHA, all four jobs green (the Windows job ran `sidecar_rejects_8dot3_short_name_of_sibling_toml` for real under `WISP_REQUIRE_8DOT3=1`, which hard-fails on a skip). It took three attempts: a `LocalFree` type mismatch, then a stale `THIRD-PARTY-NOTICES`, then a hard-coded `B~1.TOM` that Windows replaces with a hashed short name. The ubuntu leg failed once on an unrelated flake (`run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7`) and passed on re-run.
