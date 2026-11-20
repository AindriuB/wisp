# 194 — Translate Postman checks to `[[assert]]` and harden the Postman importer

**Repo:** .
**Round:** 0
**Depends on:** 189
**Owns:**
- crates/storage/src/import/postman.rs
- crates/storage/src/import/postman/** (new submodules, for example `postman/translate.rs`)
- crates/storage/src/import/mod.rs (`SecretSink` and `RequestCounter` only)
- crates/storage/tests/import_postman.rs (new)
- crates/storage/tests/fixtures/postman/** (new files only)

## Goal
A Postman collection whose request-level tests use a closed set of status,
header and JSON checks imports those checks as `[[assert]]` entries. Every
other script is still reported, never run, and never echoed. The task also:
- imports Postman GraphQL bodies;
- closes the Postman-side H4b ACs (AC27, AC28, AC29);
- stops the unsupported-method warning from echoing raw text.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 4": the closed-set table, literals, the warning text and the bounds. This brief assumes open question 7 is answered yes (the JSON form is included); the orchestrator drops that row if not.
- crates/storage/src/import/postman.rs:
  - `script_warning` :94-114; `items` :116-148 (folders); `request_item` :150-226;
  - the method warning ~:154-160 and :186-189; the userinfo-to-Basic step :212;
  - the body modes ~:430-440 (`"graphql"` warns today); `auth_of` :452.
- crates/storage/src/import/mod.rs:219-235 `RequestCounter`, :501-560 `SecretSink`.
- crates/core/src/assert.rs:36-99: `Op`, `Expected`, `Assertion` (no label; Postman test names are dropped). The JSON path syntax is as `Assertion::Json` parses it.
- docs/plan/tasks/99-hardening-0-2-0.md:83-85 (AC27-AC29).
- Worktree: `./.claude/scripts/wt-new.sh . 194 postman-test-translation`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-194`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 *(superseded by AC11, Amendment 1 — kept for history)* For a request-level `event` with `listen = "test"`, each `pm.test("…", function () { … })` or arrow-function test whose statements are all in the spec's closed set becomes one `[[assert]]` per statement, mapped per the table. The supported literals are integers, floats, single- and double-quoted strings with `\\`, `\"`, `\'` and `\n` escapes, `true` and `false`.
- [ ] AC2 *(first sentence superseded by AC12, Amendment 1; the warning, no-echo and pre-request sentences still hold)* A test with any statement outside the set (including a supported statement mixed with others) is not translated at all. A request with untranslated tests gets exactly one warning, "K of N tests not translated; scripts are not run". No warning contains script text, a test name or a literal. Pre-request scripts and folder- or collection-level scripts keep today's warning.
- [ ] AC3 Script text over 64 KiB per request is not parsed (all its tests count as untranslated) and is charged to the import budget. The parser is linear and hand-written. No new dependency.
- [ ] AC4 `body.mode = "graphql"` imports as `Body::GraphQl`, taking `graphql.query` and `graphql.variables` (an object is serialised, a string is kept). It is passed through the credential scan, and the "not imported" warning goes.
- [ ] AC5 (99-AC27) Folders count toward the node cap. Test: a collection of more empty folders than the cap is refused.
- [ ] AC6 (99-AC28) Collection variable keys are reserved in `SecretSink` before any secret is named. Test: no generated secret name equals a collection variable key.
- [ ] AC7 (99-AC29) URL userinfo becomes Basic auth only when the request has no `auth` object. An unsupported auth type with userinfo is not turned into Basic. Test.
- [ ] AC8 (170 P3) The unsupported-method warning does not include the raw method text. Test with a method containing a credential-like token.
- [ ] AC9 *(superseded by AC14, Amendment 1 — kept for history)* Tests in `tests/import_postman.rs`, prefix `pm_`:
  - one per table row;
  - a mixed test left untranslated;
  - an arrow function;
  - the warning carrying no script text or name (a canary string);
  - the 64 KiB bound;
  - GraphQL import;
  - AC5 to AC8.

  At least 15 tests.
- [ ] AC10 No file outside Owns changes. The edits to `import/mod.rs` touch only `SecretSink` and `RequestCounter`. Existing import tests pass unmodified. fmt, clippy and the workspace suite pass.

- [ ] AC11 (Amendment 1; replaces AC1) Grammar. A request-level `listen = "test"` script (its `exec` lines joined with `\n`) is translated only when the whole text is, after whitespace and `//` / `/* */` comments, a sequence of zero or more units `pm.test(<str>, function () { <stmts> })` or `pm.test(<str>, () => { <stmts> })`, each unit followed by `;`, a line terminator or the end. `<str>` is a single- or double-quoted string; `<stmts>` is one or more statements of the spec's Feature 4 table, each followed by `;`, a line terminator or the closing `}`. Whitespace and comments may sit between any two tokens. Each statement becomes one `[[assert]]` per the table, in source order. Literals are as AC1 lists; the JSON tail is `.ident` and `[int]` segments only, never `.length` (as on the branch today).
- [ ] AC12 (Amendment 1; replaces AC2's first sentence) Fail closed, whole script. If any byte of a script is outside AC11's grammar, the script yields no assertions and all its tests count as untranslated. That covers any other top-level statement or expression; any keyword (`if`, `else`, `do`, `for`, `while`, `switch`, `try`, `return`, `var`, `let`, `const`, `function` outside the callback position); a template literal; a `/` outside a comment; a callback with parameters; a nested `pm.test`; one non-table statement in any test body; an unterminated string or comment. For such a script N is the count of `pm.test(` substrings in its text, at least 1, and K = N. Scripts are judged independently: one failing script on a request does not untranslate another. The warning text and its no-echo rule are AC2's, unchanged.
- [ ] AC13 (Amendment 1) No resync. The translator never skips text it has not matched to AC11's grammar: no statement skipping, no block scanning, no regex, template or division classification. `skip_statement`, `block_body`, `block_body_inner`, `skip_regex`, `slash_kind`, `REGEX_KEYWORDS`, `AMBIGUOUS_WORDS` and the cursor fields `give_up`, `exhausted` and `scanned_to` are gone, with no replacement under another name. The parser is a single forward pass with constant lookahead and no recursion, so AC3's "linear" holds by construction.
- [ ] AC14 (Amendment 1; replaces AC9) Tests in `tests/import_postman.rs`, prefix `pm_`, at least 30 in all:
  - every test AC9 required, kept passing; `pm_mixed_test_not_translated` now asserts the whole script is untranslated (a clean test beside the mixed one yields nothing);
  - the round-1 to past-cap resync tests (`pm_regex_literal_does_not_desync_resync`, `pm_regex_after_keyword_does_not_desync_resync`, `pm_regex_in_arrow_callback_does_not_leak_nested_test`, `pm_regex_after_close_paren_does_not_leak_nested_test`, `pm_certain_division_keeps_later_tests_translated`, `pm_regex_after_arrow_keeps_later_table_test_translated`, `pm_ambiguous_slash_in_conditional_does_not_leak_nested_test`) are removed or rewritten to assert the AC12 outcome; none may assert that a test after non-grammar text translates;
  - the three linear-time tests stay;
  - these new tests, prefix `pm_strict_`, each asserting zero assertions and the exact "N of N tests not translated; scripts are not run" warning unless stated:
    - `pm_strict_comments_and_separators_translate` — table tests with `//` and `/* */` comments, with and without `;`, all translate, no warning;
    - `pm_strict_if_nested_test_left_untranslated` — `if (pm.response.code === 500) { pm.test('err', … status(500) …); }` then `pm.test('ok', … status(200) …);` gives no `Status Equals 500` and no assertions at all, "2 of 2";
    - `pm_strict_else_do_slash_left_untranslated` — the past-cap leak: a nested `pm.test` after `else /}x/.test(s);` inside an `if` block, and after `do /}/.test(s); while (0);`;
    - `pm_strict_nested_template_left_untranslated` — the round-1 leak: `` `${`}`}` `` inside an `if` block before a nested `pm.test`; also a template literal as a test name;
    - `pm_strict_division_left_untranslated` — `var secs = pm.response.responseTime / 1000;` before a table test, and `pm.expect(pm.response.json().a / 1000).to.eql(1)` inside a test (not a table form, so untranslated);
    - `pm_strict_regex_left_untranslated` — each round-1 to round-3 input (regex after `=>`, after `)`, after `return`, in an `if` block) before or around a table test;
    - `pm_strict_one_bad_test_untranslates_whole_script` — a clean test plus a test holding `console.log(1);`, "2 of 2";
    - `pm_strict_unterminated_input_left_untranslated` — an unterminated string, an unterminated `/*`, and a missing `})`: each terminates with no assertions;
    - `pm_strict_scripts_judged_independently` — two `test` events on one request, one clean and one not: the clean one's assertions are kept, warning "1 of 2".
