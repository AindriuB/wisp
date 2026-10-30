# Conventions

Fill this in per workspace. It is what `reviewer` checks against, so keep it to
rules that are actually checkable in a diff — a convention nobody can verify is
a preference, and it belongs in a conversation rather than in this file.

Sections marked *(kit)* below arrived with the agent kit. They are not project
guesses: each one was written after a specific failure cost a real workspace
attempts or lost work, and the failure is stated so the rule can be argued with
rather than obeyed blindly. Delete one only when you have decided it does not
apply here — not because it is long.

## Commits

`<task-id>: <imperative summary>` — for example `03: add IBAN checksum guard`.
Body only when the *why* is not obvious from the diff. No emoji.

*(kit)* `Co-Authored-By:` and `Claude-Session:` trailers are permitted on
agent-authored commits. The operating harness appends both automatically to
every commit; they are accurate provenance, not noise, and a rule the tooling
cannot follow costs a review round trip on every commit without improving the
history. A reviewer must not block a merge over their presence, and no existing
commit needs amending to strip them.

## Branches

`task/<id>-<slug>`, created by `.claude/scripts/wt-new.sh`. Never work directly
on the default branch.

## Toolchain

Stable Rust, pinned in `rust-toolchain.toml` (currently 1.99.1), edition 2024.
The repo is a Cargo workspace and shared dependency versions live in
`[workspace.dependencies]`; member crates reference them with `workspace = true`.

`cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
must both be clean before a task is reviewed.

## Naming

Crates are conceptually `core`, `storage` and `app`, in `crates/<name>/`.
Standard Rust naming otherwise: `snake_case` functions and modules,
`CamelCase` types, `SCREAMING_SNAKE_CASE` constants.

**Decided 2026-10-29:** the product is Wisp and the shipped binary is `wisp`.
A package named `core` would clash with std `core`, so the packages are
`wisp-core` (lib `wisp_core`), `wisp-storage` (lib `wisp_storage`) and
`wisp-app` (produces the binary `wisp`), in `crates/core`, `crates/storage` and
`crates/app`. Every workspace package sets `publish = false`: `wisp`,
`wisp-core` and `wisp-app` are already taken on crates.io by an unrelated tmux
tool, and Wisp ships as binaries from GitHub releases, not via crates.io.

## Errors

- `core` and `storage` return typed errors defined with `thiserror`.
- `anyhow` is allowed only in `crates/app`.
- No `unwrap` or `expect` outside tests, except where an adjacent comment proves
  the invariant that makes it infallible.

## Logging

Use `tracing`. Never log secrets, auth headers, or values from
`*.secrets.toml`; log header names, not values, for anything auth-related.
`Request`, `FolderDefaults`, `CollectionDefaults` (via `Auth`) and
`ClientSettings.proxy` derive Debug in plain text, so never log them with `?` or
`{:?}`. Only `Environment` has a redacting Debug.

## GPUI

GPUI and `gpui-component` appear only in `crates/app`. Both come from
crates.io (the latest `gpui-component` and the `gpui-pre` snapshot it depends
on), pinned to exact versions (`=x.y.z`) in `[workspace.dependencies]`, with one
gpui source in `Cargo.lock`. Bumping them is its own task, never folded into
feature work.

## Dependencies

Lightness is the product, so every new dependency is justified in its task or
PR for binary size and memory cost. Set `default-features = false` and enable
only the features used. TLS is `rustls` only; OpenSSL and native-tls are never
acceptable, including transitively through a feature of another crate.

## TOML

Writes go through `toml_edit` so user comments and formatting survive. Do not
serialise a whole file back with `toml` or `serde`.

## Tests

- Unit tests live in the module they test (`#[cfg(test)] mod tests`).
- `core` and `storage` are tested without a window or GPUI.
- HTTP tests run against a local mock server (for example `wiremock`), never the
  public internet.
- Storage tests use temp directories and clean up after themselves.
- Test names read as sentences; no sleeps to wait for state.

