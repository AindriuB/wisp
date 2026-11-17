# 165 — Measure 0.4.0 wave 1 against the size gate (1M)

**Repo:** .
**Round:** 0
**Depends on:** 159, 160, 161, 162, 163, 164
**Owns:**
- scripts/measure/results/v0.4.0-wave-1-size.json (new)

## Goal
Measure the GUI and `wisp-cli` on the main head once wave 1 has merged, and
show that the service-layer extraction and its wave-1 siblings stayed within the
roadmap's +10 KB GUI target, before wave 2 starts.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Size gate" (:443-445), Feature 1 "Size estimate" (:136-138), Waves 1M (:461).
- Baseline (0.3.0 release head d314c5f, measure run 44045146818, `scripts/measure/results/wave-3-size.json` — the format to follow): Windows GUI 15,533,568 B; CLI macOS 1,415,600 / Windows 1,631,232 / Linux 1,966,744 B. Gate 16,000,000 B, soft line 15,900,000 B; CLI limit 4,194,304 B per target (`scripts/measure/cli-budgets.json`).
- `.github/workflows/measure.yml` (dispatch) records GUI and CLI sizes and checks the CLI limits. Dispatching it needs no approval.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 165 measure-wave-1`; no local build. Foreground commands only; no background command or polling loop; never `pkill` or `killall`. Wait on a run with one foreground `gh run watch <id> --exit-status`.

## Acceptance
- [ ] AC1 A `measure.yml` dispatch on main at the wave-1 head (159-164 merged) completes green on every target; run id and head commit are recorded.
- [ ] AC2 The result file records per target GUI and CLI bytes, the baseline (d314c5f, run 44045146818) and both deltas, and states the Windows GUI against 16,000,000 B, 15,900,000 B and the wave-1 target of baseline + 10,240 B (15,543,808 B).
- [ ] AC3 Over the wave-1 target or the soft line: the PR says so first and cuts nothing. Over the gate: stop and report.
- [ ] AC4 No file other than the result file changes.

## Checks
- `measure-run` command — AC1
  run: `gh run view <run-id> --json conclusion -q .conclusion | grep -x success`
- `only-result` command — AC4
  run: `test "$(git diff --name-only main...HEAD | grep -vx scripts/measure/results/v0.4.0-wave-1-size.json | wc -l | tr -d ' ')" = 0`
- `figures` read — AC2, AC3
  how: the result file's bytes match the run log; deltas and the three thresholds are stated.

## Out of scope
- Any size reduction or cut (the orchestrator decides with the user; spec cut order :445).
- Changing `cli-budgets.json` (172).
