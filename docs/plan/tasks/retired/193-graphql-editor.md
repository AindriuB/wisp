# 193 — Edit GraphQL bodies in the request pane

**Repo:** .
**Round:** 0
**Depends on:** 189
**Owns:**
- crates/app/src/ui/request/body.rs
- crates/app/src/ui/request/graphql.rs (new)
- crates/app/src/ui/request/mod.rs (`mod graphql;` only)
- crates/service/src/plan.rs (the GraphQL body arm at ~:259 only; added from 189's review)

## Goal
Replace task 189's read-only GraphQL placeholder with an editor: a query
editor and a variables JSON editor with a Format action. A GraphQL request
can then be written, saved and sent from the GUI with no hand-editing of
the file.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 3" and D-K: no introspection, schema or autocomplete.
- Task 189: `Body::GraphQl(GraphQlBody { query, variables })`, and the placeholder in `body.rs`.
- crates/app/src/ui/request/body.rs: `BodyKind`/`KINDS` :25-45; `Stash`/`body_for`, which keeps each kind's content across switches; the JSON editor and its `{{var}}` highlighting.
- crates/app/src/pretty.rs: JSON pretty-printing (`PRETTY_LIMIT`).
- Worktree: `./.claude/scripts/wt-new.sh . 193 graphql-editor`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-193`. Foreground commands only; never `pkill` or `killall`; stop only a pid you launched.

## Acceptance
- [ ] AC1 Choosing GraphQL in the body picker shows two labelled editors, Query and Variables. Switching to another kind and back keeps both texts, through `Stash`, as for other kinds.
- [ ] AC2 The Query and Variables editors are multi-line and monospace and are built like the JSON body editor (`EditorState::new`, body.rs:201-203). They add no `{{var}}` highlighting (deferred, Amendment 1).
- [ ] AC3 The Variables editor has a "Format" action. It pretty-prints the text when it is valid JSON. When the text is not blank and is not a JSON object, an inline note reads "Variables are not a JSON object". The note does not block Send; resolve reports the problem at send time.
- [ ] AC4 Edits mark the body section dirty. Save writes the task 189 file format. Reopening the file shows the same query and variables.
- [ ] AC5 Tests, prefix `graphql_editor_`:
  - pure: format of valid and invalid variables; the note decision for blank, an array and an object;
  - gpui: type a query and variables, save, reload, and the texts are equal; switch kinds and back, and the texts are kept.

  At least 5 tests.
- [ ] AC6 No file outside Owns changes. No new dependency. fmt, clippy and the workspace suite pass.

## Checks
- `graphql-editor-tests` command — AC1, AC3, AC4, AC5
  run: `cargo test -p wisp-app --locked graphql_editor_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([5-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC6
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/app/src/ui/request/body.rs$' -e '^crates/app/src/ui/request/graphql.rs$' -e '^crates/app/src/ui/request/mod.rs$' -e '^crates/service/src/plan.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `placeholder-gone` read — AC1, AC2
  how: the reviewer confirms that the task 189 placeholder text is removed and that the Query and Variables editors are built the same way as the JSON body editor, with no new highlighter or copy of `highlight::text_spans`.

## Carried from task 189's review
- `crates/service/src/plan.rs:259`: a draft GraphQL body from a non-GraphQL stored body must test `blank(&d.query)`, not require both fields non-blank. Add a test.

## Out of scope
- Converting an existing JSON body into GraphQL automatically.
- Schema, introspection, autocomplete, `operationName`, GET mode.
- The task 174 vars-editor test gaps (left in PLAN).

## Wave stopped — step 2 — orchestrator findings at 69e1c15: (1) `{{var}}` highlighting (spec :163) does not exist in any body editor, so AC2 "same as the JSON editor" is vacuous; building it needs a highlighter outside Owns (likely crates/app/src/highlight.rs), or AC2/spec are amended; (2) the `scope` check omits crates/service/src/plan.rs, which Owns includes.

## Amendment 1 (2026-11-20)
Accepted by the user on 2026-11-20, in answer to the wave-stopped findings above.
- **`{{var}}` highlighting deferred.** No body editor in crates/app/src has it today. The body editors are a bare `EditorState::new` (crates/app/src/ui/request/body.rs:201-203), with no `InputHighlighter` or `RangeDecoration`. `highlight::text_spans` is used only by the URL bar. AC2 and the `placeholder-gone` check now ask for editors that behave like the JSON body editor, with no highlighting. Highlighting for all body editors is a follow-up. The Context line that mentions the JSON editor's `{{var}}` highlighting is out of date for the same reason.
- **Spec superseded.** docs/plan/specs/2026-11-19-wisp-0-5-0.md:163 still names `{{var}}` highlighting for the GraphQL editor. For 0.5.0, this amendment supersedes that line. The scribe should note this in the spec. This task does not edit the spec.
- **`scope` check fixed.** It now allows `crates/service/src/plan.rs`, which Owns already lists.

## Wave resumed — user accepted (a): Amendment 1 defers `{{var}}` highlighting to a follow-up for all body editors and fixes the scope check; fresh full-branch code-reviewer

## Wave verified — 69e1c158f16d24d72db046aca54cd0d565587f85

## Outcome
Merged via PR from `task/193-graphql-editor`, verified at 69e1c158f16d24d72db046aca54cd0d565587f85 (tester PASS, reviewer APPROVE) after 0 review rounds and one wave stop resolved by Amendment 1, which deferred `{{var}}` highlighting for all body editors. The Query and Variables editors are in `crates/app/src/ui/request/graphql.rs` and `body.rs`. Open follow-ups are in PLAN.md under "Follow-ups from task 193".
