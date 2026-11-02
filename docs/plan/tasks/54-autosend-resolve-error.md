# 54 — Report the real resolve problem in the autosend failure line

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/workspace.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-54`.

## Goal
M3 (v1 hardening). Closes the PLAN.md P3 under "Workspace shell (28)": a resolve failure in the `WISP_AUTOSEND_URL` hook is printed as `WISP_EVENT failed kind=InvalidUrl`. That mislabels, for example, an inherited `{{token}}` that cannot be resolved. Print what actually failed, and keep the line prefix the measurement scripts depend on.

## Context
- crates/app/src/workspace.rs:126-137 — the autosend block; `send` returns `Result<RequestId, ResolveError>` (:428-443).
- crates/core/src/resolve/mod.rs:14-40 — `Location`, `Problem` (UnresolvedVar, MissingPathParam, Syntax, VarCycle) and `ResolveError { problems }`.
- crates/app/src/workspace.rs:530-595 — the terminal-event autosend line, which must stay as it is.
- scripts/measure/measure.sh:119-133 and measure.ps1 — they match `^WISP_EVENT completed` and `^WISP_EVENT failed`, and nothing after the prefix.
- docs/conventions.md#logging — never print values. Variable and parameter names are fine; `Syntax.message` and values are not.

## Acceptance
- [ ] On a resolve failure, the hook prints one stdout line of the form `WISP_EVENT failed kind=Resolve problems=<list>`. `<list>` is comma-separated entries such as `UnresolvedVar(token)`, `MissingPathParam(id)`, `Syntax(Url)`, `VarCycle(a)`, built by a pure function with a unit test covering all four variants.
- [ ] The printed line contains no variable value and no `Syntax` message text (asserted in the unit test with a `Syntax` whose message contains a marker string).
- [ ] The comment at workspace.rs:126 no longer claims the failure is `InvalidUrl`.
- [ ] `git diff main -- scripts/` is empty, and `RENDERER=hardware scripts/measure/measure.sh target/wt-54/release/wisp <scratch>/m.json` still reports status "measured" on macOS arm64.
- [ ] Manual: `WISP_AUTOSEND_URL='http://127.0.0.1:9/{{nope}}' target/wt-54/release/wisp` prints `WISP_EVENT failed kind=Resolve problems=UnresolvedVar(nope)` (quote the actual line in the report).
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Release binary size is reported with its delta from 19,114,864 B (d6a8bbb).

## Out of scope
- Whether a scratch tab should inherit collection auth at all (it does, by design).
- Engine-side failure lines and `error_words`.
- The UI's display of resolve problems.
