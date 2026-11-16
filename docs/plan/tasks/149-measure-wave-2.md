# 149 — Measure wave 2 against the size gate and prepare the macOS hand test

**Repo:** .
**Depends on:** 139, 140, 141, 142, 143, 144, 145, 147, 148
**Owns:**
- scripts/measure/results/wave-2-size.json (new)

## Goal
Measure the Windows GUI exe on the main head after wave 2 merges, compare it
with the 15,500,000 B gate and the 15,400,000 B soft line, record `wisp-cli`
sizes, and hand the user a macOS hand-test checklist for the wave's UI.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Size gate" (soft line, cut order) and Amendment 1 (A5 first cut; mac-first testing)
- Baseline: Windows GUI 15,179,264 B at db79d76 (measure run 43704237455, task 138); headroom 320,736 B; soft-line room 220,736 B
- scripts/measure/results/wave-1-size.json — format to follow; .github/workflows/measure.yml (dispatch; GUI and CLI checks from 136)
- Worktree: `./.claude/scripts/wt-new.sh . 149 measure-wave-2`; no build is needed locally. Run every command in the foreground; leave nothing running; poll the dispatched run within the same turn.

## Acceptance
- [ ] AC1 A `measure.yml` dispatch on main at the wave-2 head (all listed tasks merged) completes green on every matrix target; its run id and head commit are recorded.
- [ ] AC2 `wave-2-size.json` records per target: GUI bytes, CLI bytes, baseline commit db79d76 and run 43704237455, and the GUI delta; it states the Windows GUI against the gate and the soft line.
- [ ] AC3 If the Windows GUI is over 15,400,000 B, the PR says so first and lists the cut order, without cutting anything: (1) 148's sidebar buttons, then the rest of 148 down to its palette commands; (2) the `suggest_confirm` hint (wave 3); (3) the Checks pane's expected/actual detail (141); (4) Explain's shadowed-layer list (140). Over 15,500,000 B: stop and report.
- [ ] AC4 The PR body carries a macOS hand-test checklist: three-button prompts for tab close, collection close and quit (Save first and default; a conflict keeps the tab open with the banner); an outside edit before the watcher fires is not overwritten; Explain on a real collection (secret shown by name only, dynamic marked, from-history scope); assertions edited, run and recorded (chip in history, "assertions are not resent" on Resend); New collection and New environment; `wisp-cli validate` and `explain` on the same collection.
- [ ] AC5 No file other than the result file changes.

## Checks
- `measure-run` command — AC1
  run: `gh run view <run-id> --json conclusion -q .conclusion | grep -x success`
- `only-result` command — AC5
  run: `git diff --name-only main...HEAD | grep -vx scripts/measure/results/wave-2-size.json | wc -l | grep -x 0`
- `figures` read — AC2, AC3, AC4
  how: the result file's Windows bytes match the run log; delta, gate and soft-line status are stated; the checklist covers every AC4 item.

## Out of scope
- Any size reduction or cut (the orchestrator decides with the user).
- Windows and Ubuntu hand tests (mac-first, Amendment 1).
- `docs/architecture.md#budgets` (scribe).
