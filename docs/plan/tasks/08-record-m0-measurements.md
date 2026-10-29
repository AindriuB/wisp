# 08 — Record the M0 measurements in architecture.md

**Repo:** `.`
**Depends on:** 03, 07
**Executor:** `scribe`.
**Owns:**
- docs/architecture.md (the `## Budgets` section only)

## Goal
Replace the "TBD" Budgets section with the measured M0 figures per platform, so
the GPUI exit decision and the budgets rest on recorded numbers. Measured values
are recorded as measured; the budgets set from them are the user's decision.

## Context
- docs/architecture.md:149-155 — the section to replace
- scripts/measure/results/*.json — the records from task 07
- docs/plan/specs/2026-10-29-http-client-v1.md:209-222 — M0 exit

## Acceptance
- [ ] `## Budgets` contains a table with one row per v1 target (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`) and columns binary size, idle RSS, RSS after one request, renderer, source commit.
- [ ] Every number in the table is copied from a committed `scripts/measure/results/*.json` file or a named `measure.yml` run; any platform without one reads `not measured — <reason>`.
- [ ] Software-rendered CI figures are marked as not comparable with the hardware figure.
- [ ] Budget values appear only if the user has stated them; otherwise the section says `Budgets: pending user decision on these measurements` and keeps the rule that no change may cite a number not in this section.
- [ ] `git diff` on `docs/architecture.md` touches only the `## Budgets` section.
- [ ] The scribe commits before reporting.

## Out of scope
- The GPUI keep-or-revisit decision itself; it is the user's, made on these numbers.
- `PLAN.md` and `HISTORY*.md` — done at `/record`.
- Any other section of `docs/architecture.md`.
