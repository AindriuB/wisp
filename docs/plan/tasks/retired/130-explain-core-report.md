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
- [ ] AC13 A variable in the report's `variables` list has its value `Hidden(Credential)` (name, origin and locations still shown) when ANY of its use locations is (a) an auth field the report always hides (bearer token, Basic/Digest password, API key value, SigV4 secret key, OAuth secrets) or (b) the value of a credential-named header or query pair, inline in the URL or in the params list — including when its text is spliced into such a value (e.g. `q = "1&token=<s>"` used as `?a={{q}}`). Also `Hidden(Credential)` (clarified 2026-11-15, decision (3)) when (c) any place it is used is hidden in the report — wherever the produced value holds `Hidden(Credential)` or `Hidden(Userinfo)`: URL userinfo (`http://u:{{s}}@h/`, `http://{{creds}}@h/`), credential-named query pairs inside auth URL fields (OAuth `token_url = "https://idp/token?client_secret={{s}}"`), credential-named path params (`http://h/{token}` with token = `"{{s}}"`) — or (d) it is the value of a credential-named form or multipart body field (e.g. `password`). Variables used only in places the report shows stay `Shown`. Tests, one per route, each with a sentinel plain environment variable, assert the sentinel is absent from `format!("{report:?}")`: Bearer token, Basic password, `Authorization` header, API key value, SigV4 `secret_key`, the spliced query case, userinfo `u:{{s}}`, whole userinfo `{{creds}}`, OAuth `token_url` credential query, credential-named path param, credential-named form field, credential-named multipart field; one more test asserts a plain variable used only in a shown field (e.g. SigV4 region) stays `Shown`. General rule on top (decision (4), 2026-11-15): (e) a hidden variable hides every variable nested inside it, transitively, whatever its kind; (f) any place whose produced value holds `Hidden(Credential)` or `Hidden(Userinfo)` hides every variable used there — including the URL itself (inline query values, path segments) and Plain auth fields (SigV4 region/service, OAuth client_id/scope, API key name) — and, for the URL, per decision (4a): variables in hidden parts are hidden (per-part taint), and all the URL's variables are hidden when (i) a `{{` remains in the authority after the userinfo cut or (ii) the userinfo cut falls inside a `{{…}}` reference (e.g. a variable name containing `@`); a variable used only in shown URL parts stays `Shown` (e.g. `ok` in `https://user:pw@h/{{password}}/{{ok}}`); over-hiding is safe; (g) when a `Shown` variable's value is expanded for the list, a hidden variable nested inside it appears as `Hidden`, not expanded. Further tests, each asserting the sentinel is absent from `format!("{report:?}")`: (a) url `http://h/?q={{token}}`, (b) url `http://h/x/{{token}}`, (c) SigV4 region `{{token}}`, (d) OAuth client_id `{{client_token}}` — each with `token = "{{s}}"` (resp. `client_token`) and plain sentinel `s`; (e) url `http://x@{{h}}/` with `h = "{{s}}@host"`; (f) Bearer token `{{s}}` plus JSON body `{"a":"{{x}}"}` with `x = "{{s}}"`; (h) url `http://{{a@b}}/x` with `a@b = "u:{{s}}@host"`; (i) url `http://p@{{q@r}}/` with `q@r = "{{s}}@host"` — each with plain sentinel `s`.

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
  (3) Variable value hiding by use: a listed variable's value is `Hidden(Credential)` when any use location is an always-hidden auth field or the value of a credential-named header or query pair (inline in the URL or in params), including when spliced into such a value. Clarified (orchestrator, 2026-11-15, applying the user's "hide by use" choice): also hidden when ANY place it is used is hidden in the report — wherever the produced value holds `Hidden(Credential)` or `Hidden(Userinfo)` (URL userinfo, credential-named query pairs inside auth URL fields, credential-named path params) — and when it is the value of a credential-named form or multipart body field (body content is never shown, but the variable list must not reveal it). "Otherwise it stays `Shown`" means: used only in places the report shows. Overrides the spec's name-only rule (docs/plan/specs/2026-11-14-wisp-0-3-0.md:60) and AC4's "variables carry a `Shown`" where they conflict. Added as AC13.
- 2026-11-15 (user, approving a fourth fix round beyond the usual three):
  (4) Variable hiding follows a general rule on top of (3): a hidden variable hides every variable nested inside it, transitively, whatever its kind; any place whose produced value holds `Hidden(Credential)` or `Hidden(Userinfo)` hides every variable used there, including the URL itself (inline query values, path segments) and Plain auth fields (SigV4 region/service, OAuth client_id/scope, API key name), and when the userinfo is not fully determined by the template (a `{{…}}` remains in the authority after the cut) all the URL's variables are hidden — over-hiding is safe; when a `Shown` variable's value is expanded for the list, a hidden variable nested in it is shown as `Hidden`, not expanded. Added to AC13 as (e)-(g) with six tests.