- [ ] AC15 (Amendment 1) Owns, AC3 to AC8 and AC10 are unchanged. Work already on the branch for AC4 to AC8, the 64 KiB bound in `postman.rs` and the warning stays.

## Checks
- `pm-tests` command *(superseded by `pm-tests-strict`, Amendment 1)* — AC1, AC2, AC3, AC4, AC5, AC6, AC7, AC8, AC9
  run: `cargo test -p wisp-storage --locked --test import_postman pm_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[5-9]|[2-9][0-9]) passed; 0 failed; 0 ignored'`
- `no-dep` command — AC3
  run: `git diff --exit-code main...HEAD -- Cargo.lock crates/storage/Cargo.toml`
- `scope` command — AC10
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/import/postman' -e '^crates/storage/src/import/mod.rs$' -e '^crates/storage/tests/import_postman.rs$' -e '^crates/storage/tests/fixtures/postman/' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `closed-set` read *(superseded by `strict-grammar`, Amendment 1)* — AC1, AC2, AC3
  how: the reviewer checks that the parser accepts only the table's forms, that it is all-or-nothing per test, and that no input path can make it loop or recurse unboundedly.

- `pm-tests-strict` command — AC2, AC3, AC4, AC5, AC6, AC7, AC8, AC11, AC12, AC14
  run: `cargo test -p wisp-storage --locked --test import_postman pm_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (3[0-9]|[4-9][0-9]|[1-9][0-9]{2}) passed; 0 failed; 0 ignored'`
