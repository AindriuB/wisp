# 151 — Hide every expansion of a hidden variable in the Explain report

**Repo:** .
**Depends on:** none
**Owns:**
- crates/core/src/resolve/report.rs
- crates/core/src/resolve/report_tests.rs

## Goal
The Explain report hides a variable's listed value when it is hidden "by use"
(task 130 decisions (3), (4), (4a); AC13), but every other place that expands the
same variable still shows its value. Found in review of task 142: collection var
`foo = "CANARYFOO"`, headers `Authorization: Bearer {{foo}}` and `X-Other: {{foo}}`
give `foo = hidden: credential name` in the list but `X-Other: CANARYFOO`. Make the
report consistent with its own final hidden set.

## Decision (orchestrator, 2026-11-15)
Any variable in the report's final hidden set (the set `hidden_closure` returns,
after the downward closure) renders as `Hidden(Credential)` wherever the report
expands it: headers (names and values), params, the URL (path segments, inline query
values, authority), body fields and body byte count, and auth fields. Not only in
`variables`. Over-hiding is safe.
- The hidden set is computed once from the unhidden discovery walk plus the
  closure, as today. Places that become hidden only because of this rendering do
  NOT feed back into the set. No fixpoint. Example: `X: {{foo}}-{{bar}}`, with
  `foo` hidden elsewhere and `bar` used only there, renders `bar`'s text and keeps
  `bar` `Shown`. That value is already visible, as in decision (4a).
- Problems, warnings and per-variable locations still come from the unhidden
  walk. Rendering with hidden names must not drop a problem that sits inside a
  hidden variable, such as an unresolved nested variable.
- Amended (orchestrator, 2026-11-15): any problem or warning field whose text
  came from the value of a variable in the final hidden set (for example the
  path-parameter name or pattern in `MissingPathParam`, `PatternMismatch`,
  `InvalidPattern`, or the "invalid path parameter" message) carries a fixed
  marker such as `"<hidden>"`, never the value text. Variable names may still
  appear. The problem itself is still reported.

## Context
- docs/plan/tasks/retired/130-explain-core-report.md:40 (AC13), :64-68 (decisions (3), (4), (4a))
- crates/core/src/resolve/report.rs:323-382 — `explain_counted`. It renders the fields in one walk, then calls `w.variables()`.
- report.rs:778-781 — `Walker.hidden_vars` and `hide_names`. Today only the listing walker sets `hide_names`.
- report.rs:918-922 — `expand` hides a name in `hide_names` with `Acc::hide`. That call still expands the value to keep size bounds, so a render pass with hidden names costs expansions.
- report.rs:440-453 — `Acc::hide` and `Acc::bytes`. A hidden part makes `bytes` `None`.
- report.rs:1403-1430 — `hidden_closure`. report.rs:1435-1490 — `variables`, which shares one walker budget for every listed value.
- report_tests.rs:854-1060 — the existing AC13 route tests and their sentinel pattern. report_tests.rs:1072-1140 — the bomb tests, which bound the work at `3 * MAX_EXPANSIONS`.
- Task 83-1 bounded-work guarantee: the total work `explain_counted` reports must stay within `3 * MAX_EXPANSIONS` without loosening any bomb test. A second full render walk with its own budget would break this: discovery, render and listing at 10k each, plus the closure, is over 30k. Share a budget. Rendering and listing on one walker is the likely shape. The implementer chooses.
- Worktree: `./.claude/scripts/wt-new.sh . 151 explain-hidden-set`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-151`. Run every command in the foreground and leave nothing running.
- Never log a variable or secret value. Tests use literal sentinel strings only.

## Acceptance
- [ ] AC1 When a plain variable is hidden by one header (`Authorization: Bearer {{foo}}`), another header that expands it (`X-Other: {{foo}}`) renders `Hidden(Credential)` in place of its value. The sentinel is absent from `format!("{report:?}")`.
- [ ] AC2 Sentinel tests, one per place. In each, the same plain variable also feeds a hidden place, such as a Bearer token or an `Authorization` header. The sentinel must be absent from `format!("{report:?}")` when the variable is also used in: (a) a param in the params list, (b) an inline URL query value (`http://h/?q={{foo}}`), (c) a URL path segment (`http://h/{{foo}}/x`), (d) a plain auth field (SigV4 `region = {{foo}}`, with the SigV4 secret key `{{foo}}` or an `Authorization` header as the hidden place), (e) a non-credential form body field (`note={{foo}}`), and (f) a header name (`{{foo}}: v`).
- [ ] AC3 The form-body case in AC2(e) also has `body.bytes == None`. Any body that expands a hidden-set variable reports no byte count.
- [ ] AC4 A variable reached only through the closure is also hidden where it is expanded directly. Example: `token = "{{s}}"` used as a Bearer token, and `X-S: {{s}}` elsewhere. The sentinel `s` is absent.
- [ ] AC5 A variable used only in shown places still renders `Shown`, both in its places and in `variables`, in a report that also has hidden variables. Example: SigV4 `region = {{r}}`, Bearer `{{foo}}`, and `r` used nowhere else. The test asserts that `r`'s text appears in the region field and in the list.
- [ ] AC6 There is no feedback, per the Decision. In `X: {{foo}}-{{bar}}`, with `foo` hidden elsewhere, `bar` is listed `Shown` and its text appears in `X`.
- [ ] AC7 Problems are still reported, per the amended Decision. A hidden-set variable whose value references an undefined variable still yields the same `Problem::UnresolvedVar` it yields today (one test). Any problem or warning field whose text came from a hidden-set variable's value carries a fixed marker (e.g. `"<hidden>"`), not the value text; variable names may appear.
- [ ] AC8 The bounded-work guarantee holds. `a_variable_bomb_costs_bounded_work`, `a_shadowed_variable_bomb_costs_bounded_work` and `listed_variables_share_one_walker_budget` are unchanged and pass at `3 * MAX_EXPANSIONS`. A field cut short by the budget is never shown silently shortened: it is `Hidden(TooLong)`, or a too-large problem is reported.
- [ ] AC9 All existing `report_tests` pass unmodified. Only new tests are added. The public types and the signatures of `explain` and `explain_auth` do not change. The full suite, fmt and clippy pass.
- [ ] AC10 With `foo = "{CANARYFOO}"`, url `http://h/{{foo}}` and header `Authorization: Bearer {{foo}}`, `CANARYFOO` is absent from `format!("{report:?}")` and the path-parameter problem is still reported.

