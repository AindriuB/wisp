# 142 — Add `wisp-cli validate` and `wisp-cli explain`

**Repo:** .
**Depends on:** none (needs 130 and 133, both merged)
**Owns:**
- crates/cli/** (src and tests; fixtures under crates/cli/tests/fixtures/)

## Goal
The two commands that 135 parsed but refused ("not available in this build")
now run: `validate` reports every finding of a collection and `explain` prints a
request's resolution report, both as plain text or versioned JSON, read-only,
with no secret value in the output and no network or keychain use.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md Feature 4: "Commands and output", "Redaction and read-only rules" (all 8), "Tests"
- crates/cli/src/main.rs:52 the refusal to replace; crates/cli/src/args.rs:59-61 usage text; crates/cli/src/dto.rs:5-15 sealed `Dto`; crates/cli/src/output.rs:7 `json`; crates/cli/src/cmd/inspect.rs (pattern to follow, incl. its secret handling)
- crates/storage/src/validate.rs:14-74 `Severity`, `Finding`, `validate(root) -> Report`
- crates/core/src/resolve/report.rs:303-385 `ResolutionReport`, `explain`; it is not `Serialize`
- crates/storage/src/environment.rs:230 `load_environment` (the spec says this one, not `load_environment_with`)
- scripts/check-boundaries.sh — must still pass (no `Engine`, `reqwest`, `keychain`, `fs::write`, ... in crates/cli/src)
- Worktree: `./.claude/scripts/wt-new.sh . 142 cli-validate-explain`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-142`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 `validate <collection> [--json]` prints every finding (path, key, message, severity) and exits 1 when any finding is an error, 0 otherwise, 3 when the collection cannot be read; the JSON document is `{"schema":"wisp-cli/1","command":"validate",...}`.
- [ ] AC2 `explain <collection> <request-path> [--env <name>] [--json]` maps `explain(&request, &scope)` to DTOs in `dto.rs` with the same sections as the GUI pane; without `--env` it says `environment: none`; exits 0 when the request exists (even with unresolved variables), 3 when it cannot be read, 2 on usage errors. Secret parts render as `secret NAME (resolved|not set)`, dynamic as `$NAME, generated at send`, hidden as `hidden: credential name`.
- [ ] AC3 Secrets: every secret value is replaced with a fixed placeholder before the scope is built; keychain-mode environments report `keychain: not loaded by the CLI`; no `impl Dto` for `ResolutionReport`, `Environment`, `PreparedRequest` or `HistoryEntry`.
- [ ] AC4 Canary test: a fixture with a secrets-file value, a plain variable `api_token`, an `Authorization` header literal and a keychain-mode environment; every command in text and `--json` runs through `CARGO_BIN_EXE_wisp-cli` and no canary string is in stdout or stderr.
- [ ] AC5 Read-only test: a fixture copied to a temp dir and made read-only; all three commands leave the file tree and mtimes unchanged.
- [ ] AC6 Fixture tests: a broken folder, a broken request, an invalid assertion and a symlink escape are all reported by one `validate` run; `explain` golden text and JSON (with the schema string) for one request; CRLF fixtures pass; a closed stdout pipe exits cleanly (no panic, no `unwrap` in new code).
- [ ] AC7 `scripts/check-boundaries.sh` passes; no new dependency; usage text no longer says "not available in this build"; full suite, fmt and clippy pass.

## Checks
- `cli-tests` command — AC1-AC6
  run: `cargo test -p wisp-cli 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `boundaries` command — AC7
  run: `./scripts/check-boundaries.sh && git diff --exit-code main...HEAD -- Cargo.lock`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `dto-only` read — AC3
  how: every `Serialize` type and `impl Dto` is in `dto.rs`; none wraps a core or storage type that carries values.

## Out of scope
- Any file outside crates/cli (a storage or core defect is reported, not fixed).
- `run`, `--keychain`, `--strict`, `snapshot` (0.4.0); packaging, signing and size steps (wave 3, 3B); `docs/cli.md` (scribe, 3C).

## Review round 1 — findings

Reviewed HEAD dd18ac4.

- [P2] crates/cli/src/main.rs:40,42 — `failure()` writes Failure messages to stderr raw, not through output::display; load_request/open_collection errors carry collection text (e.g. an ESC in a method or folder name). Fix: wrap `m` in output::display in both arms. Test: method "\u001b[31mEVIL" -> stderr has no raw 0x1b.
- [P2] crates/cli/tests/cli.rs:305 — validate_reports_everything_in_one_run never checks the invalid-assertion fixture; add "assert.toml" to `want`.
- [P2, orchestrator, not this round] core report.rs: a variable hidden by use still prints in other headers/params — split out as task 151 (core).
- [P3 follow-ups, not this round] canary fixture never references {{api_token}} or the secret {{k}}; golden JSON checks only four fields; non-File secrets modes all labelled "keychain".
- [P2, user review] crates/cli/src/cmd/explain.rs:75 — the code discards `environment_info.secret_names`; keychain (store) mode loads an empty secrets list, so declared secrets appear unresolved with an empty inventory, and a same-named lower-scope variable is wrongly reported (value and origin). Fix: keep declared names as empty (unavailable) secret entries without touching the keychain. Test: keychain env declaring `tok`, collection `tok = "FALLBACK-CANARY"`, `{{tok}}` in a header → text and --json show a secret marker, FALLBACK-CANARY nowhere.

## Attempt 1 — failed

Reviewer REQUEST CHANGES at 7f7a644: AC3 [?] — no test asserts the keychain note `keychain: not loaded by the CLI`.

## Outcome
Merged as PR #95 (c93d2d3) at 6de3965. `wisp-cli validate` and `explain` run in text and `--json`. Open items are in PLAN under the wave 2 follow-ups.
