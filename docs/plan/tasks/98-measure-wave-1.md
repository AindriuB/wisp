# 98 — Measure wave 1 against the 15.5 MB Windows gate

**Repo:** .
**Depends on:** 91, 92, 93, 94, 95, 96, 97
**Owns:**
- docs/plan/tasks/98-measure-wave-1.md (this file only; no source, script or workflow change)

## Goal
Measure the release binaries with all of wave 1 in, and hold the wave if the
Windows release binary is over 15.5 MB (15,500,000 B). Linux and macOS figures
are recorded and must stay within their budgets in `scripts/measure/budgets.json`.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#size-gate`
- `.github/workflows/measure.yml` — `workflow_dispatch`; uploads `measure-<target>` artefacts holding `measure-<target>.json` with `binary_bytes` and `within_budget`
- `docs/architecture.md#budgets` — last Windows figure 13,630,464 B (task 77)
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/98-measure-wave-1`

## Steps
1. Bring every wave-1 change into this branch: `git merge --no-ff origin/task/<id>-<slug>` for each of 91-97 not already in `origin/main` (run `git fetch origin` first). Never rebase others' branches, never use `git stash`. A conflict is reported, not resolved by editing sources.
2. Push the branch and run `gh workflow run measure.yml --ref task/98-measure-wave-1`. Wait for the run in the foreground with `gh run watch <run-id> --exit-status` in repeated calls under the tool timeout; do not end the turn while it runs.
3. `gh run download <run-id> -D "$TMPDIR/m98"`, then read the three JSON files.

## Acceptance
- [ ] AC1 The Windows `binary_bytes` is at most 15,500,000.
- [ ] AC2 Every target's `within_budget` is true and the run concluded `success`.
- [ ] AC3 The report gives, per target, `binary_bytes`, the change from the last recorded figure (`docs/architecture.md#budgets`), and the idle and after-request memory figures, with the run id and the merged commit.
- [ ] AC4 No file outside this task file changes on the branch except by merging task branches 91-97.

## Checks
- `windows-gate` command — AC1
  run: `jq -e '.binary_bytes <= 15500000' "$TMPDIR"/m98/measure-x86_64-pc-windows-msvc/measure-x86_64-pc-windows-msvc.json`
- `budgets` command — AC2
  run: `gh run view <run-id> --json conclusion -q .conclusion | grep -qx success && for f in "$TMPDIR"/m98/*/measure-*.json; do jq -e '.within_budget == true' "$f"; done`
- `report` read — AC3, AC4
  how: the report has the per-target table and run id; `git log --first-parent` on the branch shows only the task-file commit and merges of 91-97.

## Out of scope
- Fixing an over-budget result. If AC1 fails, report the figure and the top contributors you can name (for example with `cargo bloat` if already installed, else the crates added in 91); dropping or replacing a dependency is a new task.
- Raising any budget.
- Opening a PR for this branch unless the orchestrator asks; it exists to be measured.

## Notes
- Measurement uses CI runners only; nothing launches the app locally.

## Amendment — orchestrator (2026-11-09)

Do not merge branches 91-97 into this branch. Every branch goes through a PR, so 98 starts only after 91-97 have merged into main. Rebase onto origin/main, then dispatch measure.yml (or the release dry run) against main and read its numbers. The gate is unchanged: fail if the Windows binary is over 15,500,000 bytes, or if any target is over its budget. Record the numbers in `## Notes`. No code changes.

## Measured results (run 42823763145, commit 71f9e1a, main with 91-97 merged)

Run: https://github.com/AindriuB/wisp/actions/runs/42823763145 (conclusion success). Gate: Windows binary <= 15,500,000 B: PASS.

| Target | binary_bytes | Change vs last recorded | Idle / after request RSS (KiB) | within_budget |
|---|---|---|---|---|
| aarch64-apple-darwin | 11,049,104 | -8,131,952 vs 19,181,056 (task 63 CI) | 92,112 / 96,768 | true |
| x86_64-pc-windows-msvc | 14,514,688 | +884,224 vs 13,630,464 (task 77) | 41,748 / 50,888 (private 36,004 / 40,732) | true |
| x86_64-unknown-linux-gnu | 24,799,280 | -13,193,720 vs 37,993,000 (task 63 CI) | 180,372 / 175,728 (RssAnon 63,180 / 51,508) | true |
