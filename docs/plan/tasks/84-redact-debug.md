# 84 — Redact secret values from Debug output in wisp-core

**Repo:** .
**Depends on:** none
**Owns:**
- crates/core/src/request.rs
- crates/core/src/model.rs
- crates/core/src/resolve/mod.rs
- crates/core/src/resolve/tests.rs (insertions only)
- docs/plan/tasks/84-redact-debug.md (the `## Notes` section only)

Branch `task/84-redact-debug`, worktree `.worktrees/wisp/84-redact-debug`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-84`. Commit on the
branch; do not push.

## Goal
Several wisp-core types print secrets in plain text under `{:?}` (PLAN.md,
"Carry-forward: crates/core", **Secrets in Debug (10)** and **Secrets in
warnings (11)**). Today only `Environment` redacts. After this task, no
`Debug` output of a wisp-core type contains a password, token, API key value,
proxy credential or resolved header value. Each redaction has a test that
fails without it. wisp-storage has no secret-bearing type of its own:
`CollectionTree` and `HistoryEntry` contain secrets only through the core
types above, so fixing core covers them. Notes must confirm this.

## Context
- crates/core/src/request.rs:34-52 (`Auth`), :178-188 (the redacting `Environment` Debug: the pattern to follow), :190-205 (its test).
- crates/core/src/model.rs:27-60 (`Header`, `PreparedRequest`, `PreparedBody`), :69-75 (`ClientSettings`; it also derives `Hash`/`Eq` as a client-cache key, so keep those derives).
- crates/core/src/resolve/mod.rs:42-55 (`ResolveWarning::PatternMismatch.value` holds a resolved value, which can be a secret).
- docs/conventions.md#logging — the rule this enforces. Do not edit it: the scribe updates it when the task is recorded.

## Contract
Replace the derived `Debug` with a hand-written one only where a field is secret. Every other field prints as it does today. A redacted value prints as the literal `"<redacted>"`.

| Type | Field(s) redacted | Prints instead |
|---|---|---|
| `Auth::Basic` | `password` | `<redacted>` (the username stays) |
| `Auth::Bearer` | `token` | `<redacted>` |
| `Auth::ApiKey` | `value` | `<redacted>` (the name and location stay) |
| `ClientSettings` | `proxy` userinfo | the URL with any `user:pass@` or `user@` replaced by `<redacted>@`; no userinfo → unchanged |
| `Header` | `value` | `<redacted>` (the name stays) |
| `PreparedRequest` | `url` query, body bytes | the URL up to and including `?`, then `<redacted>` when there is a query; `PreparedBody::Bytes` prints its length, not its bytes |
| `ResolveWarning::PatternMismatch` | `value` | `<redacted>` |

`Request`, `FolderDefaults`, `CollectionDefaults`, `CollectionTree`, `Command` and `Event` keep their derived Debug. They become safe through the fields above.

## Acceptance
- [ ] AC1 For each row of the table, a test named `debug_redacts_<what>` builds a value whose secret is a distinctive marker string. It asserts that `format!("{:?}", v)` and `format!("{:#?}", v)` both omit the marker and contain the field names that stay visible. There are at least 7 such tests.
- [ ] AC2 Tests on the composite types show the redaction carries through: `Request` with Bearer auth, `CollectionDefaults` with Basic auth, and `Command::Send` with an `Authorization` header and a query secret do not print their markers. These are three more `debug_redacts_*` tests.
- [ ] AC3 `ClientSettings` still derives `Clone, PartialEq, Eq, Hash, Serialize, Deserialize`; no type's serde output changes. The existing `commands_and_events_round_trip_through_serde_json` (crates/core/tests/engine.rs) passes unchanged.
- [ ] AC4 No new dependency; `Cargo.toml` and `Cargo.lock` are unchanged.
- [ ] AC5 `## Notes` confirms whether wisp-storage has any type of its own that holds a secret (expected: none) and lists any other core type found printing a secret value that is not in the table. Do not fix such a type; list it.
- [ ] AC6 `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Checks
- `redaction-unit` command — AC1, AC2. cargo test does not write JUnit XML here, so the command asserts the pass count itself (≥ 10).
  run: `cargo test -p wisp-core --lib debug_redacts_ 2>&1 | grep -E '^test result: ok\. ([1-9][0-9]+) passed'`
- `core-tests` command — AC3, no regressions
  run: `cargo test -p wisp-core`
- `no-new-deps` command — AC4
  run: `git diff --exit-code main...HEAD -- Cargo.toml Cargo.lock crates/core/Cargo.toml`
- `lint` command — AC6
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `scope` read — AC3, AC5
  how: only the table's types gain a manual Debug, the derives listed in AC3 remain, and Notes answers AC5.

## Out of scope
- Redacting `KeyValue`, `Var`, `TextBody` or authored URLs: these are file contents the user typed, not secret fields.
- crates/core/src/engine.rs (task 86) and any wisp-storage file.
- Editing docs/conventions.md#logging (the scribe does it).
- A `Secret<String>` newtype or other change to the model's field types.

## Notes

- wisp-storage has no type of its own that holds a secret. `HistoryEntry` stores the URL template (pre-resolution) and an `Outcome` with no message; `CollectionTree`/`Node`/`RequestMeta` hold paths, names and methods. Anything secret reaches them only through core types, now redacted.
- Core types still printing possibly-secret values, not in the table and not fixed:
  - `PreparedPartValue::Text(String)` (multipart text field values, resolved).
  - `EngineError.message` (can embed the resolved URL, as the `Outcome` doc comment in storage notes).
  - `Event::Headers` response header values, and `Event` generally if it carries response data, are unredacted.
  - `Request.url`/`KeyValue`/`Var` are authored text, out of scope per the task.
- `PreparedRequest` redacts everything after `?`, including the fragment.
