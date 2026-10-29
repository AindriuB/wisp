# 01 — Create the initial commit baseline on main

**Repo:** `.`
**Depends on:** none
**Executor:** `scribe` (or the orchestrator), directly on `main`. This is the one
sanctioned commit made without a worktree: `wt-new.sh` branches from `main`'s
HEAD, and `main` has no commits yet, so no worktree can exist before this lands.
**Owns:**
- .gitignore (commit as-is, no content change)
- CLAUDE.md (commit as-is, no content change)
- docs/** (commit as-is, no content change)

## Goal
Give the repository its first commit so every later task can branch a worktree
from `main`. The commit records the docs, the spec and these task files exactly
as they stand; nothing is edited.

## Context
- .claude/scripts/wt-new.sh:22-26 — worktrees are created from `main`'s HEAD
- .gitignore — already excludes `/.claude/` and `/.worktrees/`
- docs/conventions.md#commits — message format

## Acceptance
- [ ] `git -C /Users/Andrew/HttpClient rev-list --count main` prints `1`.
- [ ] The commit message is `01: initial docs baseline`.
- [ ] `git status --porcelain` is empty after the commit.
- [ ] `git ls-files` lists `CLAUDE.md`, `.gitignore`, `docs/plan/specs/2026-10-29-http-client-v1.md` and every `docs/plan/tasks/0[1-8]-*.md`.
- [ ] `git ls-files .claude .worktrees` prints nothing.
- [ ] Files are committed byte-for-byte: `git show HEAD:CLAUDE.md | wc -l` prints `54`, and `git diff HEAD` is empty.

## Out of scope
- Trimming `CLAUDE.md` (task 03).
- Any `Cargo.toml`, `crates/` or `.gitignore` additions (task 02).
- Adding a git remote or pushing. There is no remote today; nothing in M0 requires one to land.
