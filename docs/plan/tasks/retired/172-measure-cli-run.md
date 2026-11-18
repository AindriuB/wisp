# 172 — Measure wisp-cli with run linked and apply the budget decision (2M)

**Repo:** .
**Round:** 0
**Depends on:** 166
**Owns:**
- scripts/measure/results/v0.4.0-wave-2-cli-size.json (new)
- scripts/measure/cli-budgets.json (every limit to 7,340,032 B; see Amendment)

## Goal
Get the first real `wisp-cli` size with the network stack linked, right after
`run` merges, and apply the user's pre-approved budget decision before
`snapshot` (173) adds more.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 2" size estimate (:282-284), user decision (:19), "Size gate" (:445), Waves 2M (:473), Risk 1 (:494).
- Baseline: 165's result file (`scripts/measure/results/v0.4.0-wave-1-size.json`); 0.3.0 CLI macOS 1,415,600 / Windows 1,631,232 / Linux 1,966,744 B.
- `scripts/measure/cli-budgets.json` (4,194,304 B per target) is read by both `measure.yml:98` and `release.yml:71`, so changing it is the whole budget change; `release.yml` needs no edit.
- `measure.yml` writes the sizes before its "Check CLI size limit" step, so an over-limit run still yields the numbers in its log.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 172 measure-cli-run`; no local build. Foreground commands only; no polling loop; never `pkill` or `killall`. Wait with one foreground `gh run watch <id> --exit-status`.

## Acceptance
- [ ] AC1 A `measure.yml` dispatch on main at the head where 166 merged runs on every target; run id and head commit are recorded with per-target GUI and CLI bytes and deltas against the 165 baseline.
- [ ] AC2 Decision rule (user decision 2026-11-18, superseding 2026-11-17's 6 MiB): every target's limit in `cli-budgets.json` becomes 7,340,032 B (7 MiB). The result file records this user decision and the three measured `wisp-cli` sizes it rests on (run 44293903312, main 910d681: macOS 3,485,408 B, Windows 4,201,472 B, Linux 6,336,504 B).
- [ ] AC3 A second `measure.yml` dispatch on this branch, with the 7 MiB limits, completes green on every target; its run id is recorded. If any target is over 7,340,032 B: stop and report, change nothing.
- [ ] AC4 The Windows GUI is stated against 16,000,000 B and 15,900,000 B; over the soft line the PR says so first and cuts nothing.
- [ ] AC5 No file other than the two owned files changes.

## Checks
- `measure-run` command — AC1, AC3
  run: `gh run view <final-run-id> --json conclusion -q .conclusion | grep -x success`
- `budget-values` command — AC2
  run: `jq -e '[.limits[]] | all(. == 4194304 or . == 7340032) and (unique | length == 1)' scripts/measure/cli-budgets.json`
- `only-owned` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -vx -e scripts/measure/results/v0.4.0-wave-2-cli-size.json -e scripts/measure/cli-budgets.json | wc -l | tr -d ' ')" = 0`
- `figures` read — AC1, AC2, AC4
  how: the bytes match the run logs and every limit is 7,340,032 B as the 2026-11-18 decision says.

## Out of scope
- Size reduction of `wisp-cli`; limits other than the user's 7 MiB; `docs/architecture.md` limits (176).

## Wave stopped — step 1 — AC3: Linux wisp-cli is over 6,291,456 B
measure.yml run 44293903312 on main 910d681: wisp-cli macOS 3,485,408 B, Windows 4,201,472 B, Linux 6,336,504 B. Linux is 45,048 B over the pre-approved 6 MiB limit, so by AC3 nothing was changed and no result file was written. Needs a user decision on the Linux CLI budget.

## Wave resumed — user: raise every target limit to 7 MiB (7,340,032 B); planner amends AC2/AC3 and the check, then re-fanout

## Amendment — 2026-11-18 (user decision: 7 MiB for every target)
Run 44293903312 on main 910d681 measured `wisp-cli` at macOS 3,485,408 B,
Windows 4,201,472 B, Linux 6,336,504 B; Linux is over the pre-approved
6,291,456 B. The user decided every target's limit becomes 7,340,032 B (7 MiB).
Changed in place: AC2 (the new rule, and the result file records the decision
and the three sizes), AC3 (stop threshold 7,340,032 B), the `budget-values`
check (`. == 7340032`), the `figures` read and Out of scope. AC1 may be met by
reusing run 44293903312 (main 910d681) as the main-head measurement; no
re-dispatch on main is needed. The 3,600,000 B trigger no longer applies.

## Wave verified — f193f8456c5eb23d9952e86e9e47322c31268d7d

## Outcome
Merged at f193f84 (record/172-measure-cli-run). Every `wisp-cli` limit in `scripts/measure/cli-budgets.json` is now 7,340,032 B (user decision 2026-11-18). Main run 44293903312 (910d681) measured `wisp-cli` at macOS 3,485,408 B, Windows 4,201,472 B, Linux 6,336,504 B; branch run 44306383390 was green on every target at the new limits. The Windows GUI is 15,609,344 B, 290,656 B under the 15,900,000 B soft line. Figures: `scripts/measure/results/v0.4.0-wave-2-cli-size.json`.