- `pm-strict-leaks` command — AC12, AC14
  run: `cargo test -p wisp-storage --locked --test import_postman pm_strict_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([9]|[1-9][0-9]) passed; 0 failed; 0 ignored'`
- `no-resync` command — AC13
  run: `! rg -n -w -e skip_statement -e block_body -e block_body_inner -e skip_regex -e slash_kind -e REGEX_KEYWORDS -e AMBIGUOUS_WORDS -e give_up -e exhausted -e scanned_to crates/storage/src/import/postman/`
- `strict-grammar` read — AC11, AC12, AC13, AC3
  how: the reviewer reads `translate.rs` and checks that every byte is either matched by AC11's grammar or ends the script as untranslated; that no path skips, rescans or classifies unmatched text; that the loop only moves forward with constant lookahead and no recursion; and that `pm_strict_*` inputs reproduce each leak named in the review rounds and the stops.

## Out of scope
- `openapi.rs`, `write.rs` and the URL-credential helpers in `mod.rs` (task 195).
- Running any script; folder- or collection-level tests; `pm.environment` or `pm.variables` writes.
- Status ranges (`to.be.ok`, `to.be.success`) and any form not in the table.

## Review round 1 — findings
Reviewed HEAD: c12994efb6a9ccddc1f0e9e09aff6b1475ac5946

- [P2] crates/storage/src/import/postman/translate.rs:46-54 (with block_body :170-199): the parser is quadratic, not linear as AC3 requires and as the module doc at :3 says. When `test_unit` fails, `block_body` may already have scanned to the end of the script. The cursor then goes back to `start`, and `skip_statement` only moves on to the next `;` at depth 0. Each unit can therefore rescan the rest of the script.
  route: implementer — restores AC3 ("The parser is linear") without changing the contract.
  input: one test script made of `pm.test('a', function () { ));` repeated to just under 64 KiB, so 2182 units. Then the same script on 100 requests, which is a 6.5 MB collection.  →  in a release build, one request takes 82 ms against 0.29 ms for a linear script of the same size. The 100-request collection takes 8.3 s, and about 13 s at the 10 MiB input cap; a debug build is slower again. `pm_hostile_scripts_terminate` (tests/import_postman.rs:230) does not cover this shape. It checks only that parsing terminates, so it passes either way.
  proof: probe test on an archived copy of HEAD, `--release`, timings as above.
