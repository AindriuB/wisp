# 170 — Importer credential leakage and imported descriptions (2E, H4, D3 import)

**Repo:** .
**Round:** 0
**Depends on:** 162, 165
**Owns:**
- crates/storage/src/import/**
- crates/storage/tests/import.rs
- crates/storage/tests/fixtures/** (new import fixtures only)

## Goal
Close hardening theme H4: no importer warning, file name or committed URL can
carry a credential, and an import validates everything before it writes. Postman
and OpenAPI descriptions land in the new `description` field.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" H4 row (:420), Feature 4 importers (:334), Waves 2E (:471).
- docs/plan/tasks/99-hardening-0-2-0.md AC19 :67, AC42 :69, AC43 :70, AC22 :74, AC26 :78, AC32 :89, AC33 :90, AC36 :93 (line refs are 0.2.0; re-locate).
- crates/storage/src/import/curl.rs (warnings; option parsing; extra URLs), openapi.rs:318-391 (servers: userinfo warning :344-358, `env_name` from description :371), write.rs (`folder_rel`, environment names, `{{var}}` scan), mod.rs (`split_userinfo`, `may_hold_credentials`), postman.rs.
- `Request.description` exists (162). Storage has no `url` crate dependency and `crates/storage/Cargo.toml` is owned by 171: parse hosts with the importers' existing helpers or a small local authority parser; add no dependency.
- 168 may change `wisp_core::redact` matching in parallel; if an import test then fails for that reason, report it rather than editing `redact.rs`.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 170 importer-hardening-descriptions`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-170`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (AC19) A curl warning names the construct, never its value. Test feeds `SECRETVAL` into every warning path and asserts no warning contains it.
- [ ] AC2 (AC42) With an unknown curl option, the `://` positional is the URL, or the import fails with a `Parse` error naming the option. Test: `curl --frobnicate https://u:SECRETVAL@h https://real/x` puts `SECRETVAL` in neither the URL nor any written TOML.
- [ ] AC3 (AC43) The extra-URL warning echoes only the host. Test: `https://u:SEC?RET#VAL@h2/x` gives a warning containing `h2` and none of `SEC`, `RET`, `VAL`.
- [ ] AC4 (AC22) An OpenAPI environment file name comes from the server URL host (userinfo stripped) unless the server description passes the secret-name heuristic as safe. Test: a description holding a credential-like value does not reach any file name.
- [ ] AC5 (AC26) The userinfo warning's host comes from the parsed authority after userinfo is stripped. Test: `https://u:p@h1/x?y=@h2` names `h1` and contains neither `u:` nor `p@`.
- [ ] AC6 (AC32) `folder_rel` is validated before the first file is created. Test: a bad `folder_rel` leaves the collection dir byte-identical (listing and contents).
- [ ] AC7 (AC33) Environment names are validated or suffixed before any write. Test: a server description `con` imports successfully.
- [ ] AC8 (AC36) One warning per `{{var}}` used but not defined by the import, naming only the variable. Test.
- [ ] AC9 (D3) Postman request descriptions and OpenAPI operation descriptions (the `description` field only; `summary` is not used) are copied into `Request.description` and written to the TOML. Tests for each importer; a request without one writes no `description` key.
- [ ] AC10 No new dependency; no file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `import-h4` command — AC1-AC9
  run: `cargo test -p wisp-storage --locked h4_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (prefix the new tests with `h4_`.)
- `scope` command — AC10
  run: `git diff --exit-code main...HEAD -- Cargo.toml Cargo.lock crates/storage/Cargo.toml crates/core crates/app crates/cli crates/service crates/storage/src/request.rs crates/storage/src/codec.rs`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Out of scope
- `redact.rs` heuristics (168); the curl importer's supported option set beyond AC42.
- GUI display of descriptions (175).

## Wave verified — 98045131587db0649d9c7697f42cc9cbb69a7805

## External review — findings

Key: pasted · reviewed head 86612c09acba8b2dbf7894f3cfc9e0d7889a6903 (PR #130; code unchanged at eaca40a) · round 1

- P2 — Quadratic variable scanning: `crates/storage/src/import/mod.rs:327–329` repeatedly searches overlapping suffixes for `}}`. A roughly 1 MB brace-heavy request body can trigger about 250 billion bytes of searches, causing excessive CPU use during import. Consume scanned tokens or use a linear parser.
- P2 — Missing authentication warnings: `crates/storage/src/import/mod.rs:325` scans the request's debug representation, which redacts authentication values. Undefined variables in bearer tokens, passwords and API keys therefore receive no warning. Scan the template fields directly.

## Review round 2 — findings

Reviewed HEAD: ae15c75edaad38a936fda5543f644c23cdbb48a6 (both external P2s fixed)

- [P2] crates/storage/src/import/mod.rs:702-715 — `brace_heavy_body_is_scanned_linearly` would also pass with the old quadratic scan: neither input (`"{{".repeat(..)`, `"{{a}".repeat(..)`) contains `}}`, so the old loop breaks after one linear search. Even with a `}}`, the test has no work limit, so a regression would only be slow.
  route: implementer — use an input like `"{{".repeat(n) + "}}"` and bound the work (e.g. count scanner steps or call a scanner with an operation counter) so that reverting to the old loop fails the test.

## External review — findings

Key: pasted · reviewed head eaca40a67b7c97d8b5b99c7efa53b164189f24e7 (PR #130) · round 3

- P2 — Authentication references escape validation: `crates/storage/src/import/mod.rs:325` scans the request's debug representation, which redacts authentication values. (Already fixed on the task branch at ae15c75, round 1; not yet on the PR.)
- P2 — Long descriptions are silently truncated: `crates/storage/src/import/postman.rs:36` and `crates/storage/src/import/openapi.rs:118` cap descriptions at 16 KiB, discarding valid content without warning. The spec (:323) permits descriptions of any length on disk.
  route: implementer — remove the 16 KiB description cap in both importers (and the duplicate truncate helpers), import descriptions whole; add a test with a 20 KiB description that round-trips intact.

## Wave verified — 0839ac216f838ee16cd05c7d44c7022c4b79c93a

## External review — findings

Key: pasted · reviewed head 3439423e6f8183d9db39c7f199458f7d037df64a (PR #130) · round 4 (the cap; not routed)

- P2 — curl import can select the wrong destination: `crates/storage/src/import/curl.rs:431`: importing `curl --silent api.example.com https://other.example.com` now selects `https://other.example.com`. The importer treats the schemeless first URL as an argument to `--silent`, although that option takes no argument. `--compressed` has the same problem. Retaining headers and body while changing the destination could send credentials or payloads to the wrong endpoint.

## Wave stopped — step 7 — external P2 at round 4 (cap)

## Wave resumed — user: one more fix run for the round-4 curl URL P2

## Wave stopped — step 2 — round-4 fix at 8f0eabb: the no-argument list fixes the P2, but the "--url wins" change (asked for in the orchestrator brief, not by the finding) leaks an unknown option value into a warning (AC1) and changes which URL is imported (DRIFT). Proposed: revert --url to in-order handling.

## Wave resumed — user: revert the "--url wins" change; keep the no-argument option list

## Review round 5 — findings

Reviewed HEAD: 8f0eabb95e050a6009942033b1d6b2b1d13e99e5 (round-4 P2 fixed by IGNORED_LONG_NO_ARG)

- [P2] crates/storage/src/import/curl.rs:486-494 — with an explicit `--url`, `pick_url` returns before the unknown-option branch, so an unknown option's value becomes an "extra URL" and its raw text goes into the warning (breaks AC1).
  input: `curl --frobnicate SECRETVAL --url https://real/x` → warning "extra URL (host `SECRETVAL`) is not imported".
- [DRIFT, settled by the user: revert] curl.rs:415,486-495 — `--url` now wins over an earlier positional URL; curl fetches URLs in order.
  input: `curl api.example.com --url https://other.example.com` → imports other.example.com (0839ac2 imported api.example.com).
  route: implementer — revert `--url` to in-order `set_url` handling as at 0839ac2, keep `IGNORED_LONG_NO_ARG`; drop or rewrite `h4_explicit_url_beats_positionals` to assert in-order behaviour; add a test for the AC1 input above.

## Wave verified — e20896a6faaa6a03f885ba7035fa4091404e34d3

## External review — findings

Key: pasted · reviewed head 3e910663a957f5dddec346f337886e6e6db4e802 (PR #130) · round 6 (past the cap; not routed)

- P2 — curl import can still select the wrong destination: `crates/storage/src/import/curl.rs:490–496`: valid no-argument options missing from `IGNORED_LONG_NO_ARG`, including `--http2-prior-knowledge`, trigger the unknown-option heuristic. `curl --http2-prior-knowledge api.example.com https://other.example.com -H 'Authorization: Bearer SECRET'` imports `https://other.example.com` while keeping the authorization header. Complete the no-argument option handling or reject ambiguous inputs instead of silently switching destinations.

## Wave stopped — step 7 — external P2 at round 6 (past cap); needs the user's OK for another run

## Wave resumed — user: one more run (round 6): after an unknown option, reject a command with both a schemeless positional and a `://` positional as ambiguous (Parse error naming the option, never a value) instead of picking a URL

## Review round 7 — findings
Reviewed HEAD e20896a6faaa6a03f885ba7035fa4091404e34d3 (full branch). This is the user-approved run past the cap (the "round 6" decision above).
- [P2] crates/storage/src/import/curl.rs:486-502 (`pick_url`) — after an unknown option, a command with one schemeless positional and one `://` positional silently imports the `://` one and keeps the headers and body, which switches the destination. This is the external P2 at 3e91066. The user decided to reject this case as ambiguous; the code still picks a URL.
  route: implementer — the user's recorded decision (last "## Wave resumed" above) is the spec: in the unknown-option branch, if the positionals include both a schemeless one and a `://` one, return `ImportError::Parse` naming the option, never a value. Add an `h4_` test with this exact input that asserts a Parse error and that `SECRET` appears in no message.
  input: `curl --http2-prior-knowledge api.example.com https://other.example.com -H 'Authorization: Bearer SECRET'`. `http2-prior-knowledge` is in neither `IGNORED_LONG_NO_ARG` (:18-76) nor the with-arg list, so `f.unknown` is set and `urls = [api.example.com, https://other.example.com]`; the code returns `https://other.example.com` with the Authorization header attached. It should return a Parse error.

## Wave verified — 25d8836af320c7cda16514e480905d8dac91d7cc

## Outcome
Merged at 25d8836 (PASS + APPROVE; AC1-AC10 met; the last verified run was workspace 1422 passed at e20896a), after three external reviews and review rounds 1-7. Importer warnings, file names and committed URLs no longer carry credentials, an import validates folder and environment names before it writes, and Postman and OpenAPI descriptions land in `Request.description` whole (the 16 KiB cap is removed, spec :323). The undefined-variable scan is linear and reads the template fields directly: auth values are included, descriptions and names are excluded. Known curl options that take no argument no longer consume the next argument (round 4). `--url` stays in-order, as curl does: round 5 reverted an orchestrator-requested `--url` priority that leaked an unknown option's value into a warning and changed the imported URL. Follow-ups are in PLAN.md. Round 7 (user-approved, past the cap): after an unknown curl option, more than one positional is rejected as ambiguous with a Parse error naming the option and never a value, instead of picking a URL.
