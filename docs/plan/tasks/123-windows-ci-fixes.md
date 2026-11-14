# 123 — Make the Windows CI leg green

**Repo:** .
**Depends on:** 122 (stacked: the worktree starts from `task/122-windows-ci-job`'s head)
**Owns:**
- crates/storage/src/** — only `cfg(windows)` code, `cfg` gating, and `#[cfg(test)]` modules
- crates/storage/tests/**
- crates/core/src/** — only `cfg(windows)` code, `cfg` gating, and `#[cfg(test)]` modules
- crates/core/tests/**
- crates/app/src/** — only `cfg(windows)` code, `cfg` gating, and `#[cfg(test)]` modules
- .gitattributes (new; only if CRLF checkout breaks a test or fixture)

## Goal
Make task 122's Windows CI leg pass clippy and the full test suite. Fix only the
Windows-only lint, compile and test failures it reports. Do not change behaviour
on Linux or macOS. This is the last code gate before the 0.2.0 tag.

## Before the first attempt
The orchestrator adds task 122's failure list (its PR description, AC4) to the
Context below. After that, the criteria freeze.

## Context
- Known before 122 ran (from PLAN.md follow-ups):
  - 54-15 `crates/storage/tests/vault.rs` ~2222 `file_mode_env` is dead on Windows.
  - 54-16 `crates/storage/src/vault_binding.rs` ~405 has a nested if-let (collapsible_if).
  - 52-W covers the uncompiled `cfg(windows)` tests and `simplify_verbatim`.
  - In storage-safety follow-ups, `sidecar_rejects_8dot3_short_name_of_sibling_toml`
    should return early when `B~1.TOM` does not exist.
  - 52-47: `canonicalize` on Windows volumes.
- 122's failure list: 122's first Windows run 43612434572 (PR #67) failed at
  Clippy with `clippy::collapsible_if` at crates/storage/src/vault_binding.rs:405,
  which stopped `wisp-storage` compiling. The test step did not run, so the rest
  of the list was found iteratively by this task (see PR #71).
- docs/conventions.md#tests and #errors — the test and error rules for the fixes.

## Acceptance
- [ ] AC1 On this branch's head, CI's Windows leg concludes `success`, and so
  do the Linux and macOS legs and the `notices` job.
- [ ] AC2 Each fix falls into exactly one of these kinds: `cfg` gating of
  platform-specific code or tests; a lint fix in Windows-only code; a test
  that returns early when a precondition the test does not control is missing
  (8.3 names off, for example); or a Windows-only product fix of at most about
  20 lines inside a `cfg(windows)` block. The PR description lists each fix
  with its kind.
  *Note (2026-11-14, orchestrator):* a fifth allowed kind is a
  test-portability fix: a test or test helper made OS-neutral (absolute-path
  fixtures via `temp_dir()`, path-separator normalisation, and pinning fixture
  line endings via `.gitattributes`). Owns already allows `#[cfg(test)]` edits
  and `.gitattributes`. The fixes at crates/app/src/settings.rs:378,
  crates/storage/src/data_dir.rs:128, crates/storage/src/ops.rs:740 and
  /.gitattributes:2 are this kind.
- [ ] AC3 No test is deleted, `#[ignore]`d or wrapped in `cfg(not(windows))`
  just to hide a failure. A Windows-only product bug that needs more than AC2's
  small fix is reported as a successor task and the work stops. It is not
  folded in here.
- [ ] AC4 Production code outside `cfg(windows)` blocks is unchanged. The
  Linux and macOS test counts on this head equal those on 122's head.
- [ ] AC5 If any non-test Windows code changed, a `measure.yml` run on this
  head reports a Windows `binary_bytes` of at most 15,500,000. Its run id is
  in the PR description.
- [ ] AC6 `Cargo.toml`, `Cargo.lock` and `.github/**` are unchanged.

## Checks
cargo writes no JUnit XML here (task 99 Checks), so these are commands.
- `ci-green` command — AC1
  run: `gh run watch <id> --interval 60 --exit-status` on the branch head's CI run
- `local-suite` command — AC4
  run: `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-123 cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `windows-lint-local` command — AC2
  run: `cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings`
  (best effort from macOS; if GPUI or ring will not cross-check, say so and rely on `ci-green`)
- `fix-kinds` read — AC2, AC3, AC4
  how: every hunk outside tests sits inside `cfg(windows)` or changes only a `cfg` attribute. No `#[ignore]` is added.
- `size` command — AC5
  run: `gh workflow run measure.yml --ref task/123-windows-ci-fixes`, then `gh run watch <id> --interval 60`
- `scope` command — AC6
  run: `git diff --exit-code task/122-windows-ci-job...HEAD -- Cargo.toml Cargo.lock .github`

## Out of scope
- Task 99's Windows security ACs (AC68 credential persistence, AC69 service
  naming, AC71 verbatim I/O). They stay in the post-0.2.0 backlog even if a
  test brushes against them.
- keyring `map_error` reclassification (114 follow-up (a)), unless a test fails
  because of it. If one does, apply AC3.
- CI workflow changes. If the job itself is wrong, report back to 122.
