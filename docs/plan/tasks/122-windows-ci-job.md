# 122 — Add a Windows test and clippy job to CI

**Repo:** .
**Depends on:** none
**Owns:**
- .github/workflows/ci.yml

## Goal
CI has never compiled, linted or run the workspace's tests on Windows, so the
`cfg(windows)` code in `crates/storage` (vault_binding, keychain, request) is
unverified (follow-up 54-CI, task 99 AC38). Add a `windows-latest` leg that runs
the same clippy and test commands as Linux and macOS. This task adds the job and
takes its first complete reading; fixing what it finds is task 123.

## Context
- .github/workflows/ci.yml:14-49 — the `check` matrix; adding `windows-latest`
  to it is the expected shape. A separate job is acceptable if a step cannot be
  shared, but fmt, clippy and test must use the same commands as the matrix.
- .github/workflows/release.yml:62-65 — toolchain install that already works on
  `windows-latest` (the release exe builds there).
- docs/plan/tasks/99-hardening-0-2-0.md AC38 — the original criterion.
- docs/plan/PLAN.md "Follow-ups from task 114/115" — 54-15, 54-16, 52-W, 52-47
  and the 8.3 short-name test are the expected first failures.
- Checkout on Windows converts line endings to CRLF (PLAN residual 72). If a
  test fails only because of that, record it for task 123; do not add
  `.gitattributes` here.

## Acceptance
- [ ] AC1 `ci.yml` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`
  and `cargo test --workspace --locked` on `windows-latest` on every push and
  pull request, with `fail-fast: false` kept so a Windows failure does not
  cancel Linux or macOS.
- [ ] AC2 The Windows leg sets a `timeout-minutes` large enough for a cold
  Windows build (at least 60) and uses no `continue-on-error`.
- [ ] AC3 The Ubuntu and macOS legs run the same steps as on main 17176a6
  (no step removed, renamed or reconditioned for them).
- [ ] AC4 At least one CI run of the branch head has completed the Windows leg
  (success or failure, not cancelled or timed out). The PR description lists
  the run id and every failing clippy lint and test by `path:line` or test
  name, which becomes task 123's input.
- [ ] AC5 Nothing outside `ci.yml` changes.

## Checks
cargo writes no JUnit XML in this repo (task 99 Checks), so these are commands.
- `lint` command — AC1, AC2
  run: `actionlint .github/workflows/ci.yml`
- `legs-unchanged` read — AC3
  how: `git diff main...HEAD -- .github/workflows/ci.yml` touches only the
  matrix `os` list, Windows-conditioned steps and the timeout.
- `windows-run` command — AC4
  run: `gh run list --branch task/122-windows-ci-job --workflow CI --limit 1 --json databaseId,status`
  then `gh run watch <id> --interval 60`; the Windows job's conclusion is
  `success` or `failure`, never `cancelled` or `timed_out`.
- `scope` command — AC5
  run: `git diff --exit-code main...HEAD -- . ':!.github/workflows/ci.yml'`

## Out of scope
- Any fix to Rust code, tests or `.gitattributes` (task 123).
- Making the Windows leg green. AC4 asks for a completed reading, not a pass.
  The 0.2.0 gate is green Windows CI after task 123.
- `release.yml`, `measure.yml` and caching (`Swatinem/rust-cache` or similar):
  a cache action is a new third-party dependency and needs its own decision.

## Merge order
The user merges 122 and 123 back to back. 123 is stacked on 122's branch, so
main does not carry a red Windows leg between the two merges.