## Checks
- `report-unit` command — AC1-AC10
  run: `cargo test -p wisp-core resolve::report_tests 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
  (main has 46 tests in report_tests. The work must add at least 9, so expect at least 55 passed and 0 failed.)
- `tests-only-added` command — AC8, AC9
  run: `git diff main...HEAD -- crates/core/src/resolve/report_tests.rs | grep -E '^-[^-]' | wc -l | grep -qx ' *0'`
- `diff-scope` command — AC9
  run: `git diff --name-only main...HEAD | grep -vxE 'crates/core/src/resolve/report(_tests)?\.rs' | wc -l | grep -qx ' *0'`
- `suite-and-lint` command — AC9
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `shape` read — AC6, AC7, AC8, AC9, AC10
  how: In the diff, the hidden set is computed once and then applied to the rendered fields. Problems, warnings and locations come from the unhidden walk. No new budget is added that could exceed `3 * MAX_EXPANSIONS`. No `pub` item changes. No test asserts or prints a sentinel value outside an `!contains` check.

## Out of scope
- `explain_auth`. It walks auth alone by design (`explain_auth_expands_only_auth_fields`), so it cannot know about hides that come from headers or the URL. Task 140 uses only its kind and origin. Changing it is a follow-up.
- Changing what counts as hidden (credential names, AC13 routes (a)-(g), the URL per-part rule in (4a)). This task only applies the existing set more widely.
- The CLI (`wisp-cli explain`, task 142) and the Explain pane (task 140). Both consume the report unchanged.
- The send path (`resolve::engine`) and history redaction.

## Review round 1 — findings
Reviewed HEAD cd8e8c9.
- [P2] report.rs:329-334 (`finish_url` :603, `hide_query_values` :571) — the hidden render hides a variable that supplies `?`/`&`, which breaks query parsing, so a literal credential-named query value is shown: `http://h/{{q}}token=LITCANARY` with `q="?"`; `?a=1{{amp}}token=LITCANARY` with `amp="&"`; realistic `{{base}}&api_key=LIT` with `base="http://h/?v=1"`. Regression vs main. Fix: take the query and userinfo cuts from the discovery layout, or hide the whole URL when the hidden render loses a `?`, `&`, `=` or `@` the discovery layout had.
- [P2] Same cause for userinfo: `http://alice:LITCANARY{{at}}h.example/` with `at="@"` shows the literal password; main showed `Hidden(Userinfo)`.
- [P2] report.rs:1002-1016 with :329-334 — real work is about 4×`MAX_EXPANSIONS` while the reported `work` looks bounded: the userinfo sub-walker (`Walker::new` :1013) has its own uncounted budget and now runs again during render. Fix: skip taint/sub-walk work in the render pass and count all sub-walker expansions in `work`. Add a test with a 13-level doubling bomb in the URL before `@h/` plus `Authorization: Bearer {{b0}}`, asserting `work <= 3 * MAX_EXPANSIONS`.
- [P2, orchestrator] Problems carry value text of hidden-set variables (pre-existing on main). Settled by the AC7 / Decision amendment and AC10.
- [P3, follow-up — not this round] A header name from a hidden variable gets key `MARK h:<name>` and no longer merges with a same-named header (lists a header not sent; can duplicate Content-Type/Authorization).
- [P3] Stale doc comment on `hide_names` (report.rs:818). Fix with this round (one line).

## Review round 2 — findings (reviewed HEAD 9d03b1e)

1. [P2] report.rs:338-341: the `?&=@` cut-count heuristic is fooled by path-placeholder text returning in the render. Decision: remove it; if any variable in the final hidden set is used in the URL, the whole URL renders as one Hidden(Credential).
2. [P2] report.rs:367, :1075-1077: the userinfo sub-walker had its own budget, so work exceeded 3*MAX_EXPANSIONS. Decision: it shares the discovery walker's expansion counter.
3. [P2] report_tests.rs:1361-1379: replace the brittle `work >= 6000` floor with a differential test plus a budget-edge test.
Note (no change): Location::PathParam keeps a name that came from a hidden value only when req.path already lists that name.

## Outcome
Merged as PR #98 (08a781f) at 873b777. A variable in the report's final hidden set renders hidden everywhere the report expands it. Open items are in PLAN under the wave 2 follow-ups.
