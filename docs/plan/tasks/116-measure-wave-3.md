# 116 — Measure 0.2.0 wave 3 against the size gate

**Repo:** `.`
**Depends on:** 110, 111, 112, 113, 114, 115 (all merged to main)
**Size impact:** this is the gate
**Owns:**
- scripts/measure/results/aarch64-apple-darwin.json (only if a local macOS run is recorded, as earlier measure tasks did)

## Goal
Measure the merged wave-3 build on all three CI targets. The wave fails if the Windows release binary exceeds 15,500,000 B. Wave 2 ended at 15,136,256 B (task 108, run 42890358687). Linux and macOS figures are recorded and must stay within `scripts/measure/budgets.json`. A failure is resolved by dropping or replacing what caused it, in a new task. It is never resolved by raising a budget, or by a change made here.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#size-gate
- .github/workflows/measure.yml (dispatch only; one artifact per target, `measure-<target>.json`, field `binary_bytes`)
- scripts/measure/budgets.json
- The per-task Windows figures in the PRs of 109 (the prediction: `keyring` +26,112 B on Windows and +290,960 B on Linux, PR #51), 114 and 115. Task 112 reports RSS for 1 and 8 open collections.
- If you run `scripts/measure/measure.sh` locally on macOS, set `WISP_DATA_DIR` to a fresh `mktemp -d` and `WISP_KEYCHAIN_SERVICE=wisp-test-116`, and stop only the pid it starts. Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-116`.

## Acceptance
- [ ] AC1 The measure workflow ran on this task's branch, cut from main after 115 merged. The PR cites the run URL and the main commit it measured.
- [ ] AC2 Windows `binary_bytes` is at most 15,500,000.
- [ ] AC3 Every target's `binary_bytes`, idle RSS and after-request RSS are within `budgets.json`.
- [ ] AC4 The PR has two tables. The first gives, per target, the wave-2 figure (task 108) beside the wave-3 figure and the delta. The second attributes the Windows delta to 112 and 113, 114 and 115 from their PRs, compares it with 109's prediction, and states any unexplained remainder.
- [ ] AC5 If AC2 or AC3 fails, the PR is marked failed. It names the dependency most responsible, using `cargo bloat` or the per-task deltas, and proposes dropping or replacing it as a follow-up. No code or budget is changed.

## Checks
- `measure-run` command — AC1
  run: `gh workflow run measure.yml --ref task/116-measure-wave-3 && gh run list --workflow measure.yml --branch task/116-measure-wave-3 -L 1 --json databaseId,url`
- `windows-gate` command — AC2
  run: `gh run download <run-id> -n measure-x86_64-pc-windows-msvc -D <scratch> && jq -e '.binary_bytes <= 15500000' <scratch>/measure-x86_64-pc-windows-msvc.json`
- `budgets` command — AC3
  run: for each target, `gh run download <run-id> -n measure-<target> -D <scratch>`, then compare every metric in `scripts/measure/budgets.json` with `jq -e`.
- `report` read — AC4, AC5
  how: both tables are present, and the deltas add up to the measured change within the stated remainder.

## Out of scope
- Task 99 (hardening) and PLAN follow-ups.
- Any size fix; a failed gate gets its own task.
- Budget changes in `docs/architecture.md` or `budgets.json`.
