# 189 — Add a GraphQL body to the model, file format, resolve and history

**Repo:** .
**Round:** 0
**Depends on:** 183
**Owns:**
- crates/core/src/request.rs
- crates/core/src/resolve/engine.rs
- crates/core/src/resolve/report.rs
- crates/core/src/resolve/tests.rs
- crates/core/src/resolve/report_tests.rs
- crates/storage/src/request.rs
- crates/storage/src/history.rs (body cap and redaction arms only)
- crates/storage/src/import/mod.rs (the credential-scan arm at ~:363 only)
- crates/storage/src/import/{curl.rs,openapi.rs,postman.rs} (new match arms only, if the compiler requires them; no behaviour change)
- crates/storage/tests/graphql.rs (new)
- crates/service/src/plan.rs (body arms only)
- crates/app/src/workspace.rs (body arms only)
- crates/app/src/draft.rs (body arms only)
- crates/app/src/ui/request/body.rs (the `BodyKind`/`KINDS` entry and a read-only placeholder only)

## Goal
A request can carry `Body::GraphQl { query, variables }`. It round-trips to
TOML, resolves to a JSON POST body, appears in Explain, and is capped and
redacted in history like the body types it resembles. This task is the
compiler-driven sweep that every other GraphQL task builds on. The editor
is task 193.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 3": the file format, the resolve rule, history and compatibility.
- crates/core/src/request.rs:250-290: `TextBody` and `Body`.
- crates/core/src/resolve/engine.rs:670-700 `build_body`.
- crates/core/src/resolve/report.rs:150-160 `BodyKind`.
- crates/storage/src/request.rs: `read_body` :340, `write_body` :735.
- crates/storage/src/history.rs: `cap_request_body` :412-460, the body redaction pass :585.
- crates/storage/src/import/mod.rs:363 is the credential scan.
- crates/service/src/plan.rs:239.
- crates/app/src/ui/request/body.rs:25-45 `BodyKind`/`KINDS`.
- PLAN.md "Follow-ups from task 164": the stale comments at `resolve/engine.rs` ~:824 and ~:980.
- Worktree: `./.claude/scripts/wt-new.sh . 189 graphql-body-model`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-189`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `Body::GraphQl(GraphQlBody { query: String, variables: String })` exists, with serde round-trip.
- [ ] AC2 File format: `[body] type = "graphql"` with `query` and `variables` strings; a missing `variables` reads as `""`. `write_body` writes multi-line strings and round-trips a file byte-for-byte when the request is unchanged. `requests_equal` covers both fields. Tests `graphql_file_*`.
- [ ] AC3 Resolve:
  - `{{var}}` is substituted in both fields;
  - the body sent is compact JSON `{"query":q}`, plus `"variables":obj` when the substituted variables are not blank;
  - variables that are not a JSON object after substitution produce a resolve problem at the body with fixed text and no value;
  - the Content-Type is `application/json` unless the request sets one (the same rule as JSON).

  Tests `graphql_resolve_*`.
- [ ] AC4 Explain: `BodyKind::GraphQl` with the label `graphql`, and variables used in either field are listed as for JSON. Test `graphql_report_*`.
- [ ] AC5 History:
  - the query is redacted as a Text body and the variables as a JSON body (credential-named keys emptied);
  - the pair is capped at `BODY_CAP` and the request is marked `truncated` when cut.

  Tests `graphql_history_*` in `tests/graphql.rs`.
- [ ] AC6 The import credential scan covers the query and the variables (test). Postman import is unchanged here (task 194).
- [ ] AC7 App: the body picker lists "GraphQL". Selecting it, or opening a GraphQL request, shows the query and variables read-only with a one-line note that the editor is not built yet. Dirty tracking and save work through the existing body section.
- [ ] AC8 The 164 follow-up: the stale redirect comments in `resolve/engine.rs` (~:824, ~:980) describe the current engine-followed redirects. Comments only; no code change there.
- [ ] AC9 No file outside Owns changes, and the edits in files listed as "arms only" are arms only. `crates/cli/**` is untouched (labels flow through `label()`); if it needs a change, stop and report. fmt, clippy and the workspace suite pass.

## Checks
- `graphql-core` command — AC1, AC3, AC4
  run: `cargo test -p wisp-core --locked --lib graphql_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([6-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `graphql-storage` command — AC2, AC5, AC6
  run: `cargo test -p wisp-storage --locked --test graphql graphql_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([6-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `no-cli` command — AC9
  run: `git diff --exit-code main...HEAD -- crates/cli`
- `suite-and-lint` command — AC9
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `arms-only` read — AC7, AC8, AC9
  how: in `history.rs`, `import/*.rs`, `plan.rs`, `workspace.rs`, `draft.rs` and `body.rs`, the reviewer confirms the diff only adds GraphQL handling; the `engine.rs` redirect change is comment-only.

## Out of scope
- The GraphQL editor (193) and Postman GraphQL import (194).
- GET-encoded GraphQL, `operationName`, sidecar files and introspection.
- Changing `PreparedBody`, `Request` fields or `DirtySections`.
