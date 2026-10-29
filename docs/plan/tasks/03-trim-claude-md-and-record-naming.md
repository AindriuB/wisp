# 03 — Trim CLAUDE.md under its cap

**Repo:** `.`
**Depends on:** 01
**Executor:** `scribe` (only scribe writes docs and `CLAUDE.md`).
**Owns:**
- CLAUDE.md
- docs/workflow.md

## Goal
`CLAUDE.md` is 54 lines against a hard cap of 50; bring it under the cap by
moving detail into `docs/workflow.md` rather than deleting it. Keep the product
name Wisp in its title line.

## Context
- CLAUDE.md:3-4 — the cap and the rule that detail belongs in `docs/`
- CLAUDE.md (`## Roles` section) — the longest block; `docs/workflow.md` already describes each phase's role

## Acceptance
- [ ] `wc -l < CLAUDE.md` prints a number no greater than `50`.
- [ ] The `HARD CAP: 50 lines` comment is still present in `CLAUDE.md`.
- [ ] Every file path in the "Read on demand" table before the change is still present in `CLAUDE.md`.
- [ ] Each of the eight role names (`explorer`, `architect`, `planner`, `implementer`, `code-reviewer`, `tester`, `reviewer`, `scribe`) still appears in `CLAUDE.md`; any one-line role description removed from `CLAUDE.md` appears in `docs/workflow.md`.
- [ ] The six numbered "Rules that hold everywhere" are unchanged in meaning (diff shows wording kept or only shortened).
- [ ] The scribe commits its change before reporting (`git status --porcelain` clean in its tree).

## Out of scope
- The `## Budgets` section of `docs/architecture.md` (task 08).
- `PLAN.md` and `HISTORY*.md` updates — those happen at `/record`, not here.
- Rewriting `docs/workflow.md` beyond receiving moved text.
