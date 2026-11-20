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

## Orchestrator note (2026-11-20)

AC8 (164 stale redirect comments) moved to a follow-up: the comments are in crates/core/src/engine.rs (~:927, ~:1090), not crates/core/src/resolve/engine.rs, so they are outside 189's Owns. Comment-only, undoable; PLAN.md keeps the 164 follow-up open. Reviewers: judge 189 on AC1-AC7 and AC9.

## Review round 1 — findings

Reviewed HEAD: 998bf72ddd0129cd290ab697252a868ef85ed39a

- [P2] crates/service/src/plan.rs:247 — On a GraphQL body, a whole-body "body" redaction turns off the field-level leaf check, so the guard lets a resend go out with a redacted variables leaf still empty.
  route: implementer — the fix stays inside the plan.rs body arm and restores spec Feature 3 ("the variables are redacted like a JSON body … the 178 resend refusal applies"). When the body is GraphQL and `fields` is non-empty, also run `empty_json_leaves(variables, &fields)`, even when `whole` is set. Add GraphQL refusal tests (no test covers this arm today).
  input: history stores query `{ a(token: "abc") }` with variables `{"password":"hunter2"}`. `redact_request` clears the query, which records "body" (crates/storage/src/history.rs:643-645), and empties the leaf, which records "body.password" (:648). Redacted list = ["body", "body.password"]. The user retypes the query and leaves the variables as `{"password":""}` → `body_redaction_refusal` returns None and the request is sent with the empty password.
- [P2] crates/storage/src/request.rs:804-805 — `write_body` always writes `query` and `variables`, so the spec's own format with `variables` left out does not survive an unchanged save byte for byte.
  route: implementer — restores AC2 (missing `variables` reads as `""` and round-trips byte-for-byte when unchanged). Do not insert an empty `variables` (and arguably `query`) when the key is absent. Add a `graphql_file_*` test for it.
  input: file `…[body]\ntype = "graphql"\nquery = "{ a }"\n` → `load_request` then `save_request` → the file gains `variables = ""\n`.

## Review round 2 — findings

Reviewed HEAD: 9f46016b0bda68bc7528f05f798b9fd7f095cd29

- [P2] crates/service/src/plan.rs:250-252 — When a GraphQL body has "body" redacted, `unedited` is true whenever the draft's `variables` is blank. A GraphQL request with no variables, whose query was cleared for a credential, therefore stays refused after the user fills the query back in.
  route: implementer — fix inside the plan.rs body arm; restores spec Feature 3 (docs/plan/specs/2026-11-19-wisp-0-5-0.md:159-161): the query is redacted like a Text body, so the refusal lifts once the cleared text is filled in. Work out which field was cleared from `origin.stored_body` (the field that is blank there) and refuse only while that field is still blank in the draft. Keep the round-1 leaf check. Add a test: query refilled, variables blank → None.
  input: history stores query `{ a(token: "abc") }` with variables `""`. `redact_request` clears the query, records ["body"], leaves variables alone. The user retypes the query as `{ a(token: "new") }`, variables `""`. `body_redaction_refusal` returns `Some("Edit the request body before resending: the body was not stored")`.

Follow-up, not this round:
- [P3] plan.rs:275-279 — when only the leaf check triggers, the message says "the body was not stored" instead of naming the leaf.

## Wave verified — fcacbf495789e45a27e1c4dd8326e3577838b563

## Outcome
Tester PASS and reviewer APPROVE at fcacbf495789e45a27e1c4dd8326e3577838b563 after two review rounds. `Body::GraphQl` is in the model, the file format, resolve, Explain, history, the import scan, the resend plan and the app picker (read-only placeholder). AC8 was not done here: the stale redirect comments are in `crates/core/src/engine.rs`, outside Owns, so it moved to a `PLAN.md` follow-up. The other follow-ups are in `PLAN.md`.
AC10 was added after the external review, and the final verified head is 13b743a.

## External review — findings

Key: pasted by user · reviewed head 7945bc676a932576c353949e65dedf797bdd2a4c · round 3

