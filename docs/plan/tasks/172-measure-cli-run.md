# 172 — Measure wisp-cli with run linked and apply the budget decision (2M)

**Repo:** .
**Round:** 0
**Depends on:** 166
**Owns:**
- scripts/measure/results/v0.4.0-wave-2-cli-size.json (new)
- scripts/measure/cli-budgets.json (only if a target measures over 3,600,000 B)

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
- [ ] AC2 Decision rule (pre-approved, user decision 2026-11-17): if any target's CLI is over 3,600,000 B, every target's limit in `cli-budgets.json` becomes 6,291,456 B; otherwise the file is unchanged. The result file states which applied and why.
- [ ] AC3 If the file changed, a second `measure.yml` dispatch on this branch completes green on every target; its run id is recorded. If any target is over 6,291,456 B: stop and report, change nothing.
- [ ] AC4 The Windows GUI is stated against 16,000,000 B and 15,900,000 B; over the soft line the PR says so first and cuts nothing.
- [ ] AC5 No file other than the two owned files changes.

## Checks
- `measure-run` command — AC1, AC3
  run: `gh run view <final-run-id> --json conclusion -q .conclusion | grep -x success`
- `budget-values` command — AC2
  run: `jq -e '[.limits[]] | all(. == 4194304 or . == 6291456) and (unique | length == 1)' scripts/measure/cli-budgets.json`
- `only-owned` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -vx -e scripts/measure/results/v0.4.0-wave-2-cli-size.json -e scripts/measure/cli-budgets.json | wc -l | tr -d ' ')" = 0`
- `figures` read — AC1, AC2, AC4
  how: the bytes match the run logs and the decision follows the 3,600,000 B rule.

## Out of scope
- Size reduction of `wisp-cli`; limits other than the pre-approved 6 MiB; `docs/architecture.md` limits (176).
