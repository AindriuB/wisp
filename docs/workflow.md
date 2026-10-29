# The workflow

A five-phase loop designed so that the expensive phase — implementation — runs
wide and in parallel, and the main session's context stays small.

```
/plan <goal>     planner   →  docs/plan/tasks/NN-slug.md  (one file per unit)
/fanout          implementer × N, one git worktree each
/review          code-reviewer per branch; fixes loop back to the implementer
/verify          tester + reviewer, one pair per reviewed task
/record          scribe    →  PLAN.md, HISTORY.md, docs/
```

## Roles

Each agent returns a conclusion, not a transcript. Definitions live in
`~/.claude/agents/` (shared by every project).

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

## Why it is shaped this way

The main session is the scarcest context in the run. Everything that produces
bulk output — directory walks, file reads, build logs, diffs — happens inside a
subagent, whose context is discarded when it returns. The main session only ever
sees conclusions: a list of task ids, a pass/fail, a set of review verdicts.

That is the whole token strategy. Two corollaries:

- **Never read a file in the main session that an agent could read for you.**
  If you are about to open something to answer a question, send `explorer` —
  or `architect`, if the question is about shape rather than location.
- **Never let an agent return raw content.** Every role below has a return
  contract that caps what comes back. Enforce it.
- **Never load `HISTORY.md` whole.** It is append-only and unbounded, so it
  becomes the biggest file in the workspace. `HISTORY-INDEX.md` holds one row
  per entry; scan that, then grep `HISTORY.md` for the exact heading the row
  names and read that section alone.

## Phase 1 — plan

`planner` reads the goal and the current `PLAN.md`, then writes one file per
unit of work into `docs/plan/tasks/`. It returns only the task ids and their
dependency edges.

A task file is parallel-safe when it declares:

- **Owns** — the exact file globs this task may write. Disjoint from every
  sibling task. This is what makes concurrency safe.
- **Depends on** — task ids that must land first, or `none`.
- **Acceptance** — checkable statements. Not "works well"; "GET /accounts
  returns 200 with the seeded fixture".
- **Context** — the two or three files worth reading, with line ranges.

Tasks with `depends on: none` form the first wave. Everything else waits.

## Phase 2 — fanout

For each task in the current wave, in a single message, spawn one `implementer`
with the task id. Each one:

1. runs `.claude/scripts/wt-new.sh <repo> <task-id>` to get its own worktree
   and branch,
2. reads only its task file and the files that task names,
3. implements, commits on its branch,
4. returns the branch name, the files touched and a one-line summary.

Concurrency is bounded by the worktree scripts, not by discipline: two agents
physically cannot be in the same checkout.

## Phase 3 — review

For each returned branch, spawn one `code-reviewer`. It reads the diff from git
objects and returns findings, each with the input that triggers it:

- **bugs** (P1, P2, P3): security holes, wrong results, races, unbounded growth,
  and tests that would pass with their fix removed;
- **style**: breaks of `docs/conventions.md`, the lint config, or the
  surrounding code's own idiom — never taste.

Each finding is routed. **In-spec** findings — the task file already says what
the code should do — are appended to the task file under
`## Review round N — findings`, the same task's `implementer` fixes only those
in its existing worktree, and `code-reviewer` re-reviews just the fix commits,
flagging any drift in them. Up to three rounds; after that the user decides.

**Spec-changing** findings — the fix would change specified behaviour, add
scope, need a file outside `Owns`, or reopen a decision — stop the loop and
come to the orchestrator. The user decides, the `planner` records the amendment
in the task file, and only then does anything go back to the implementer. A
fix loop must never be where the spec changes.

Only a `CLEAN` branch moves on.

This is a different question from phase 4's. `reviewer` asks "did it do what
the task said?"; `code-reviewer` asks "what is wrong with what it did?" Asking
the second one before the PR is what keeps outside review from finding it
after.

## Phase 4 — verify

For each returned branch, spawn `tester` and `reviewer` together. `tester` runs
the build and suite in that worktree and returns `PASS`/`FAIL` plus, on failure,
the first failing test name and its assertion — never the log. `reviewer` reads
the diff against the task's acceptance criteria and returns a verdict per
criterion.

A failing task goes back to phase 2 with the failure appended to its task file,
and through phase 3 again before it returns here.
It does not get "fixed inline" — that is how the main context blows up.

## Phase 5 — record

`scribe` merges the passing branches, removes their worktrees, moves the task
files out of `tasks/`, and updates `PLAN.md` and `HISTORY.md`. It is the only
role that touches those files, which is why they never end up with conflicting
concurrent edits.

Each `HISTORY.md` entry gets its row in `HISTORY-INDEX.md` in the same commit.
That pairing is what keeps the index trustworthy enough for `planner` to rely
on instead of the file itself.

## The skills, alongside the phases

The phases say who runs. Five skills say how a kind of work is done, and load
only when it comes up:

- Before `/plan`, when the goal is still a sentence — `shaping-work`. It
  classifies the work, gets the shape agreed, and hands the agreed design into
  the `/plan` invocation, because the planner reads the goal and not the
  conversation above it.
- Inside `/fanout` — `test-driven-development`. The acceptance criteria are the
  test list. On a second attempt, `systematic-debugging` first, since the last
  attempt already tried the obvious thing and wrote it down.
- At `/verify` — `verification-before-completion`. A main session that never
  reads the code cannot verify by looking, so it checks the branch instead.
- Inside `/record` — `documenting-a-phase`, when the wave changed what a user
  can do or what a developer needs to know. Most waves did not.

## Out of band

Two roles sit outside the loop and can be sent at any point in it:

- `/recon <question>` → `explorer`. "Where is X", "what calls Y", "does this
  already exist". Returns `path:line` citations.
- `/design <question>` → `architect`. "One service or two", "what is the blast
  radius of changing this contract". Returns one recommendation with the
  evidence under it, and labels anything it did not verify as `GUESS`.

Both are read-only, and both exist so the searching happens in a context that
gets discarded. Sending one mid-phase is normal — a planner that needs to know
what already exists should have asked before splitting the work.

## When not to use the loop

A one-file change, a typo, a question. The loop costs a planner round-trip and
a worktree; below roughly three files of work it is not worth it. Do it inline
and tell `scribe` afterwards if it changed anything a future session needs.