## Code comments *(kit)*

Short by default — a line or two. Length is earned, not assumed: only a
non-obvious trap, or a decision that would otherwise be re-litigated, buys more,
and even then keep it tight. Do not narrate what the code plainly does. Do not
restate the task file that produced the change. Never write a comment asserting
a result nobody observed — visual outcome, performance, behaviour — unless
someone actually looked.

The test for a long comment: would deleting it cost someone real time, or let
them reintroduce a bug? A comment recording that an awkward construct is a
deliberate workaround for a framework bug passes — without it, the next tidy-up
reintroduces the bug. A comment that runs tens of lines to say what one sentence
would does not.

One workspace shipped three "fixes" in a single milestone whose comments claimed
a visual result nobody had checked, and all three were wrong. A short comment has
less room to assert something unverified; that is not incidental to the brevity
rule, it is most of the reason for it.

## Documentation

Prose in complete sentences. Tables only for genuinely tabular content. Say what
changed and what it cost. Avoid "comprehensive", "robust", "seamlessly" — they
carry no information.

*(kit)* `scribe` commits what it writes before reporting back — `PLAN.md`,
`HISTORY.md` and its index, task-file retirements, any doc edit — rather than
leaving the working tree dirty for a later session to pick up or lose. This is
not optional cleanup: uncommitted scribe output has already gone missing, and a
retired task file once had to be reconstructed from a transcript. A `scribe`
turn that has not run `git commit` has not finished.

*(kit)* `docs/plan/tasks/retired/` is archive, not working set: exclude it from
default `rg` searches with an `.ignore` file, placed both at the repo root and
inside `docs/plan/` — some `rg` builds do not honour a parent-directory
`.ignore` when given a relative subdirectory path. Git still tracks every file
there and existing citations to a specific `retired/<file>.md` still resolve;
read one by explicit path when a citation points at it, and use `--no-ignore` to
search the archive itself.

## Pruning planning docs *(kit)*

Before deleting narrative from `PLAN.md` or any planning doc, verify coverage
mechanically: extract the set of task ids in the text you are about to delete,
compare it against the set surviving in `HISTORY.md` and `docs/plan/tasks/**`,
and state the residual explicitly. Do not delete on a header skim. One prune
claimed every deleted block had a `HISTORY.md` equivalent "verified by
cross-checking headers"; the claim was false and lost two close-out records,
recovered only by a later commit. The one-liner that does the comparison:

```
git show <commit>^:docs/plan/PLAN.md | grep -oE '\b[A-Z]?[0-9]{2,3}\b' | sort -u
```

against the same extraction over `HISTORY.md` and the task directories.

## Progress reporting *(kit)*

After each task closes — merged or otherwise resolved, not only at wave or
milestone boundaries — the orchestrator reports a markdown table of every task in
the current milestone or phase, including tasks not yet started. A table of only
completed work hides the point, which is to make remaining work visible at a
glance.

The table lists, at minimum, task id, a short description of what it owns, and
status. Status distinguishes at least complete/merged, in flight, and not
started; a task that is blocked or in review is reported as such rather than
collapsed into "in flight". The table reflects actual repository state — merged
branches, task files retired into `docs/plan/tasks/retired/` — not intent, and a
task is never shown as complete before it has merged and its gate has passed.

This is a reporting convention for the orchestrator's messages to the user. It
does not change what `PLAN.md` or `HISTORY.md` record: those remain `scribe`'s.

## Task-file baselines *(kit)*

A task file that states a test-count baseline ("467 existing tests still pass")
names the commit it was measured on. A `planner` re-measures the baseline when
the base branch has moved since that commit rather than copying a figure forward
from an earlier task file. Two task files in one wave once both cited a stale
commit; harmless that time because the arithmetic still reconciled, but it was
the third stale-figure incident in one project and worth catching before it
stops reconciling by luck.

## Task-file scoping *(kit)*