- [P2] crates/storage/src/import/postman/translate.rs:288-322 (`json_tail`): it accepts JS expressions that are not property paths and turns them into JSON assertions that differ from the Postman check.
  route: implementer — this does not change the closed-set table. It only stops accepting forms outside the table, and those then count as untranslated, which the spec already allows.
  input: `pm.expect(pm.response.json().items.length).to.eql(3)` (a common Postman idiom)  →  imports as `Json{path:"$.items.length", Equals, Int(3)}`. `eval_json` in crates/core/src/assert.rs:634 applies `as_object()` to the array, so the assertion always fails where Postman passes. No warning is raised.
  input: `pm.expect(pm.response.json().total-1).to.eql(9)`  →  the subtraction becomes `$.total-1 equals 9`, because `-` is allowed in dot names (:298) and `check_path` accepts it (assert.rs:594).
- [P2] crates/storage/src/import/postman/translate.rs:136-166 (`skip_statement`): resync does not handle regex literals, so a `pm.test` nested inside a conditional block can be read as top-level. It is then translated as an unconditional assertion.
  route: implementer — AC1/AC2 and the `closed-set` check expect only the table's forms to be translated. Leaving such tests untranslated fits the spec.
  input: the test script `if (pm.response.code === 500) { /}/.test('x');` / `pm.test('err', function () { pm.response.to.have.status(500); });` / `}` / `pm.test('ok', function () { pm.response.to.have.status(200); });`  →  the assertions are `[Status Equals 500]`, which fails on every normal 200 response. The real `ok` test is swallowed, and the warning reads "2 of 3 tests not translated". The `}` inside the regex drops the depth to 0, so the `;` inside the `if` block ends the skip. Any regex containing an unbalanced `)`, `]`, `}` or a quote causes the same desync, for example `.to.match(/\)$/)`.
  proof: probe run on the archived copy produced exactly that output.

## Review round 2 — findings
Reviewed HEAD: d4f9697ff4dafa23cfd491e24b17341750285fd7

- [P2] crates/storage/src/import/postman/translate.rs:46-63 — the parser can still take quadratic time, so AC3's "parsing is linear" (and the module doc at :3) is still false. The new `exhausted` shortcut only covers a `block_body` that fails. Here `block_body` succeeds after a long scan, and the unit then fails anyway: in `statements(body)` (:420) or at `lit(")")` (:415). The cursor goes back to `start`, and `skip_statement` stops at the first `;` at depth 0, inside the failed body. So the next unit scans almost the whole script again. `pm_failing_units_parse_in_linear_time` (tests/import_postman.rs:321) does not cover this shape.
  route: implementer — restores AC3 as written.
  input: `pm.test('a', function () { ));` repeated 1920 times, then `})` repeated 1920 times (61,440 bytes, under the 64 KiB cap)  →  one request takes 544 ms in release, against 0.72 ms for a linear script of the same size. That is about 54 s for 100 such requests, and about 90 s at the 10 MiB input cap.
  proof: probe test on the archived copy of HEAD, `--release`, timings as above. The warning read "1921 of 1921 tests not translated".
- [P2] crates/storage/src/import/postman/translate.rs:167,216,391-396 — finding 3 is still open. `regex_allowed` decides whether `/` starts a regex from the byte before it. So a regex after a keyword (`return`, `typeof`, `void`, `case`, `throw`, `in`, `of`, `delete`, `await`, `yield`) is still read as ordinary bytes. A `}` inside it then makes resync stop inside a conditional block, and the `pm.test` nested there is translated as an unconditional assertion.
  route: implementer — AC1, AC2 and the `closed-set` check only allow the table's forms to be translated. Leaving such tests untranslated fits the spec.
  input: test script lines
    `if (pm.response.code === 500) {`
    `  const ok = [1].some(function (x) { return /}/.test(x) });`
    `  pm.test('err', function () { pm.response.to.have.status(500); });`
    `}`
    `pm.test('ok', function () { pm.response.to.have.status(200); });`
  →  assertions are `[Status Equals 500]`, with the warning "2 of 3 tests not translated". This is the same wrong output as round 1: it fails on every normal 200 response, and the real `ok` test is lost. `return /…/` inside a callback is ordinary JS.
  proof: probe run on the archived copy produced exactly that output.