P2] Resend guard permits silently discarded GraphQL variables — crates/service/src/plan.rs:253–254

When history clears both query and variables, the guard checks only whether the query was refilled. A supported variables object containing duplicate keys can be cleared entirely during history sanitization. Refilling just the query then permits resend with those variables silently omitted. Track which fields were cleared and retain the refusal until the missing variables are acknowledged.

## Wave stopped — step 7 — orchestrator finding: knowing which GraphQL field history cleared needs a design choice (new redaction names vs fail-safe both-fields rule)

## Amendment 2 (2026-11-20)
User decision, user's words: "Record per-field names". Fixes the round 3 P2 above: when history clears both the query and the variables it records only `"body"`, so the resend guard (`crates/service/src/plan.rs` ~:253-254) lifts once the query alone is refilled and the request goes out with the variables silently gone.

- [ ] AC10 GraphQL history redaction names and the resend guard:
  - for a GraphQL body, history records `"body.query"` when it clears the query and `"body.variables"` when it clears the whole variables string. Leaf redactions inside the variables keep their existing names (e.g. `"body.password"`). Non-GraphQL bodies keep `"body"`;
  - the resend guard refuses while any field named as cleared is still blank in the draft;
  - an old entry with a GraphQL body and `"body"` but no per-field names stays fail-safe: refuse until both the query and the variables are non-blank.

  Tests `graphql_resend_*` in `plan.rs`'s test module:
  (a) both fields cleared, only the query refilled → refused;
  (b) only the query cleared and refilled, variables blank → allowed;
  (c) only the variables cleared, query intact, variables still blank → refused; refilled → allowed;
  (d) an old `"body"`-only entry with both blank → refused until both are filled.
  Plus a `graphql_history_*` test in `tests/graphql.rs` that both-fields clearing records `"body.query"` and `"body.variables"`, not `"body"`.

Files: `crates/storage/src/history.rs` (the GraphQL redaction arm only) and `crates/service/src/plan.rs` (the GraphQL body arm and its tests). Both are already in Owns; nothing else changes.

Docs: no line needed. No user doc, `docs/architecture.md` or `docs/cli.md` lists history redaction names, and the spec's "the 178 resend refusal applies" stays true. Nothing is added to task 197.

Check for AC10:
- `graphql-resend` command — AC10
  run: `cargo test -p wisp-service --locked --lib graphql_resend_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([4-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `graphql-storage` (above) also covers the new `graphql_history_*` test.

## Wave resumed — user chose "Record per-field names" (2026-11-20): Amendment 2, then round 3 fix

## Wave stopped — step 2 — round cap: next fix round would be the 4th. Open: P2 regression at 5487019 (crates/service/src/plan.rs:276-286) — a stored GraphQL body with per-field names switched to None or a blank JSON/Text/XML draft is allowed; fix: treat gql_stored && (q || v) as whole on the non-GraphQL-draft path

## Wave resumed — user chose one more fix run past the cap ("One more fix run"), 2026-11-20

## Review round 4 — findings

Reviewed HEAD: 5487019778006caadd1e967b95b7e98a7a24f5a1

- [P2] crates/service/src/plan.rs:276-286 — Regression from round 3. A stored GraphQL body with per-field clearing names, whose draft switches the body type to None or to a blank JSON/Text/XML body, is now allowed. The legacy `"body"` name still refuses.
  route: implementer — on the non-GraphQL-draft path, count `gql_stored && (q || v)` as `whole`. That restores the 178 rule (`Body::None | Json | Text | Xml` blank = unedited). Add tests: stored `GraphQl{query:"",variables:""}` with each of `["body.query"]`, `["body.variables"]` and both names, and a draft of `Body::None` and of `Body::Json("")` → refused, in every case.
  input: stored `GraphQl{query:"",variables:""}`, redacted `["body.query"]`, draft `Body::None` → `body_redaction_refusal` returns None and the send goes out with no body.

## Wave verified — 13b743acde47f6a2be3f7d1e2f3c3e37415d2a19