- 2026-11-15 (orchestrator refinement of (4)(f) for the URL):
  (4a) The URL keeps a narrower rule than (4)(f)'s literal "every variable used there", so `https://user:pw@h/{{password}}/{{ok}}` keeps `ok` `Shown` (its value is visible in the URL anyway). URL rule: variables in hidden parts are hidden (per-part taint), and ALL the URL's variables are hidden when (i) a `{{` remains in the authority after the userinfo cut, or (ii) the userinfo cut falls inside a `{{…}}` reference (e.g. a variable name containing `@`). Added to AC13 (f) with two tests, (h) and (i).

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

## Review round 3 — findings (HEAD b53bab8)
- [P2] report.rs:935-942 a variable used in URL userinfo is listed `Shown` — settled by (3) clarification; AC13 (c).
- [P2] report.rs:1137-1147 a variable in a credential-named query pair inside an auth URL field (OAuth `token_url`) is listed `Shown` — settled by (3); AC13 (c).
- [P2] report.rs:1031-1032, :938-941 a variable used as a credential-named path param value is listed `Shown` — settled by (3); AC13 (c).
- [P3, in scope] report.rs:1269 a variable used as a credential-named form/multipart body field value is listed `Shown` — settled by (3); AC13 (d).

## Review round 4 — findings (HEAD 7526b52)
- [P2] report.rs:940-942, :1162 a credential variable nested in a variable used in the URL or a Plain auth field is listed `Shown` — settled by (4); AC13 (e), (f).
- [P3] report.rs:943-950 an `@` supplied by a variable after a literal `@` leaves userinfo variables `Shown` — settled by (4); AC13 (f).
- [P3] report.rs:1382-1419 a `Shown` variable's listed value expands a hidden nested variable — settled by (4); AC13 (g).

## Review round 5 — findings (HEAD 1de468f)
- [P2] report.rs:947-959 with userinfo_cut :592-597 — the cut lands inside a `{{…}}` whose name contains `@`, leaving userinfo variables `Shown` — settled by (4a); AC13 (f), tests (h), (i).
- [DRIFT] report.rs:944-962 the URL uses a narrower rule than (4)(f) — settled by (4a), which adopts it.

## PR #83 review round 1 — findings (HEAD 7d22195)
- 83-1 [Medium] report.rs:1405 — hidden_closure and variables() start a fresh 10k-expansion walker per hidden var per layer, including shadowed layers; explain can hang where send is instant. Fix: closure by var_refs reachability (no expansion), one shared budget; add a "variable bomb" test that counts operations (no timing asserts).
- 83-2 [Medium] report.rs:1352 — multipart part names are expanded in explain but sent literally by the engine (false UnresolvedVar, phantom uses). Fix: treat the name as literal; credential-named literal still hides value variables (AC13(d)). Test: part named `{{missing}}` gives no UnresolvedVar and no var use.
- Lows 83-3..83-8 and Nit 83-9: follow-ups, not this round.

## PR #83 review round 2 — findings

Reviewed HEAD 2eeadd2.

- [P2] report_tests.rs:1072-1091 — `a_variable_bomb_costs_bounded_work` passes with either half of the 83-1 fix reverted. Fix: shadowed-layer 2^8 bomb, cheap winning layer, ~2000 hidden names, work <= 3*MAX_EXPANSIONS.
- [P3] report.rs:1439-1476 — once the shared listing budget runs out, later values are cut short with no marker; a hidden variable with a cut value that has no parts is listed as `Shown([])`. Fix: hidden-set variables always Hidden(Credential); non-hidden cut values Hidden(TooLong).

## PR #83 review round 3 — findings

Reviewed HEAD ad8a2ad.

- [P2] report_tests.rs:1093-1113 — no test catches reverting the "one shared walker in variables()" half of 83-1 (fresh walker per listed variable). Fix: add a test with a bomb b0..b8, a chain d0..d7 with d7 = `{{k0}}..{{k99}}` and each k_i = `{{b0}}`, header `X-A: {{d0}}`; assert work <= 3*MAX_EXPANSIONS via `explain_counted`. Reviewer measured 10210 on HEAD vs 48337 with a fresh walker per listed variable.
- [P3, accepted residual] report.rs:1472 — a hidden-set variable whose value is empty and not cut lists as `Shown([])` (reveals only emptiness; matches finish/finish_for). Orchestrator decision: accepted, follow-up.

## Outcome
Merged as PR #83 (93f447b); PASS and APPROVE at 0d9531a. `core` has `ResolutionReport`, `explain` and `explain_auth`. Decisions (1)-(4a) on credential hiding are in HISTORY. Residual accepted: a hidden-set variable with an empty value lists as `Shown([])`. Lows 83-3 to 83-10 are follow-ups in PLAN.