## Review round 3 — findings
Reviewed HEAD: a85b76c30913a715a6f16892fb45699080577706

- [P2] crates/storage/src/import/postman/translate.rs:431 (`regex_allowed`), used at :180 and :233 — round 2 finding 2 is still open for the most common case. `/` after `=>` is still read as division, because `>` is not in the byte list. Inside a conditional block, an arrow callback with a regex holding an unbalanced `}`, `)`, `]`, `(`, `[` or `{` still makes the resync stop inside the block. The `pm.test` nested there is then translated as an unconditional assertion. The `if (…) /re/` form (regex after a closing `)`) has the same effect.
  route: implementer — AC1, AC2 and the `closed-set` check allow only the table's forms to be translated. Leaving these tests untranslated is within spec. Two possible fixes: treat `>` (and the other operator bytes that can never come before a division, `<`, `*`, `%`, `^`, `~`) as regex-allowed; or, for `)`, stop translating later tests once an ambiguous `/` was seen during resync.
  input: test script lines `if (pm.response.code === 500) {` / `  const ok = ['a)'].some(x => /\)$/.test(x));` / `  pm.test('err', function () { pm.response.to.have.status(500); });` / `}` / `pm.test('ok', function () { pm.response.to.have.status(200); });`  →  assertions `[Status Equals 500]`, warning "2 of 3 tests not translated". This is the same wrong output as rounds 1 and 2. Two other inputs give exactly the same result: `x => /}/.test(x)`, and `if (s) /}/.test(s);` on line 2.
  proof: probe tests on an archived copy of HEAD, all three inputs.
- [P2] crates/storage/src/import/postman/translate.rs:261-279 (`skip_regex`), called at :180 and :233 — parsing is still quadratic, so AC3's "Parsing is linear" (spec docs/plan/specs/2026-11-19-wisp-0-5-0.md:185, module doc :3) is still false. The path is different from round 2 finding 1. A failed `skip_regex` scans to the end of the line, leaves the cursor where it was, and the next regex-allowed `/` on the same line scans the rest of the line again. Opening a character class with `[` stops each attempt from closing on the next `/`.
  route: implementer — restores AC3 as written. One option: remember how far a failed attempt scanned, and treat later starts before that point on the same line as not-regex.
  input: one request whose test script is `pm.test('a', function () { ` + `(/[` repeated 21,300 times + ` });` (63,931 bytes, under the cap, no newline)  →  1.18 s for that one request in release, against 0.65 ms for a linear script of the same size. That is about 2 min for 100 such requests in a 6.4 MB collection; a debug build is slower. The same text without the `pm.test` wrapper takes 0.59 s, and so does the `=/[` form.
  proof: probe test on an archived copy of HEAD, `--release`.

## Wave stopped — step 2 — round cap (3 rounds used; next would be 4) + orchestrator DRIFT at f02bfdf: round 3 fixed both findings (29 regex variants, all pathological shapes linear), but the fail-closed rule (from the orchestrator's brief) gives up on every `/` that is not a certain regex start, including certain divisions after an identifier, number, `]` or closing quote. Tests after `var secs = pm.response.responseTime / 1000;` are no longer translated (AC1 weakened, fails safe). Options: (1) accept as a recorded AC1 residual; (2) one more round narrowing give-up to ambiguous `)`/`}` and failed regex attempts, plus the two P3s (`of`/`yield`/`await`/`}` certain-start; `=>` test that proves later tests still translate). Pre-existing NOTE: `skip_statement` lets an `if (…) { … }` with no `;` swallow the next top-level test.

## Wave resumed — user chose (a): one fresh implementer run past the cap under systematic-debugging, narrowing the fail-closed rule: a `/` after a non-keyword identifier, a number, `]` or a closing quote is a division again (so `responseTime / 1000`, `n / 2`, `d.a / d.b` keep later tests translated); give up only after an ambiguous `)` or `}` before `/`, or after a regex attempt that fails; also fold in the round-3 P3s — remove `of`, `yield`, `await` and `}` from the certain-regex-start set, and add a test that a regex after `=>` lets a later table-form test still translate. Linearity (AC3) must hold.

