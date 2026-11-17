# 158 — Measure wave 3 and dry-run the release (3M)

**Repo:** .
**Depends on:** 154, 155, 156
**Owns:**
- scripts/measure/results/wave-3-size.json (new)

## Goal
Prove the wave-3 head fits its budgets and that the release workflow produces
all three archives with `wisp-cli` inside, before anyone tags. Hand the user a
macOS hand-test checklist. Tagging, the version bump and publishing stay with
the user.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Waves" 3M (:349), Amendment 1 (mac-first testing; Tag rule) and Amendment 2 (task 153): Windows GUI gate 16,000,000 B, soft line 15,900,000 B.
- Baseline: wave 2, main at 4769107, run 43976706350: Windows GUI 15,494,656 B; CLI macOS 1,415,600 / Windows 1,631,232 / Linux 1,966,744 B (`scripts/measure/results/wave-2-size.json`, the format to follow).
- `measure.yml` (dispatch) records GUI and CLI sizes and checks `cli-budgets.json`; `release.yml` with `-f draft_release=true` builds the three archives, runs the 16,000,000 B and CLI size steps (task 156), and creates a `dryrun-<run id>` draft release.
- A draft release is outward-facing: dispatch `release.yml` only after the orchestrator confirms the user approved it. `measure.yml` needs no approval (as in 136 and 149).
- Worktree: `./.claude/scripts/wt-new.sh . 158 measure-and-release-dry-run`; no local build is needed. Run every command in the foreground; never start a background command or a polling loop; never use `pkill` or `killall`. Wait on a dispatched run with one foreground `gh run watch <id> --exit-status`.

## Acceptance
- [ ] AC1 A `measure.yml` dispatch on main at the wave-3 head (154, 155, 156 merged) completes green on every target; run id and head commit are recorded.
- [ ] AC2 `wave-3-size.json` records per target GUI and CLI bytes, the baseline (4769107, run 43976706350) and both deltas, and states the Windows GUI against 16,000,000 B and 15,900,000 B. Over the soft line: the PR says so first, cuts nothing. Over the gate: stop and report.
- [ ] AC3 After approval, a `release.yml` dispatch with `draft_release=true` on the same head completes green; its run id is recorded. Each archive in the draft lists `wisp-cli` at the AC5 path of task 156 (`unzip -l` / `tar -tzf` output quoted in the PR), and `SHA256SUMS` lists exactly three `Wisp-*` files.
- [ ] AC4 The PR states the Windows CLI's signing state from the run log and whether the draft's notes carry the unsigned-CLI sentence.
- [ ] AC5 The PR body carries a macOS hand-test checklist: guard on and off from the palette and the badge; Send and Cancel on a guarded environment; switching environment with the prompt open sends nothing; Resend from history is guarded; the `suggest_confirm` prompt appears once; the sidebar has no New buttons and the palette still creates a collection and an environment; the bundled `Wisp.app/Contents/MacOS/wisp-cli` runs `validate` and `explain` and passes `codesign --verify` and `spctl` with the app.
- [ ] AC6 No file other than the result file changes; the dry-run draft release is left for the user to delete.

## Checks
- `measure-run` command — AC1
  run: `gh run view <measure-run-id> --json conclusion -q .conclusion | grep -x success`
- `release-run` command — AC3
  run: `gh run view <release-run-id> --json conclusion -q .conclusion | grep -x success`
- `only-result` command — AC6
  run: `git diff --name-only main...HEAD | grep -vx scripts/measure/results/wave-3-size.json | wc -l | grep -x 0`
- `figures` read — AC2, AC3, AC4, AC5
  how: the result file's bytes match the run logs; gate and soft-line status are stated; archive listings and the checklist are in the PR.

## Out of scope
- The version bump to 0.3.0, the tag, publishing and deleting the dry-run draft (user and orchestrator).
- Windows and Ubuntu hand tests (mac-first, Amendment 1).
- Any size reduction or cut; investigating the CLI's growth since wave 1.
- `docs/architecture.md#budgets` (task 157).
