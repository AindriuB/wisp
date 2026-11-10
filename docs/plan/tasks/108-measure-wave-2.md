# 108 — Measure 0.2.0 wave 2 against the size gate

**Repo:** `.`
**Depends on:** 100, 101, 102, 103, 104, 105, 106, 107 (all merged to main)
**Size impact:** this is the gate
**Owns:**
- scripts/measure/results/aarch64-apple-darwin.json (only if a local macOS run is recorded, as earlier measure tasks did)

## Goal
Measure the merged wave-2 build on all three CI targets and fail the wave if the Windows release binary exceeds 15.5 MB (15,500,000 B), per the spec's size gate. Wave 1 ended at 14,514,688 B (task 98), leaving about 1 MB. Linux and macOS figures are recorded and must stay within `scripts/measure/budgets.json`. A failure is resolved by dropping or replacing the dependency responsible, in a new task. It is never resolved by raising a budget, and never by a change made in this task.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#size-gate
- .github/workflows/measure.yml (dispatch only; one artifact per target, `measure-<target>.json`, field `binary_bytes`)
- scripts/measure/budgets.json — the per-target limits for binary size, idle RSS and after-request RSS
- Per-task Windows figures reported in the PRs of 103, 104 and 105. Use them to attribute the wave's growth.
- Do not run `scripts/measure/measure.sh` locally against the user's real data. If you run it on macOS, set `WISP_DATA_DIR` to a fresh `mktemp -d` and stop only the pid it starts. Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-108`.

## Acceptance
- [ ] AC1 The measure workflow ran on this task's branch, cut from main after 107 merged. The PR cites the run URL and the main commit it measured.
- [ ] AC2 The Windows `binary_bytes` is at most 15,500,000.
- [ ] AC3 Every target's `binary_bytes`, idle RSS and after-request RSS are within `budgets.json`.
- [ ] AC4 The PR has a table: per target, the wave-1 figure (task 98) next to the wave-2 figure and the delta. A second table gives the Windows delta attributed to 100+103, 104 and 105 from their PRs, plus any unexplained remainder.
- [ ] AC5 If AC2 or AC3 fails, the PR is marked failed, names the dependency most responsible (by `cargo bloat` or the per-task deltas), and proposes the drop or replacement as a follow-up. No code or budget is changed here.

## Checks
- `measure-run` command — AC1
  run: `gh workflow run measure.yml --ref task/108-measure-wave-2 && gh run list --workflow measure.yml --branch task/108-measure-wave-2 -L 1 --json databaseId,url`
- `windows-gate` command — AC2
  run: `gh run download <run-id> -n measure-x86_64-pc-windows-msvc -D <scratch> && jq -e '.binary_bytes <= 15500000' <scratch>/measure-x86_64-pc-windows-msvc.json`
- `budgets` command — AC3
  run: for each target, `gh run download <run-id> -n measure-<target> -D <scratch>`, then compare every metric in `scripts/measure/budgets.json` against its limit with `jq -e`.
- `report` read — AC4, AC5
  how: both tables are present and the deltas add up to the measured change within the stated remainder.

## Out of scope
- Wave 3 (multiple collections, keychain) and task 99 hardening.
- Any size fix. A failed gate gets its own task.
- Budget changes in `docs/architecture.md` or `budgets.json`.
