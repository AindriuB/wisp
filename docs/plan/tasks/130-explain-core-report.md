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
- [ ] AC4 The report carries everything 2B and 2D render (task-scoping rule, `docs/conventions.md` "Task-file scoping"): environment name and per-secret `{name, present}`; method; URL as `Shown`; params with kind (query or path), name, `Shown` value and origin; headers with name, `Shown` value, origin, overridden origins and a synthetic marker naming its source (body type or auth kind); auth with effective kind, origin (`Request`, `Folder(i)`, `Collection` or none), application (header, query, or at send by the engine for Digest, SigV4, OAuth2) and fields as name plus `Shown` value per the 2026-11-15 auth-display decision (configuration values shown subject to the length limit; identity and credentials hidden, see AC7); settings (timeout, follow redirects, verify TLS) each with value text and explicit-or-default; body kind, resolved byte length and variable names used, never content; variables with name, origin, `Shown` (value hidden per AC13), shadowed origins and the locations used; problems and warnings as `Problem`/`Location`.
- [ ] AC5 `pub fn explain(&Request, &Scope) -> ResolutionReport` and `pub fn explain_auth(&Request, &Scope) -> AuthReport` are re-exported from `resolve`; `explain_auth` does no variable expansion of non-auth fields.
- [ ] AC6 A secret-sourced variable is never expanded: the walker emits `Secret { name, resolved }` where it meets one. Canary test: a secret with a unique marker value used directly, nested in a plain variable, in a header, in the URL query and in auth; the marker appears in neither `format!("{report:?}")` nor `format!("{report:#?}")`.
- [ ] AC7 Credential-named headers, params, variables and auth fields are `Hidden(Credential)` even when the value is a plain literal; URL userinfo is `Hidden(Userinfo)`. A header, param or API-key value is also `Hidden(Credential)` when its name holds any `Hidden` or `Secret` part. Credential-named query pairs inline in the URL and in OAuth `token_url`/`auth_url` are hidden as history does (`history.rs:100-116`). Basic/Digest usernames are `Hidden(Userinfo)`; passwords, tokens, secret/access keys, session tokens, client secrets and API key values are always `Hidden(Credential)`, whatever the API key's header or param name.
- [ ] AC8 Dynamic variables are `Dynamic { name }` and never generated: a test asserts no UUID-shaped text in the report of a request using `$uuid`.
- [ ] AC9 Parity test: every fixture of `resolve/tests.rs` with no secret, dynamic, credential-named or over-200-character value runs through `resolve()` and `explain()`; the flattened report text equals the `resolve()` output field by field (URL, params, headers including synthetic ones, auth-applied header or query). The test asserts the number of fixtures compared is at least the number it found eligible and greater than zero. Added fixtures cover 2- and 3-folder header and auth inheritance; header names and positions are compared even where values are hidden (including auth overriding a user `Authorization` header); body byte length equals `resolve()`'s `PreparedBody` size.
- [ ] AC10 Precedence tests: request over folder over environment secret over environment plain over collection, with the losers listed as shadowed; header layering and Content-Type synthesis; inherited auth names the right folder index; unresolved, cycle and too-deep appear as problems.
- [ ] AC11 `resolve()`'s behaviour and public signature are unchanged and the existing `resolve` tests pass unmodified apart from fixture visibility.
- [ ] AC12 `report.rs` reads `Request` fields by name and does not destructure `Request` exhaustively (task 131 adds a field in parallel).
- [ ] AC13 A variable in the report's `variables` list has its value `Hidden(Credential)` (name, origin and locations still shown) when ANY of its use locations is (a) an auth field the report always hides (bearer token, Basic/Digest password, API key value, SigV4 secret key, OAuth secrets) or (b) the value of a credential-named header or query pair, inline in the URL or in the params list — including when its text is spliced into such a value (e.g. `q = "1&token=<s>"` used as `?a={{q}}`). Variables used only in shown places stay `Shown`. Tests, one per route, each with a sentinel plain environment variable, assert the sentinel is absent from `format!("{report:?}")`: Bearer token, Basic password, `Authorization` header, API key value, SigV4 `secret_key`, the spliced query case; one more test asserts a plain variable used only in a shown field (e.g. SigV4 region) stays `Shown`.

## Checks
- `core-resolve` command — AC2-AC11, AC13
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

## Decisions
- 2026-11-15 (user, after code review round 1 of PR #83):
  (1) Auth display: show configuration values — region, service, scope, client_id and OAuth URLs (with credential-named query params removed or hidden) — subject to the existing length limit; hide identity and credentials — Basic/Digest usernames are Hidden like URL userinfo and history; passwords, tokens, secret/access keys, session tokens, client secrets and API key values are ALWAYS `Hidden(Credential)`, regardless of the API key's header/param name. Resolves the AC4/AC7 conflict (AC4 amended).
  (2) Parity: within 130, add 2- and 3-folder header and auth-inheritance fixtures, compare header names and positions even where values are hidden (including auth overriding a user `Authorization` header), and compare body byte sizes with `resolve()`'s `PreparedBody`. Converting `resolve/tests.rs` into a shared fixture table is a follow-up.
- 2026-11-15 (user, resolving re-review [P2] at report.rs:1317-1349):
  (3) Variable value hiding by use: a listed variable's value is `Hidden(Credential)` when any use location is an always-hidden auth field or the value of a credential-named header or query pair (inline in the URL or in params), including when spliced into such a value; otherwise it stays `Shown`. Overrides the spec's name-only rule (docs/plan/specs/2026-11-14-wisp-0-3-0.md:60) and AC4's "variables carry a `Shown`" where they conflict. Added as AC13.

## Review round 1 — findings (HEAD 0e2a655)
- [P1] report.rs:1104-1111 API key value shown when its name isn't credential-like (e.g. `X-Tenant: sk_live_…`) — settled by (1); parity fixture report_tests.rs:141-152 must change.
- [P2] report.rs:1021, :875, :1103-1104 value hiding checks only plain-text names; a name from a credential-named variable or a secret is empty text so the value isn't hidden — fix: also hide when the name holds any `Hidden` or `Secret` part.
- [P2] report.rs:334, :503-533 credential-named query pairs inline in the URL (and OAuth `token_url`/`auth_url`) are shown — fix: hide them as history does (history.rs:100-116).
- [SPEC] auth field values (spec:56 vs :83) — settled by (1).
- [P2] parity coverage thin (reversing folder order fails no test) — settled by (2).
- [P3] eligibility filter drops credential-named headers and Basic/Bearer — covered by (2)'s name/position comparison.
- [P3] report.rs:449-467 two header names hidden by credential-named variables collide on MARK and merge — fix.
- [P3] report_tests.rs:529-543 `explain_auth` test asserts only its kind — strengthen.
- [P3] body byte length never compared — covered by (2).
- [NOTE] secret values aren't expanded, so cycles/unresolved/oversize inside a secret appear in `resolve()` but not `explain()`; document for 2B/2D.
- [NOTE] a non-credential-named variable nested inside a credential-named one is shown (spec:60 name-only rule).