When a later wave will obviously consume a type this wave owns, the owning wave's
task file states every field the consumer will need — not only the fields its own
acceptance criteria exercise directly. Checkable in the task file itself: does its
`Owns`-list type carry the shape the next wave's task files already describe
wanting? Seen repeatedly — a state type missing `Equatable`, a row type missing
the two fields its view needed — each added under review rather than in the
original scope. Adding a field after the fact means a wave-2 task edits a file
wave 1 owns, exactly the contention disjoint file ownership exists to prevent. A
`planner` should ask, for every owned type, "what will the consumer already
planned in a later wave read off this" before freezing the wave-1 acceptance list.

## Acceptance-criteria discipline *(kit)*

Four rules, all evidenced by one task that took six attempts to repair a test
suite and whose cause was the task file each time, not the implementers.

**A criterion must be checkable by a measurement already taken at least once.**
That task's acceptance list opened with "the full suite passes", written when no
such run had ever completed; five attempts went on discovering that the blocker
lived in another task's file before a sixth could take the measurement the
criterion assumed. If nobody has watched the instrument produce a reading, the
first deliverable is a completing run, filed as its own task — not a criterion
riding on top of one that has never happened.

**A criterion may not name a file outside the task's `Owns`, except to require it
unchanged.** Three of that task's criteria pointed at files it did not own, each
costing a full attempt to discover. If closing a criterion needs an edit outside
`Owns`, the criterion belongs to the task that owns the file: widen `Owns` when
the plan is written, or put the criterion where the file already is.

**A task's criteria freeze when its first attempt starts.** New work found
mid-flight is filed as a successor task and named in the close-out, not folded
into the task already in flight. That task kept absorbing scope across six
attempts, so "done" moved every time an attempt got close to it, and a task whose
definition of done moves after each attempt cannot converge.

**Do not run two tasks that drive the same exclusive resource at once** — one
simulator, one device, one bound port. Attempt 4 measured a 0% flake rate with
exclusive access on a case three earlier attempts had called flaky; contention had
been resolving a build artefact from one run into a sibling worktree. Re-measure
any historical flake rate before trusting it rather than inheriting a number taken
under contention.

## Reviewer isolation *(kit)*

A reviewer reads from git objects only — `git diff`, `git show`, `git log` —
never the working tree, because another agent may be actively mutating it. A
reviewer and a tester were once run against the same worktree at the same time;
the tester's brief required temporarily mutating a source file to prove a guard
non-vacuous, the reviewer read the working tree mid-mutation, and reported a
phantom defect that existed at no commit. The reviewer had already been told to
use `git diff`/`git show` and checked the working tree anyway, so state both
halves: do not schedule a reviewer and a tester against the same worktree at the
same time when the tester's brief includes a mutation-based non-vacuity proof —
serialise them, or have the reviewer work from an explicit `git show` of the tip.
Mutation-based proofs are standard practice under the discipline above, so this
collision recurs unless both halves are stated.

## Background tasks in subagent turns *(kit)*

Subagents do not receive background-task completion notifications. An implementer
that starts a long build or test run in the background and ends its turn saying
"waiting for job X" is never woken — it stalls rather than resumes. Run long
commands in the foreground and block, or poll within the same turn; never yield a
turn expecting to be resumed later. This has also retrospectively explained
stalled runs previously written off as environment hangs.

## Verification environment *(kit)*

Before trusting any test failure against a service-backed suite, confirm the
service is actually accepting connections for *this* run — an explicit client
connect, not just a container reporting healthy. One wave lost significant time
to three environment faults that produced red results indistinguishable from code
failures until traced by hand. A container can restart healthy while never
re-binding a host port already held by something else, and a healthcheck does not
catch that.

## What never goes in a file

Credentials, tokens, keys, real personal data, and pasted log dumps.

## Logs *(kit)*

If an agent needs to write a log file — verification output, replay traces — it
goes in `logs/` at the repo root, never loose at the top level. `logs/` and
`*.log` are gitignored: logs are scratch, not an artifact to commit.
