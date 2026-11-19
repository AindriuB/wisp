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
- [ ] AC1 For a request-level `event` with `listen = "test"`, each `pm.test("…", function () { … })` or arrow-function test whose statements are all in the spec's closed set becomes one `[[assert]]` per statement, mapped per the table. The supported literals are integers, floats, single- and double-quoted strings with `\\`, `\"`, `\'` and `\n` escapes, `true` and `false`.
- [ ] AC2 A test with any statement outside the set (including a supported statement mixed with others) is not translated at all. A request with untranslated tests gets exactly one warning, "K of N tests not translated; scripts are not run". No warning contains script text, a test name or a literal. Pre-request scripts and folder- or collection-level scripts keep today's warning.
- [ ] AC3 Script text over 64 KiB per request is not parsed (all its tests count as untranslated) and is charged to the import budget. The parser is linear and hand-written. No new dependency.
- [ ] AC4 `body.mode = "graphql"` imports as `Body::GraphQl`, taking `graphql.query` and `graphql.variables` (an object is serialised, a string is kept). It is passed through the credential scan, and the "not imported" warning goes.
- [ ] AC5 (99-AC27) Folders count toward the node cap. Test: a collection of more empty folders than the cap is refused.
- [ ] AC6 (99-AC28) Collection variable keys are reserved in `SecretSink` before any secret is named. Test: no generated secret name equals a collection variable key.
- [ ] AC7 (99-AC29) URL userinfo becomes Basic auth only when the request has no `auth` object. An unsupported auth type with userinfo is not turned into Basic. Test.
- [ ] AC8 (170 P3) The unsupported-method warning does not include the raw method text. Test with a method containing a credential-like token.
- [ ] AC9 Tests in `tests/import_postman.rs`, prefix `pm_`:
  - one per table row;
  - a mixed test left untranslated;
  - an arrow function;
  - the warning carrying no script text or name (a canary string);
  - the 64 KiB bound;
  - GraphQL import;
  - AC5 to AC8.

  At least 15 tests.
- [ ] AC10 No file outside Owns changes. The edits to `import/mod.rs` touch only `SecretSink` and `RequestCounter`. Existing import tests pass unmodified. fmt, clippy and the workspace suite pass.

## Checks
- `pm-tests` command — AC1, AC2, AC3, AC4, AC5, AC6, AC7, AC8, AC9
  run: `cargo test -p wisp-storage --locked --test import_postman pm_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[5-9]|[2-9][0-9]) passed; 0 failed; 0 ignored'`
- `no-dep` command — AC3
  run: `git diff --exit-code main...HEAD -- Cargo.lock crates/storage/Cargo.toml`
- `scope` command — AC10
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/import/postman' -e '^crates/storage/src/import/mod.rs$' -e '^crates/storage/tests/import_postman.rs$' -e '^crates/storage/tests/fixtures/postman/' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `closed-set` read — AC1, AC2, AC3
  how: the reviewer checks that the parser accepts only the table's forms, that it is all-or-nothing per test, and that no input path can make it loop or recurse unboundedly.

## Out of scope
- `openapi.rs`, `write.rs` and the URL-credential helpers in `mod.rs` (task 195).
- Running any script; folder- or collection-level tests; `pm.environment` or `pm.variables` writes.
- Status ranges (`to.be.ok`, `to.be.success`) and any form not in the table.
