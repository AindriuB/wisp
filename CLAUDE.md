# HttpClient

<!-- HARD CAP: 50 lines. This file is a router, not a manual.
     If you are adding detail here, it belongs in docs/ instead. -->

A single Rust Cargo workspace for a portable, lightweight HTTP client with a
GPUI-based UI (crates `core`, `storage`, `app`; only `app` touches GPUI). The
design and milestones are in `docs/plan/specs/2026-10-29-http-client-v1.md`.

## Read on demand, not up front

| Need | File |
|---|---|
| How we work (the loop, roles, parallelism) | `docs/workflow.md` |
| Code style, naming, commit format | `docs/conventions.md` |
| What is open, in priority order | `docs/plan/PLAN.md` |
| What was already built — scan, never open `HISTORY.md` whole | `docs/plan/HISTORY-INDEX.md` |
| One task's full contract | `docs/plan/tasks/<id>.md` |
| System shape, service boundaries, ports | `docs/architecture.md` |
| Where the repositories live and how to obtain them | `docs/architecture.md#repository-topology` |

Load exactly one of these when the task needs it. Do not preload the set.

## Roles

Delegate rather than doing it inline. Each agent returns a conclusion, not a
transcript. Definitions: `~/.claude/agents/` (shared by every project).

- `explorer` — read-only recon: where a thing is, what calls it. Cites lines.
- `architect` — read-only judgement on shape, boundaries and blast radius.
- `planner` — turns a goal into numbered, parallel-safe task files with
  disjoint file ownership and explicit acceptance criteria.
- `implementer` — executes exactly one task file in its own worktree.
- `code-reviewer` — hunts bugs and style breaks in a diff, each with a concrete
  failing input; its findings go back to the implementer before verify.
- `tester` — runs builds and suites, reports pass/fail and the minimal failing
  detail. Never the full log.
- `reviewer` — checks a diff against its task's acceptance criteria.
- `scribe` — the only agent that edits `PLAN.md`, `HISTORY*.md` and `docs/`.

## The loop

`/plan <goal>` → `/fanout` → `/review` → `/verify` → `/record`. See `docs/workflow.md`.
Out of band, any time: `/recon <question>`, `/design <question>`.

## Rules that hold everywhere

1. One task = one worktree = one branch. Never two agents in one tree.
2. A task file names the files it owns. Editing outside that set is a bug —
   stop and report instead.
3. Never paste file contents into a summary. Cite `path:line`.
4. Prefer `rg` over `grep`, and read ranges over whole files.
5. Secrets, tokens and dumps never leave the machine and never enter a doc.
6. Only `scribe` writes docs. Only `implementer` writes code.
