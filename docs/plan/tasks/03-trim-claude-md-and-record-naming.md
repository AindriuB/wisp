# 03 — Trim CLAUDE.md under its cap and record the crate naming decision

**Repo:** `.`
**Depends on:** 01
**Executor:** `scribe` (only scribe writes docs and `CLAUDE.md`).
**Owns:**
- CLAUDE.md
- docs/workflow.md
- docs/conventions.md (the `## Naming` section only)
- docs/architecture.md (the one paragraph at lines 37-38 about the open naming decision only)

## Goal
`CLAUDE.md` is 54 lines against a hard cap of 50; bring it under the cap by
moving detail into `docs/workflow.md` rather than deleting it. Record the crate
naming decision settled in task 02 so the "open decision" text stops sending
readers to a question that is closed.

## Context
- CLAUDE.md:3-4 — the cap and the rule that detail belongs in `docs/`
- CLAUDE.md (`## Roles` section) — the longest block; `docs/workflow.md` already describes each phase's role
- docs/conventions.md:39-48 — the open decision to replace
- docs/architecture.md:37-38 — the matching pointer to the open decision
- docs/plan/tasks/02-scaffold-cargo-workspace.md — the decision itself (packages `hc-core`/`hc_core`, `hc-storage`/`hc_storage`, `hc-app`; binary `httpclient`)

## Acceptance
- [ ] `wc -l < CLAUDE.md` prints a number no greater than `50`.
- [ ] The `HARD CAP: 50 lines` comment is still present in `CLAUDE.md`.
- [ ] Every file path in the "Read on demand" table before the change is still present in `CLAUDE.md`.
- [ ] Each of the eight role names (`explorer`, `architect`, `planner`, `implementer`, `code-reviewer`, `tester`, `reviewer`, `scribe`) still appears in `CLAUDE.md`; any one-line role description removed from `CLAUDE.md` appears in `docs/workflow.md`.
- [ ] The six numbered "Rules that hold everywhere" are unchanged in meaning (diff shows wording kept or only shortened).
- [ ] `docs/conventions.md#naming` no longer contains "Open decision"; it states the three package names, the two lib names and the binary name `httpclient`, dated 2026-10-29.
- [ ] `docs/architecture.md` no longer says the naming is an open decision; it names the packages and points to `conventions.md#naming`. No other line of `architecture.md` changes (`git diff --stat` shows only that hunk).
- [ ] The scribe commits its change before reporting (`git status --porcelain` clean in its tree).

## Out of scope
- The `## Budgets` section of `docs/architecture.md` (task 08).
- `PLAN.md` and `HISTORY*.md` updates — those happen at `/record`, not here.
- Rewriting `docs/workflow.md` beyond receiving moved text.
