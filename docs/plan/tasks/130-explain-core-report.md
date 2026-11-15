# 130 — Add the core `ResolutionReport` and `explain` walker

**Repo:** .
**Depends on:** 129
**Owns:**
- crates/core/src/resolve/report.rs (new)
- crates/core/src/resolve/report_tests.rs (new)
- crates/core/src/resolve/mod.rs
- crates/core/src/resolve/engine.rs (visibility changes and `lookup_all` only)
- crates/core/src/resolve/tests.rs (fixture visibility only: make fixtures reachable from `report_tests.rs`)
- crates/core/src/model.rs (`redact_userinfo` to `pub(crate)` only)

## Goal
Give `core` a read-only explanation of the effective request: every variable,
URL part, param, header, auth choice, setting and body fact with the layer it
came from, without ever holding a secret value. The GUI Explain pane and
context strip (wave 2, 2B) and `wisp-cli explain` (2D) both consume this type.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Feature 1 — Explain Request": Data model, Implementation approach, Security and redaction, Tests — the contract; follow it
- crates/core/src/resolve/mod.rs:15-33 — `Problem`, `Location` (reuse)
- crates/core/src/resolve/engine.rs:19-26 bounds; :77-84 `VarSource::Folder` index; :442-458 `merged_headers`; :463-474 `apply_auth`
- crates/core/src/redact.rs:32-35 — `is_credential_name`
- crates/core/src/model.rs:72-78 — `redact_userinfo`
- Worktree `wt-new.sh . 130 explain-core-report`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-130`

## Acceptance
- [ ] AC1 `report.rs` defines `ResolutionReport` deriving `Debug, Clone, PartialEq` and not `Serialize`/`Deserialize`; no type in `report.rs` derives or implements `Serialize`.
- [ ] AC2 Display values are `Shown`, a sequence of parts `Text(String)`, `Secret { name, resolved: bool }`, `Dynamic { name }`, `Hidden(HideReason)` with `HideReason` at least `Credential`, `Userinfo`, `TooLong`. A text value over 200 characters is `Hidden(TooLong)`.
- [ ] AC3 `Origin` has `Request`, `Folder(usize)` (index into `Scope::folders`, outermost 0), `Collection`, `Environment`, `Secret`, `Dynamic`, `BodyType`, `Auth`.
- [ ] AC4 The report carries everything 2B and 2D render (task-scoping rule, `docs/conventions.md` "Task-file scoping"): environment name and per-secret `{name, present}`; method; URL as `Shown`; params with kind (query or path), name, `Shown` value and origin; headers with name, `Shown` value, origin, overridden origins and a synthetic marker naming its source (body type or auth kind); auth with effective kind, origin (`Request`, `Folder(i)`, `Collection` or none), application (header, query, or at send by the engine for Digest, SigV4, OAuth2) and field names only; settings (timeout, follow redirects, verify TLS) each with value text and explicit-or-default; body kind, resolved byte length and variable names used, never content; variables with name, origin, `Shown`, shadowed origins and the locations used; problems and warnings as `Problem`/`Location`.
- [ ] AC5 `pub fn explain(&Request, &Scope) -> ResolutionReport` and `pub fn explain_auth(&Request, &Scope) -> AuthReport` are re-exported from `resolve`; `explain_auth` does no variable expansion of non-auth fields.
- [ ] AC6 A secret-sourced variable is never expanded: the walker emits `Secret { name, resolved }` where it meets one. Canary test: a secret with a unique marker value used directly, nested in a plain variable, in a header, in the URL query and in auth; the marker appears in neither `format!("{report:?}")` nor `format!("{report:#?}")`.
- [ ] AC7 Credential-named headers, params, variables and auth fields are `Hidden(Credential)` even when the value is a plain literal; URL userinfo is `Hidden(Userinfo)`.
- [ ] AC8 Dynamic variables are `Dynamic { name }` and never generated: a test asserts no UUID-shaped text in the report of a request using `$uuid`.
- [ ] AC9 Parity test: every fixture of `resolve/tests.rs` with no secret, dynamic, credential-named or over-200-character value runs through `resolve()` and `explain()`; the flattened report text equals the `resolve()` output field by field (URL, params, headers including synthetic ones, auth-applied header or query). The test asserts the number of fixtures compared is at least the number it found eligible and greater than zero.
- [ ] AC10 Precedence tests: request over folder over environment secret over environment plain over collection, with the losers listed as shadowed; header layering and Content-Type synthesis; inherited auth names the right folder index; unresolved, cycle and too-deep appear as problems.
- [ ] AC11 `resolve()`'s behaviour and public signature are unchanged and the existing `resolve` tests pass unmodified apart from fixture visibility.
- [ ] AC12 `report.rs` reads `Request` fields by name and does not destructure `Request` exhaustively (task 131 adds a field in parallel).

## Checks
- `core-resolve` command — AC2-AC11
  run: `cargo test -p wisp-core --lib resolve`
- `suite-and-lint` command — AC11
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-serialize` command — AC1
  run: `! rg -n 'Serialize|Deserialize' crates/core/src/resolve/report.rs`
- `shape` read — AC3, AC4, AC5, AC12
  how: the public types carry every listed field; `engine.rs` diff is visibility plus `lookup_all`; no exhaustive `Request { .. }` pattern.
- `parity-non-vacuous` read — AC9
  how: the eligibility filter is explicit and the count assertion exists.

## Out of scope
- `VarLookup`, `lookup_var`, `hover_text` and `VarSource`: unchanged.
- Any `crates/app` or `crates/cli` file; rendering text for the pane or CLI.
- `crates/core/src/lib.rs` and `request.rs` (task 131 owns them).
