# 119 — Keep imported credentials out of committed TOML and off Basic auth

**Repo:** `.`
**Depends on:** none
**Size impact:** small (expected well under 20 KB on the Windows binary; no new dependency)
**Owns:**
- crates/storage/src/import/mod.rs (including its inline tests)
- crates/storage/src/import/curl.rs (including its inline tests)
- crates/storage/tests/import.rs
- crates/storage/src/import/postman.rs and crates/storage/src/import/openapi.rs — only the call sites where a URL is split/imported and their warnings (plus inline tests for those); in postman.rs also `url_of` and `split_url` and their inline tests (see Decisions, round 3)

## Goal
An imported collection is meant to be committed, so any credential the importers leave in a request or environment TOML is a credential in git. Three shared helper gaps leave one there: a value with `{{` anywhere is kept literally, and URL userinfo survives when the password holds `/`, `?` or `#`, or when the URL uses a lenient scheme form. Separately, curl's `--digest`, `--ntlm` and `--negotiate` with `-u` import as Basic, which sends the password in a form those schemes avoid. Carried from task 99 AC30, AC31, AC44, AC18.

## Context
- crates/storage/src/import/mod.rs:320-323 `SecretSink::secret_ref` returns any value containing `{{` unchanged.
- crates/storage/src/import/mod.rs:268-287 `split_userinfo`: the authority ends at the first `/`, `?` or `#`; `https:///u:p@h` and `http:/u:p@h` (accepted by reqwest) are not treated as having an authority. Callers: curl.rs, postman.rs:156, openapi.rs:250.
- crates/storage/src/import/curl.rs:343-358 (`--user`, unknown long options) and :477 onwards (`-u` to auth). The core model already has `Auth::Digest` (crates/core/src/request.rs:52).
- crates/storage/tests/import.rs:199 is the credential end-to-end test; its pattern is "write the import, then read every TOML under the root and assert the marker is absent".

## Acceptance
- [ ] AC1 (99-AC30) `secret_ref` keeps a value literally only when the whole trimmed value is exactly one `{{name}}` reference. Any other non-empty value, including `Bearer live-tok {{x}}`, goes to the secrets environment and the request holds a `{{name}}` reference. A test imports a curl header `Authorization: Bearer SECRETVAL {{x}}` and asserts no request or plain environment TOML contains `SECRETVAL`.
- [ ] AC2 (99-AC31) A URL whose password holds a raw `/`, `?` or `#` keeps no password in any written TOML: the authority is taken to end at the last `@` before the first `/` that follows that `@`, or the URL is refused with a warning that names no part of the userinfo. Two cases of a first authority segment `name:` followed by `/`, `?` or `#` and a later `@` (see Decisions, 2026-11-14):
  - Non-empty digit run (`name:<digits>`): taken as host:port and the URL is kept unchanged; every importer (curl, Postman, OpenAPI) adds a warning that names no part of the URL or userinfo, saying the URL may contain credentials and should be checked before committing.
  - Empty port (`name:` then `/`, `?` or `#`): never a real port, so the shared `split_userinfo` treats it as userinfo and moves it into secrets for every importer; it is not refused.
  - Tests, curl: `https://u:SEC/RET@h/x`, `https://u:SEC?RET@h/x` and `https://u:SEC#RET@h/x` leave no `SEC`, `RET` or the password in any request or plain environment TOML; `http://localhost:8080?email=a@b.com` and `https://u:12?34@h/x` import with the URL unchanged and the warning; `https://u:?x@h/x` and `https://u:#x@h/x` leave their userinfo in no TOML other than the secrets file.
  - Tests, for curl, Postman and OpenAPI each: `https://u:/SECPW@h/x` writes no `SECPW` to any TOML other than the secrets file (it goes to secrets), and `http://localhost:8080/users/me@example.com` imports unchanged with the warning.
  - Tests, Postman (round 3): `https://u:?SECPW@h/x` and `https://u:ab?SECPW@h/x` leave no `SECPW` or password fragment in any TOML other than the secrets file; `https://u:12?34@h/x` imports with the URL kept and the may-contain-credentials warning.
  - Tests, Postman (round 4; see Decisions, round 4): string URL `http://localhost:8080/users/:id?email=a@b.com&x=1` → url `http://localhost:8080/users/{id}`, params `email`/`x`, path var `id` (same as main); object URL `{"raw":"http://localhost:8080?email=a@b.com&x=1","query":[email,x]}` → url keeps the query, params empty, warning emitted.
