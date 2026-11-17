# 153 — Prune PLAN.md and record the wave-3 user decisions (3C, part 1)

**Repo:** .
**Depends on:** none
**Executor:** `scribe` (docs only; no implementer, no code)
**Owns:**
- docs/plan/PLAN.md
- docs/plan/archive/2026-11-17-closed-sections.md (new)
- docs/plan/specs/2026-11-14-wisp-0-3-0.md (append `## Amendment 2` at the end only)
- docs/plan/roadmap.md (size-gate lines only: :7, :20-22, :142, :144, :159, :203)
- docs/plan/tasks/99-hardening-0-2-0.md (AC39's gate figure only)

## Goal
PLAN.md is about 100 KB and every session that opens it pays for that. Archive
its closed sections so it holds only open work, and record the two user
decisions of 2026-11-17 where the spec and roadmap state the old rule. This
must merge before any other wave-3 task is recorded, because every record
edits PLAN.md.

## Context
- User decisions, 2026-11-17 (binding):
  1. Cut task 148's sidebar empty-state buttons (the first cut under Amendment 1 / roadmap D-A); keep the palette "New collection…" and "New environment…" commands. Implemented by task 154.
  2. Raise the Windows GUI hard gate from 15,500,000 B to 16,000,000 B and the soft line from 15,400,000 B to 15,900,000 B. The enforced check (`.github/workflows/release.yml:130-131`) changes in task 156. `scripts/measure/budgets.json` keeps its 16 MiB figure.
- Why: wave 2 measured the Windows GUI at 15,494,656 B, 5,344 B under the old gate (`scripts/measure/results/wave-2-size.json`; HISTORY "## 2026-11-16 — Measure wave 2 (task 149)").
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Size gate" (:313-315) and "Amendment 1" (:379-395) — the text the amendment overrides; do not edit the body.
- docs/plan/roadmap.md:7, :20-22, :142, :144, :159, :203 — the old gate and the "raise only at 0.4.0" rule (D-1), which decision 2 supersedes.
- Precedent archives: docs/plan/archive/2026-11-16-closed-sections.md (format and header to follow).
- PLAN.md "Now" → "v0.3.0 (in progress)" :21-29 — the 149 bullet's "decision not yet made" is now made; wave 3 is 153-158.
- Worktree: `./.claude/scripts/wt-new.sh . 153 plan-prune-and-gate-amendment`. Run every command in the foreground; never start a background command; never use `pkill` or `killall`.

## Acceptance
- [ ] AC1 Every section or item in PLAN.md that is closed (done, superseded, or only a pointer to an archive or HISTORY) is moved verbatim to `docs/plan/archive/2026-11-17-closed-sections.md`; open follow-ups stay in PLAN.md. Nothing is deleted: each line removed from PLAN.md appears in the archive file, apart from lines the scribe rewrites under AC2.
- [ ] AC2 PLAN.md's "v0.3.0 (in progress)" section states both 2026-11-17 decisions in one line each, replaces "Decision with the user, not yet made" with the decision, and lists wave 3 as in flight: 153-158.
- [ ] AC3 The PR body states PLAN.md's size before and after, in bytes. PLAN.md is smaller than 99,898 B (its size at 55d55ba).
- [ ] AC4 The 0.3.0 spec gains `## Amendment 2 (2026-11-17, user decisions)` at its end, recording: the gate is 16,000,000 B and the soft line 15,900,000 B for the Windows GUI exe; the cut of 148's sidebar buttons (palette actions kept); the remaining cut order beyond that is unchanged. No line above the new heading changes.
- [ ] AC5 roadmap.md's gate figures and decision D-1 match Amendment 2 (16,000,000 B gate, 15,900,000 B soft line, raised in 0.3.0 by user decision 2026-11-17); the 0.4.0 "raise only if headroom is under 100 KB" condition is marked superseded.
- [ ] AC6 `99-hardening-0-2-0.md` AC39 names the current gate (16,000,000 B) with a pointer to Amendment 2; no other line of that file changes.
- [ ] AC7 No file outside `Owns` changes; HISTORY.md, HISTORY-INDEX.md, retired task files, earlier archive files and `scripts/measure/results/*.json` are left as historical record.

## Checks
- `scope` command — AC7
  run: `git diff --name-only main...HEAD | grep -vxE 'docs/plan/PLAN\.md|docs/plan/archive/2026-11-17-closed-sections\.md|docs/plan/specs/2026-11-14-wisp-0-3-0\.md|docs/plan/roadmap\.md|docs/plan/tasks/99-hardening-0-2-0\.md' | wc -l | grep -x 0`
- `spec-append-only` command — AC4
  run: `git diff main...HEAD -- docs/plan/specs/2026-11-14-wisp-0-3-0.md | grep -E '^-[^-]' | wc -l | grep -x 0`
- `smaller` command — AC3
  run: `test "$(wc -c < docs/plan/PLAN.md)" -lt 99898`
- `nothing-lost` command — AC1
  run: `git diff -U0 main...HEAD -- docs/plan/PLAN.md | grep -E '^-[^-]' | sed 's/^-//' | grep -v '^$' | while IFS= read -r l; do grep -qF -- "$l" docs/plan/archive/2026-11-17-closed-sections.md || echo "MISSING: $l"; done | tee /dev/stderr | wc -l` — every MISSING line must be one AC2 rewrote; the reviewer checks each.
- `decisions` read — AC2, AC4, AC5, AC6
  how: the reviewer reads the amendment, the roadmap lines and the PLAN v0.3.0 section against the two decisions in Context.

## Out of scope
- docs/architecture.md, docs/cli.md and README.md (task 157).
- Changing `release.yml` (task 156) or `scripts/measure/budgets.json` (unchanged).
- Re-triaging open follow-ups or deleting open items; this is an archive move, not a re-plan.

## Wave verified — a2e443826783a826d5eb7ba8c15452cd387a563d

## Outcome
Merged via record/153 at a2e4438. PLAN.md 99,898 B to 94,230 B; closed sections archived to docs/plan/archive/2026-11-17-closed-sections.md; spec Amendment 2 and roadmap/task 99 gate figures updated. Roadmap :23 (D-1 superseded) accepted as in scope.
