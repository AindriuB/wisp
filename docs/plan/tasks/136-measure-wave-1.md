# 136 — Measure wave 1: GUI size unchanged, first `wisp-cli` size and its limit

**Repo:** .
**Depends on:** 130, 131, 132, 133, 134, 135
**Owns:**
- .github/workflows/measure.yml
- scripts/measure/cli-budgets.json (new)
- scripts/measure/results/** (new result files only)

## Goal
Prove wave 1 left the Windows GUI exe within its gate, record the first
`wisp-cli` size on every measured target, and set the CLI's initial 4 MiB
limit in its own budget file. Runs on the main head after tasks 130-135 merge.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Binary budget", "Size gate", "Release packaging" (last bullet: `cli-budgets.json` separate from `budgets.json`)
- .github/workflows/measure.yml:20-35 matrix; :71 build; :124-131 Windows size check (15,500,000 B)
- scripts/measure/budgets.json — read by both measure scripts; must not change
- Baseline: Windows `wisp.exe` 15,181,824 B (spec). If `task/theme-palette` merged before this task starts, the baseline is a measure run on that merge commit instead; state which in the PR.
- Worktree `wt-new.sh . 136 measure-wave-1`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-136`

## Acceptance
- [ ] AC1 `measure.yml` builds `-p wisp-app` (size recorded), then `-p wisp-app -p wisp-cli` in one invocation, and fails if the GUI binary's byte size differs between the two builds.
- [ ] AC2 A step reads `scripts/measure/cli-budgets.json` and fails when `wisp-cli` exceeds its target's limit. The file is `{"limits": {"<target triple>": 4194304, ...}}` with one entry per matrix target.
- [ ] AC3 The CLI binary size is uploaded with the existing per-target artifact.
- [ ] AC4 A `measure.yml` dispatch on the branch completes green on every matrix target; the run URL is in the PR.
- [ ] AC5 `scripts/measure/results/` gains one file for this run recording, per target: GUI bytes, CLI bytes, the baseline commit and GUI delta. The PR states the Windows GUI figure against the 15,500,000 B gate and the 15,400,000 B soft line.
- [ ] AC6 `scripts/measure/budgets.json`, `measure.sh`, `measure.ps1` and `release.yml` are unchanged.

## Checks
- `measure-run` command — AC1-AC4
  run: `gh run view <run-id> --json conclusion -q .conclusion | grep -x success`
- `untouched` command — AC6
  run: `git diff --exit-code main...HEAD -- scripts/measure/budgets.json scripts/measure/measure.sh scripts/measure/measure.ps1 .github/workflows/release.yml`
- `figures` read — AC5
  how: the result file's Windows GUI bytes are at most 15,500,000 and match the run's log; the delta and soft-line status are stated.

## Out of scope
- The CLI size step in `release.yml`, packaging and signing: 3B.
- Recording the budgets in `docs/architecture.md#budgets`: the scribe, after the user approves.
- Any size reduction: if the gate fails, stop and report.

## Review round 1 — findings
Reviewed HEAD 0001494.
- [P2] scripts/measure/results/wave-1-size.json:5,9,11 — macOS/Linux deltas recorded as null and "no baseline" though run 43622975912 (commit a02a779) measured macOS 11,601,344 B and Linux 25,812,488 B; no baseline commit recorded (AC5 asks for per-target baseline commit and GUI delta).
- [P2] same file :5 — baseline is not the one the task file's Context requires: theme-palette merged first (4323fbe, PR #79), so the baseline must be a measure run on 4323fbe; none exists. The recorded +265,216 B also includes theme #79, 128 and 129.
- [P3, follow-up] measure.yml "Check CLI size limit" — a non-integer limit passes silently (`[` exit 2 inside `if`).
