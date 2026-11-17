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

## Outcome
Merged at 0839ac2 (PASS + APPROVE: workspace 1418 passed, import-h4 11 passed, fmt and clippy clean; AC1-AC10 met), after two external reviews and review rounds 1-3. Importer warnings, file names and committed URLs no longer carry credentials, an import validates folder and environment names before it writes, and Postman and OpenAPI descriptions land in `Request.description` whole (the 16 KiB cap is removed, spec :323). The undefined-variable scan is linear and reads the template fields directly: auth values are included, descriptions and names are excluded. Follow-ups are in PLAN.md.
