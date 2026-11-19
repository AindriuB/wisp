# 180 — Let a Windows data dir on a volume without ACLs work (portable FAT/exFAT)

**Repo:** .
**Round:** 0
**Depends on:** 171
**Owns:**
- crates/storage/src/data_dir.rs (`ensure_private_dir` and its tests)
- crates/storage/src/win_acl.rs (the volume-capability query, the skip, and tests)
- crates/storage/Cargo.toml (only to add a `windows-sys` feature if `GetVolumeInformationW` needs one 171 did not enable)

## Goal
171 makes `ensure_private_dir` set an owner-only DACL on Windows and fail if it
cannot. A portable install (data dir `<exe-dir>/data`, docs/architecture.md:188)
on a FAT or exFAT USB stick has no ACLs at all, so history and cookies would fail
there. On a volume that reports no persistent-ACL support, skip the DACL and warn
once; on an ACL-capable volume a DACL failure is still an error.

## Context
- crates/storage/src/data_dir.rs:92 `ensure_private_dir`; crates/storage/src/win_acl.rs (both as 171 leaves them; re-locate after 171 merges).
- Win32: `GetVolumePathNameW` for the path's volume root, then `GetVolumeInformationW`; the flags word lacks `FILE_PERSISTENT_ACLS` (0x8) on FAT/exFAT.
- crates/storage/src/history.rs:1222, cookies.rs:145/317, vault_binding.rs:99/130 — callers (read only; unchanged).
- Warn with `tracing::warn!` (already a storage dependency), guarded by a `std::sync::Once` or `AtomicBool` so it fires once per process.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 180 windows-no-acl-volume`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-180`. Foreground commands only; never `pkill` or `killall`. Windows code is proved by the Windows CI leg; locally run `cargo check --target x86_64-pc-windows-msvc -p wisp-storage` if the target is installed.

## Acceptance
- [ ] AC1 On Windows, before setting the DACL, `ensure_private_dir` asks whether the path's volume supports persistent ACLs. When the flags lack `FILE_PERSISTENT_ACLS`, it creates the directory, sets no DACL, returns `Ok(())`, and logs one `tracing::warn!` per process naming the reason (no ACL support on the volume) and no file contents.
- [ ] AC2 When the volume supports ACLs, behaviour is exactly 171's: a DACL failure returns `Err`. When the capability query itself fails, it is treated as ACL-capable (fail closed: the DACL is attempted and its failure is an error).
- [ ] AC3 The decision is a pure function (for example `fn acl_policy(volume_flags: Option<u32>) -> AclPolicy`) compiled on every platform and unit-tested there: flags without 0x8 → skip; flags with 0x8 → apply; `None` (query failed) → apply. At least 3 tests, named `no_acl_volume_*`.
- [ ] AC4 A `cfg(windows)` test, named `no_acl_volume_*`, checks that the runner's temp volume (NTFS) reports persistent ACLs, so the real query is exercised in CI and the DACL path from 171 still runs there.
- [ ] AC5 Unix behaviour is unchanged; no new crate or `windows-sys` version (a new feature on the existing entry is allowed); `Cargo.lock` unchanged; no file outside Owns changes; fmt, clippy and the workspace suite pass on all three CI legs.

## Checks
- `policy-tests` command — AC3
  run: `cargo test -p wisp-storage --locked no_acl_volume_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `ci-green` command — AC1, AC2, AC4, AC5
  run: `gh pr checks <pr> --watch --fail-fast`
- `scope` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/data_dir.rs$' -e '^crates/storage/src/win_acl.rs$' -e '^crates/storage/Cargo.toml$' | wc -l | tr -d ' ')" = 0 && git diff --exit-code main...HEAD -- Cargo.lock`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `fail-closed` read — AC1, AC2
  how: the reviewer checks that only an explicit "no `FILE_PERSISTENT_ACLS`" answer skips the DACL, a query error falls through to 171's path, and the warning is once-only and names no path contents.

## Out of scope
- Any other permission scheme on non-ACL volumes (FAT has none; do not emulate one).
- ACLs on collection folders or settings next to a portable exe.
- The CLI's or app's user-facing messaging about portable mode; `docs/architecture.md` wording (scribe).
- 171's DACL construction, 8.3 test and `same_file_entry`.

## Review round 1 — findings
Reviewed HEAD: bbfd9dd84dab1d58e50b52724c9596031556cd5e

- [STYLE] crates/storage/src/win_acl.rs:69 — `volume_flags` was inserted between `is_owner_only`'s doc comment and the function it describes. The two-line `is_owner_only` doc ("True when the DACL of `path` is already protected…", lines 67-68) now runs straight into `volume_flags`'s own doc, so rustdoc gives `volume_flags` a four-line doc that wrongly opens with the `is_owner_only` description. `is_owner_only` (line 100) is left with no doc at all.
  route: implementer — move the new function, with its own two-line doc, above line 67 (or below `is_owner_only`), so each doc sits directly over its own fn again.
  rule: the file's own idiom is one `///` block directly above each fn. See `current_user_sid_string` at win_acl.rs:33-34 and `is_owner_only` as 171 left it on main.

## Wave verified — 7c36a28c70ebb7d1456226f0c4df618ea90d9819

## Outcome
Merged at 7c36a28 (merge 207a762). Tester PASS (1535 passed, 4 skipped; fmt and clippy clean; policy-tests 3/3), reviewer APPROVE after one round. The Windows-only code, including the `cfg(windows)` test `no_acl_volume_temp_dir_reports_persistent_acls`, is first compiled and run by the Windows CI leg on the record PR. Follow-ups are in PLAN.md under the task 180 follow-ups.