- [ ] AC3 (99-AC44) `https:///u:SECRETVAL@h/x` and `http:/u:SECRETVAL@h/x` have their userinfo stripped into secrets (or are refused with a warning). A test with each asserts no request or plain environment TOML contains `SECRETVAL`.
- [ ] AC4 (99-AC18) curl `--digest -u user:SECRETVAL URL` imports `Auth::Digest` with both fields as secret refs. `--ntlm` or `--negotiate` with `-u` imports no auth, keeps both credentials in the secrets environment, and adds a warning that names the option and no value. Tested for all three options; no TOML other than the secrets file contains `SECRETVAL`.
- [ ] AC5 Existing import behaviour is otherwise unchanged: every existing test in crates/storage/src/import/** and crates/storage/tests/import.rs passes. Where AC1 changes an expected value, the test is updated and the PR description names it. Includes the Postman round 4 input `http://localhost:8080/users/:id?email=a@b.com&x=1`: url `http://localhost:8080/users/{id}`, params `email`/`x`, path var `id`, as on main.

## Checks
cargo writes no JUnit XML here, so these are command checks.
- `import-it` command — AC1, AC2, AC3, AC4
  run: `cargo test -p wisp-storage --test import`
  pass: exit 0, 0 failed, passed count at least the count on main 7ec7505 plus 4 (re-measure the base first; state both in the PR).
- `import-unit` command — AC1-AC5
  run: `cargo test -p wisp-storage --lib import`
  pass: exit 0, 0 failed.
- `storage-suite` command — AC5
  run: `cargo test -p wisp-storage`
- `lint` command — all
  run: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `exact-ref` read — AC1
  how: `secret_ref`'s keep-literal condition is "exactly one whole `{{name}}`", not a substring test.

## Standing constraints
- Worktree via `./.claude/scripts/wt-new.sh . 119 import-credential-leaks`, `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-119`.
- Secrets never logged or in docs; warnings name the construct, never its value.
- Tests never touch the real OS keychain; none is needed here. Storage tests use temp dirs.
- Never run `pkill` or `killall`.
- No Windows-only code is expected; the Windows CI job (54-CI) lands later.

## Out of scope
- crates/storage/src/import/write.rs is not edited. In postman.rs and openapi.rs, anything beyond the URL split/import call sites and their warnings (AC2), and in postman.rs `url_of` and `split_url`. If another inline test there fails because of the fix, stop and report.
- 99-AC19, AC20, AC42, AC43 (curl warning text, test-only and unknown-option items), and every F, G and other H item from task 99 (deferred).
- Making `import::write` transactional.

## Decisions
- 2026-11-14 (user decision, after review round 1 of PR #60) — A URL whose first authority segment is `name:<digits>`, the digit part non-empty, followed later by `@` (e.g. `http://localhost:8080/users/me@example.com`, indistinguishable from `https://u:1234/abcd@h/x`) is treated as host:port and kept; the import adds a warning naming no part of the URL or userinfo, saying the URL may contain credentials and should be checked before committing. Only clearly broken forms are refused, with an error naming no userinfo: an empty port (`u:/SECPW@h`) and a digit run cut by `?` or `#` (`u:12?34@h`). Reason: refusing every port+`@` URL blocked legitimate imports; the residual case (a password of only digits before its first raw `/`) is accepted and surfaced by the warning.
  - Correction 2026-11-14 (orchestrator, consistent with the user's "keep URL, warn" choice): a non-empty digit run cut by `?` or `#` (`u:12?34@h`, and also the legitimate `http://localhost:8080?email=a@b.com`) is just as ambiguous as one cut by `/`, so it is kept as host:port with the same warning, not refused. Only the empty-port form (`u:/SECPW@h`, `u:?x@h`, `u:#x@h`) is refused.
- 2026-11-14 (user decisions, review round 2 of PR #60) — (a) The orchestrator's correction is accepted: a non-empty digit run cut by `/`, `?` or `#` with a later `@` is kept with the no-value warning. (b) The empty-port form (`u:` then `/`, `?` or `#` and a later `@`, e.g. `https://u:/SECPW@h/x`) can never be a real port, so the shared `split_userinfo` treats it as userinfo and moves it into secrets for every importer; curl stops refusing it. This supersedes the empty-port refusal in the entry above. (c) The Postman and OpenAPI importers emit the same no-value "may contain credentials" warning as curl for a kept `name:<digits>…@` URL. Owns widened to the postman.rs and openapi.rs URL call sites and their warnings.
- 2026-11-14 (user decision, after review round 3 of PR #60) — One more round. Owns widened to postman.rs `url_of` and `split_url` (and their inline tests), so userinfo is split off the raw URL before the query is cut at `?`. AC2 gains Postman tests: `https://u:?SECPW@h/x` and `https://u:ab?SECPW@h/x` (no `SECPW` or password fragment in any non-secrets TOML), and `https://u:12?34@h/x` (kept with the may-contain-credentials warning, consistent with the accepted correction).
- 2026-11-14 (user decision, after review round 4 of PR #60) — Postman keeps the whole URL (no query cut) only when the authority's `name:<digits>` segment ends at `?` or `#` (e.g. `u:12?34@h/x`, `http://localhost:8080?email=a@b.com`). When the authority ends at `/`, the query is cut as before, so `http://localhost:8080/users/:id?email=a@b.com&x=1` keeps its params table and path variable `id` as on main. For a kept URL whose Postman URL object also has an explicit `query` array, the explicit array is ignored (the URL already carries the query and stays unchanged per AC2), so no parameter is sent twice.

## Review round 1 — findings (HEAD 605f9e8)
- [P2] crates/storage/src/import/mod.rs:296-299 — the fallback guard `looks_like_userinfo` treats a segment as `host:port` whenever the part after the first `:` is all digits, including empty, so when a password's first raw `/`, `?`, `#` follows only digits or comes right after the colon, userinfo stays in the URL and is written to the request TOML (breaks AC2: no password in any written TOML, or the URL is refused with a warning). Fix: when a later `@` exists but the segment looks like host:port, refuse the URL with a warning that names no part of the userinfo, or handle the empty-port case. Inputs: `curl 'https://u:1234/abcd@h/x'` → url keeps the password; `curl 'https://u:/SECPW@h/x'` → url keeps it; `curl 'https://u:12?34@h/x'` → url `https://u:12` and `34@h/x` into query params.
  - Settled by Decisions 2026-11-14: `u:1234/abcd@h` is kept with the may-contain-credentials warning; `u:/SECPW@h` and `u:12?34@h` are refused. AC2 amended.
- [P2] crates/storage/src/import/mod.rs:293-310 — the same fallback treats an IPv6 literal host as userinfo when the path has an `@`; the real host goes into secrets as a username and the request is rewritten to a different host (breaks AC5, existing behaviour unchanged). Fix: a segment starting with `[` is never userinfo. Input: `curl 'http://[::1]:8080/a@b'` → url `http://b`, Basic auth with `{{get_username}}`.
- [P3, follow-up] curl.rs:349-351 — `--basic`/`--anyauth` unknown, so `--digest --basic -u u:p` imports Digest though curl's last option wins.

## Review round 2 — findings (HEAD 681dbe0)
- [P2] The empty-port refusal and the may-contain-credentials warning exist only in curl; Postman (postman.rs:156) and OpenAPI (openapi.rs:248) still write the empty-port password to TOML and give no warning.
  - Settled by Decisions 2026-11-14 round 2 (b) and (c). AC2 rewritten.
- [STYLE] mod.rs:358, :363 — doc comments run to 105/107 chars; wrap at ~80 like their neighbours.
- [P3, follow-up] curl.rs:409-415 — the extra-URL warning in the Maybe case still prints the URL (its digits may be a password).
- [P3, follow-up] curl.rs:349-351 — `--basic`/`--anyauth` not recognised.

## Review round 3 — findings (HEAD f6c3798)
- [P2] postman.rs:284 `split_url` (reached from `url_of` :223/:273, before `split_userinfo` at :157) cuts the query at the first `?` before userinfo handling, so a password containing raw `?` is written to the request TOML and the empty-port/digit-run checks never fire for `?`.
  - Settled by Decisions 2026-11-14 round 3: Owns widened to `url_of`/`split_url`; AC2 Postman tests added.
- [P3, follow-up] curl.rs:409-411 — the extra-URL warning prints a `name:<digits>…@` URL.
- [P3, follow-up] curl.rs:349-351 — `--basic`/`--anyauth` not recognised.

## Review round 4 — findings (HEAD 73a47e8)
- [P2] postman.rs:293-298 — `keep_whole` suppresses the query cut for any `name:<digits>` URL with a later `@`, even when the authority ends at `/`; ordinary Postman URLs lose params and path vars (AC5).
  - Settled by Decisions 2026-11-14 round 4.
- [P2] postman.rs:293-299 — a kept URL with an explicit `query` array sends each param twice.
  - Settled by Decisions 2026-11-14 round 4.
- [NOTE] postman.rs:221 — `url_of` doc comment still says "url without query".

## Appendix: superseded
Agents: do not read this section.

AC2 before the round 2 rewrite:
- [ ] AC2 (99-AC31) A URL whose password holds a raw `/`, `?` or `#` keeps no password in any written TOML: the authority is taken to end at the last `@` before the first `/` that follows that `@`, or the URL is refused with a warning that names no part of the userinfo. Tests import `https://u:SEC/RET@h/x`, `https://u:SEC?RET@h/x` and `https://u:SEC#RET@h/x` through curl and assert no request or plain environment TOML contains `SEC`, `RET` or the password. Exception (see Decisions, 2026-11-14): a first authority segment `name:<digits>` (non-empty, all digits) with a later `@` is taken as host:port and the URL is kept, and the import adds a warning that names no part of the URL or userinfo, saying the URL may contain credentials and should be checked before committing (test: `http://localhost:8080/users/me@example.com` imports unchanged with that warning). Only clearly broken forms are refused, with an error naming no userinfo: an empty port (`https://u:/SECPW@h/x`) and a digit run cut by `?` or `#` (`https://u:12?34@h/x`); tests assert neither writes `SECPW`, `12` or `34` as userinfo to any TOML.
  - Correction 2026-11-14 (orchestrator, per the user's "keep URL, warn" choice; see Decisions): a non-empty digit run cut by `?` or `#` is kept as host:port with the same warning, not refused. Only the empty-port form is refused. Tests: `http://localhost:8080?email=a@b.com` and `https://u:12?34@h/x` import with the URL unchanged and the may-contain-credentials warning; `https://u:/SECPW@h/x`, `https://u:?x@h/x` and `https://u:#x@h/x` are refused with an error naming no userinfo, and the tests assert that no TOML contains `SECPW` or the userinfo of the other two.