## Wave stopped — step 2 — past-cap run at 1718030 not CLEAN: the narrowing works (divisions keep later tests translated; linear; of/yield/await/`}` fail closed) but (1) `/` after `else`/`do` (and `break`/`continue`+newline) is read as division, so `else /}x/.test(s)` leaks a nested pm.test as unconditional again; (2) pm_ambiguous_slash_in_conditional_does_not_leak_nested_test passes under any classification; (3) pre-existing: a nested template inside `${…}` ends the backtick string early and leaks (since round 1); P3 `o. in /` lookback. Four rounds plus one past-cap run have each closed one path and exposed another in the hand-rolled JS resync scanner. Options for the user: re-plan to a strict grammar (translate only scripts whose top-level statements are all table-form pm.test calls; anything else leaves the whole script untranslated, no resync through arbitrary JS) / one more run on these findings / accept residual (not recommended: wrong unconditional assertions).

## Amendment 1 — strict grammar (re-plan)
**Reason.** Rounds 1 to 3 and the past-cap run each closed one path in the hand-written resync scanner and exposed another: a `pm.test` nested in a conditional leaked out as an unconditional assertion via regex after `)`/`}`/`=>`/keywords, then via `/` after `else`/`do`, and since round 1 via a nested template `${…}`. Telling regex from division without a JS parser has no fixed point. The user chose (2026-11-20) a strict grammar instead.

**What changes.** AC1 is replaced by AC11, AC2's per-test sentence by AC12, AC9 by AC14; AC13 and AC15 are new. Checks `pm-tests` and `closed-set` are replaced by `pm-tests-strict`, `pm-strict-leaks`, `no-resync` and `strict-grammar`. `no-dep`, `scope` and `suite-and-lint` stand. Owns is unchanged.

**What it costs, by design.** A script with anything but table-form `pm.test` units — a `var`, a `console.log`, a helper function, a division — is now wholly untranslated, where the old contract translated its clean tests. That is the accepted price of failing closed. The past-cap DRIFT (tests after `responseTime / 1000` not translated) is now the intended outcome, not a residual. The pre-existing NOTE that an `if (…) { … }` without `;` swallows the next test no longer applies: an `if` untranslates the script.

**Implementation note.** Keep `postman.rs` integration and the AC4 to AC8 work, and reuse `string`, `literal`, `json_tail` and `statement` in `translate.rs`. Replace the driver in `translate()` and parse the test body directly as table statements up to the closing `}`; delete the resync machinery AC13 names.

**Follow-ups unchanged.** The P3s recorded in the stops stay follow-ups; those about regex classification (`of`/`yield`/`await`/`}` certain-start, `o. in /` lookback) lapse, since nothing classifies `/` any more.

## Wave resumed — user chose re-plan to strict grammar (2026-11-20)
Next: one fresh implementer on branch `task/194-postman-test-translation` (update from `main` first if the merge is clean), working to Amendment 1.

## Review round 4 — findings
Reviewed HEAD: 502b71245e23a9407a48e952c0079f30611802e4

- [P2] crates/storage/src/import/postman/translate.rs:108-112 — A `//` comment ends only at `\n` or `\r`. In JavaScript it also ends at U+2028 and U+2029, so code after one of those characters, which Postman runs, is treated as part of the comment and never checked.
  route: implementer — AC11/AC12: match what JavaScript treats as a comment, or fail closed. Either end `//` at U+2028/U+2029 too (the next bytes then fail the grammar), or reject those characters anywhere in the script.
  input: exec `["// note<U+2028>if (pm.response.code === 500)\npm.test('err', function () { pm.response.to.have.status(500); });"]`  →  assertions `[Status Equals 500]`, no warning. Postman runs that test only when the status is 500.
  proof: a probe test on an archived copy of HEAD gave exactly that output.
- [P2] crates/storage/src/import/postman.rs (translate_tests, `Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect()`) — Non-string `exec` entries are dropped without a trace. Postman builds the script with `exec.join('\n')`, which turns a nested array into its text, so those bytes run in Postman and Wisp never sees them.
  route: implementer — AC12: a non-string `exec` entry makes the whole script untranslated; it must not disappear.
  input: exec `[["if (pm.response.code === 500)"], "pm.test('err', function () { pm.response.to.have.status(500); });"]`  →  assertions `[Status Equals 500]`, no warning.
  proof: a probe test on an archived copy of HEAD gave exactly that output.

