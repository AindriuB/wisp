# 131 — Add assertions: core evaluator, `Request.assertions`, `[[assert]]` codec

**Repo:** .
**Depends on:** 129
**Owns:**
- crates/core/src/assert.rs (new)
- crates/core/src/request.rs
- crates/core/src/lib.rs (one `pub mod assert;` line)
- crates/storage/src/codec.rs
- crates/storage/src/request.rs
- crates/storage/src/history.rs (`redact_request`, `AssertionRecord`/`AssertionState` types and the `BODY_CAP` parity test only; no change to `HistoryEntry` or `NewEntry`)
- crates/storage/src/import/{write.rs,mod.rs,curl.rs} (Request literal updates only, if any fail to compile)
- crates/storage/tests/history.rs
- crates/storage/tests/assert_codec.rs (new)
- crates/storage/tests/fixtures/assert/** (new)

## Goal
Requests gain declarative, report-only assertions stored as an additive
`[[assert]]` array in the request TOML, with a pure evaluator in `core`.
Unknown or malformed entries survive load and save verbatim. History never
stores expected or actual values. The history index fields themselves are
task 137 (they need `crates/app` edits).

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Feature 3": File format, validity matrix, Data model, Redaction rules for history, Tests — the contract
- crates/core/src/request.rs:307-336 — `Request` and its `Default`
- crates/storage/src/request.rs:97-110 load, :329-370 in-place save, :347-349 keep-what-exists rule
- crates/storage/src/history.rs:23 `BODY_CAP`; :296-340 `redact_request`
- docs/conventions.md:62-70 — `debug_redacts_*` tests
- Worktree `wt-new.sh . 131 assertions-core-storage`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-131`

## Acceptance
- [ ] AC1 `core::assert` defines `Assertion` (`Status`, `Header`, `Json`, `Unrecognised { raw: String, reason }`), `Op`, `Expected`, `AssertionResult { label, verdict }`, `Verdict { Pass, Fail { actual: Option<String> }, Skipped { reason } }`, `Assertion::label()`, `Assertion::fingerprint()` (first 8 bytes of SHA-256 over kind, target (header name or JSON path) and op only, never the expected value, 16 lowercase hex characters; an `Unrecognised` entry hashes the kind, target and op strings read from its raw table (missing ones as empty) plus the literal "unrecognised", never the raw text, `value` or any unknown key's value; a test shows two not-supported entries differing only in `value` share a fingerprint and the raw text is not hashed; see Decisions 2026-11-15 (2)) and `pub fn evaluate(&[Assertion], &Facts) -> Vec<AssertionResult>` with `Facts { status: u16, headers, body: Option<&[u8]> }`. `Fail.actual` is at most 80 characters.
- [ ] AC2 The validity matrix holds exactly: each kind and operator has a pass and a fail test; header names are case-insensitive and any of several same-name values may pass; `equals` is type-strict (200 vs "200" fails); `exists` takes an optional bool defaulting to true.
- [ ] AC3 JSON path accepts `$`, `.name`, `['name']`, `[n]` only; wildcard, filter, recursion, slice and negative index are rejected (tests for each). Path at most 256 characters and 16 segments.
- [ ] AC4 Limits: regex at most 256 characters, compiled with a size limit; JSON assertions on a body over `MAX_JSON_BODY` (1 MiB, a test in storage asserts it equals `BODY_CAP`) are `Skipped` with a body-size reason; a non-JSON body gives a distinct reason; the body is parsed once per `evaluate`; entries after the 50th are `Skipped` with a limit reason; `Unrecognised` is `Skipped` "not supported by this version" or "malformed".
- [ ] AC5 `AssertionResult` and any type holding an actual value has a hand-written `Debug` omitting it; `debug_redacts_*` tests cover `{:?}` and `{:#?}`.
- [ ] AC6 `Request` has `assertions: Vec<Assertion>` with `serde(default)`; `Request::default()` has it empty; `crates/app` still compiles unchanged.
- [ ] AC7 `load_request` reads `[[assert]]`; an unknown `kind`/`op` loads as `Unrecognised` with reason "not supported", a malformed known kind as `Unrecognised` with reason "malformed"; the request still loads.
- [ ] AC8 `save_request` writes the array only when non-empty or already present, preserves comments, and writes each `Unrecognised` entry's raw text back verbatim. Tests: exact TOML round-trip with comments; unknown kind kept byte-for-byte through load, save, reload; no entry dropped on save.
- [ ] AC9 0.2.0 compatibility: a test saves a fixture with `[[assert]]` after changing only `url` and asserts every other byte is unchanged. It is the branch's first commit and passes there, before the codec change (the 0.2.0 code path).
- [ ] AC10 `redact_request` clears `assertions` in the stored copy. Canary test: an expected value and a header value from the request appear nowhere in `index.jsonl` or `<id>.req.json`.
- [ ] AC11 `history.rs` defines `AssertionRecord { label: String, hash: String, state: AssertionState }` and `AssertionState { Pass, Fail, Skipped }` serialised lowercase, plus `AssertionRecord::from_result(&Assertion, &AssertionResult)`; a test pins the JSON shape `{"label":"status equals","hash":"<16 hex>","state":"pass"}`. `HistoryEntry` and `NewEntry` are unchanged.
- [ ] AC12 No new dependency: `Cargo.toml` and `Cargo.lock` unchanged.

## Checks
- `core-assert` command — AC1-AC5
  run: `cargo test -p wisp-core --lib assert`
- `storage` command — AC4, AC6-AC11
  run: `cargo test -p wisp-storage`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-new-deps` command — AC12
  run: `git diff --exit-code main...HEAD -- Cargo.toml Cargo.lock crates/core/Cargo.toml crates/storage/Cargo.toml`
- `first-commit` read — AC9
  how: the compatibility test is added in the first commit, before any `codec.rs` or `request.rs` change.
- `history-scope` read — AC10, AC11
  how: `HistoryEntry`/`NewEntry` untouched; no expected or actual value reaches any history struct.

## Out of scope
- `HistoryEntry.environment`/`.assertions`, `NewEntry` fields, the index write: task 137.
- `validate` findings for assertions: task 133. `DirtySections`, editor, Checks pane: wave 2.
- `crates/core/src/resolve/**`, `model.rs` (task 130); `openapi.rs` (task 129).

## Decisions
- 2026-11-15 (user, after code review round 1 of PR #84):
  (1) An unknown key on a known assertion kind (e.g. `kind="status" op="equals" value=200 negate=true`) makes the entry "not supported": shown, kept verbatim on save, never evaluated, so a newer Wisp's modifiers can't produce a wrong verdict here. Keys outside the validity matrix that are known keys of another kind (e.g. `name` on status, `path` on header) make it malformed (spec:180).
  (2) The assertion fingerprint hashes kind, target (header name / JSON path) and operator only, never the expected value, so a stored hash can't be a hash of a response value. AC1 amended.
  (2a) Clarification (orchestrator, review round 2, applying the user's "never hash the value" decision): an `Unrecognised` entry's fingerprint is computed from the kind, target (name or path) and op string fields read from its raw table when present (missing ones as empty), plus the literal "unrecognised" — never from the raw text and never from `value` or any unknown key's value. AC1 amended.

## Review round 1 — findings (HEAD 65d3a8b)
- [P2] codec.rs:749-768 write_assertions gives positions only to the first old.len() tables; reused tables past that and rebuilt Unrecognised entries keep stale positions, so reorder/insert reorders on disk (inputs: insert 301 then 300 at 0 → 300,301,202,201,203; append an Unrecognised → written before status). Fix: assign positions to every written table in order.
- [P2] codec.rs:766-767 moving an entry repositions only the parent [[assert]], not its child tables ([assert.x]/[[assert.x]]), so a newer version's sub-table moves to a different entry. Fix: move child tables with their parent.
- [P2] assert.rs:455-462 (with :404-411) header `matches` recompiles the regex per header value (50 assertions × 100 headers → 6.5 s in release; a rejected pattern still 82 ms each). Fix: compile each pattern once per evaluation (cache rejection too).
- [P2] codec.rs:650-656 target keys read only for header/json, so `status` with `name`/`path` loads as valid — settled by Decision (1)'s malformed rule.
- [P2→decision] codec.rs:642-666 unknown keys on known kinds ignored — settled by Decision (1).
- [SPEC] assert.rs:195-226 fingerprint hashes expected value (spec:197 vs :210-211) — settled by Decision (2).
- [P3] codec.rs:756-761 `value = nan` never equals itself, so unchanged load+save rebuilds the entry and drops its comments — fix (compare bit patterns or treat NaN specially).
- [P3] codec.rs:735-737 `assert = []` (non-array-of-tables) makes every save silently skip writing assertions — fix: error or replace with [[assert]] after the user adds one (choose the error if replacing could lose data).
- [P3] tests/assert_codec.rs:114 assertion is always true — fix.

## Review round 2 — findings (HEAD 093b1e5)
- [SPEC] assert.rs:217-220 Unrecognised fingerprint hashes the whole raw entry including `value` (e.g. `kind="header" name="Authorization" op="equals" value="Bearer s3cr3t" negate=true` — the hash changes with the value) — settled by Decision (2a).
- [P3, follow-up] codec.rs:678-681 when both an unknown key and a wrongly typed value are present the entry is labelled "malformed" rather than "not supported" (kept verbatim and never evaluated either way).

## Review round 3 — findings (HEAD 86f4391)
- [P2] crates/core/src/assert.rs:241-243 — `raw_string` skips every `[[...]]` line instead of stopping at child tables, so a missing top-level kind/name/path/op is read from a child array-of-tables. Input: `[[assert]]\nkind = "header"\nname = "Authorization"\n[[assert.any]]\nop = "Bearer s3cr3t"` → the fingerprint hashes "Bearer s3cr3t".
- [P2] crates/core/src/assert.rs:240-260 — no multi-line string tracking: a line inside a `"""`/`'''` value that looks like `op = "..."` is read as `op`. Input: `[[assert]]\nkind = "header"\nname = "A"\nvalue = """\nop = "s3cr3t"\n"""\nop = "future"` → hashes "s3cr3t".
- [P3, optional, follow-up] trailing comments / quoted keys / escapes make `raw_string` return "" or undecoded text → fingerprint collisions only, no leak.

## Attempt 1 — failed
Reviewer REQUEST CHANGES at 86a6f0a: AC4 '[?]' — body_parsed_once_per_evaluate (crates/core/src/assert.rs:1009) only checks that three verdicts agree and would pass if the body were parsed three times; no counter like REGEX_BUILDS. The code parses once (Body::Untried memo at :415); only the test is missing.