## Wave stopped — step 2 — round cap: review round 4 after the re-plan has two P2 fail-open leaks (U+2028/U+2029 ending a // comment; non-string exec entries dropped)

## Wave resumed — user chose one narrow past-cap run (2026-11-20): fix both round-4 P2s (U+2028/U+2029 ending a // comment; non-string exec entries dropped) and two same-class P3s (raw line break inside a string or between `()` and `=>`; integer literal with a leading 0 followed by digits) — each must leave the whole script untranslated

## Review round 5 — findings
Reviewed HEAD: a3b718605cb116a5bf58c8e1a7d63f815234ca6e (past-cap run: all four round-4 items FIXED, each proven by mutation)

- [P2] crates/storage/src/import/postman/translate.rs:284-296 (`json_tail`, the `[` arm) — the leading-zero fix covers only `literal()`. A JSON index `[010]` is accepted and `check_path` reads it as decimal 10 (crates/core/src/assert.rs:613-614); Postman reads octal 8 (sloppy) or throws (strict).
  route: implementer — reject a multi-digit index starting with `0`; the whole script is then untranslated (AC11/AC12).
  input: exec `["pm.test('a', function () { pm.expect(pm.response.json().items[010]).to.eql(1); });"]`  →  `[Json { path: "$.items[010]", op: Equals, expected: Int(1) }]`, no warning.
  proof: probe test on an archive copy of HEAD gave exactly that output.
- [STYLE] crates/storage/tests/import_postman.rs:567-570 — `assert_one_warning` (used by the four new `pm_strict_` tests) checks only the warning count, not the exact "N of N tests not translated; scripts are not run" that AC14 (task file :60) requires. Use `assert_untranslated` (:383-392) for the three `import_tests` cases and assert "1 of 1" in `pm_strict_non_string_exec_entry`.

## Wave stopped — step 2 — past-cap run reviewed FIX: one P2 (leading-zero JSON index `[010]`) and one STYLE (exact warning text in four tests)

## Wave resumed — user chose one more past-cap run (2026-11-20): reject multi-digit JSON index starting with 0; assert exact warning text in the four new pm_strict_ tests (round 5)

## Attempt 6 — failed
/verify at 29200c8c04dbb3d22372717bd2de9ddb08741915: tester PASS (1691 passed; pm_ 38, pm_strict_ 16); reviewer REQUEST CHANGES on AC11/AC12.
- crates/storage/src/import/postman/translate.rs:273-282 — the `.` arm of `json_tail` accepts a segment starting with a digit (core assert.rs:592 `check_path` also allows it). exec `["pm.test('a', function () { pm.expect(pm.response.json().0).to.eql(1); });"]` (also `.a.0`, `.1b`, `items.0`) → `[Json { path: "$.0", op: Equals, expected: Int(1) }]`, no warning. In JS this is a SyntaxError, so Postman runs none of the script's tests; AC11 allows `.ident` only, so expected: no assertions and "1 of 1 tests not translated".
- Suggestions (follow-ups): warning N too low when a 64 KiB reset drops a non-string-exec count (postman.rs:142-151) or when nested-array text holds `pm.test(` (postman.rs:121); disabled events still translated (spec call).

## Wave stopped — step 3 — round cap: verify after the second past-cap run found digit-leading `.` member accepted (AC11)

## Wave resumed — user chose one more tiny past-cap run (2026-11-20): reject a `.` JSON member starting with a digit (Attempt 6)

## Wave verified — a5f31fdb94c9f57b6dc4b27f69b8ad81c9c0a9a8

## Outcome
Recorded at a5f31fdb94c9f57b6dc4b27f69b8ad81c9c0a9a8 (PR for `task/194-postman-test-translation`). Tester PASS (1692 passed; pm_ 39, pm_strict_ 17); reviewer APPROVE on all ACs, including AC11 to AC15 from Amendment 1. The importer translates a request-level test script to `[[assert]]` only when the whole script is table-form `pm.test` units; anything else leaves the script wholly untranslated with a warning. It also imports GraphQL bodies and carries the 170 P3 hardening (AC4 to AC8). The hand-written resync scanner was removed. Three past-cap runs were authorised by the user: round-4 leaks, round-5 index and test text, Attempt-6 digit member. Follow-ups are in PLAN under "Follow-ups from task 194".
