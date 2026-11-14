# 119 — Keep imported credentials out of committed TOML and off Basic auth

**Repo:** `.`
**Depends on:** none
**Size impact:** small (expected well under 20 KB on the Windows binary; no new dependency)
**Owns:**
- crates/storage/src/import/mod.rs (including its inline tests)
- crates/storage/src/import/curl.rs (including its inline tests)
- crates/storage/tests/import.rs

## Goal
An imported collection is meant to be committed, so any credential the importers leave in a request or environment TOML is a credential in git. Three shared helper gaps leave one there: a value with `{{` anywhere is kept literally, and URL userinfo survives when the password holds `/`, `?` or `#`, or when the URL uses a lenient scheme form. Separately, curl's `--digest`, `--ntlm` and `--negotiate` with `-u` import as Basic, which sends the password in a form those schemes avoid. Carried from task 99 AC30, AC31, AC44, AC18.

## Context
- crates/storage/src/import/mod.rs:320-323 `SecretSink::secret_ref` returns any value containing `{{` unchanged.
- crates/storage/src/import/mod.rs:268-287 `split_userinfo`: the authority ends at the first `/`, `?` or `#`; `https:///u:p@h` and `http:/u:p@h` (accepted by reqwest) are not treated as having an authority. Callers: curl.rs, postman.rs:156, openapi.rs:250.
- crates/storage/src/import/curl.rs:343-358 (`--user`, unknown long options) and :477 onwards (`-u` to auth). The core model already has `Auth::Digest` (crates/core/src/request.rs:52).
- crates/storage/tests/import.rs:199 is the credential end-to-end test; its pattern is "write the import, then read every TOML under the root and assert the marker is absent".

## Acceptance
- [ ] AC1 (99-AC30) `secret_ref` keeps a value literally only when the whole trimmed value is exactly one `{{name}}` reference. Any other non-empty value, including `Bearer live-tok {{x}}`, goes to the secrets environment and the request holds a `{{name}}` reference. A test imports a curl header `Authorization: Bearer SECRETVAL {{x}}` and asserts no request or plain environment TOML contains `SECRETVAL`.
- [ ] AC2 (99-AC31) A URL whose password holds a raw `/`, `?` or `#` keeps no password in any written TOML: the authority is taken to end at the last `@` before the first `/` that follows that `@`, or the URL is refused with a warning that names no part of the userinfo. Tests import `https://u:SEC/RET@h/x`, `https://u:SEC?RET@h/x` and `https://u:SEC#RET@h/x` through curl and assert no request or plain environment TOML contains `SEC`, `RET` or the password. Exception (see Decisions, 2026-11-14): a first authority segment `name:<digits>` (non-empty, all digits) with a later `@` is taken as host:port and the URL is kept, and the import adds a warning that names no part of the URL or userinfo, saying the URL may contain credentials and should be checked before committing (test: `http://localhost:8080/users/me@example.com` imports unchanged with that warning). Only clearly broken forms are refused, with an error naming no userinfo: an empty port (`https://u:/SECPW@h/x`) and a digit run cut by `?` or `#` (`https://u:12?34@h/x`); tests assert neither writes `SECPW`, `12` or `34` as userinfo to any TOML.
- [ ] AC3 (99-AC44) `https:///u:SECRETVAL@h/x` and `http:/u:SECRETVAL@h/x` have their userinfo stripped into secrets (or are refused with a warning). A test with each asserts no request or plain environment TOML contains `SECRETVAL`.
- [ ] AC4 (99-AC18) curl `--digest -u user:SECRETVAL URL` imports `Auth::Digest` with both fields as secret refs. `--ntlm` or `--negotiate` with `-u` imports no auth, keeps both credentials in the secrets environment, and adds a warning that names the option and no value. Tested for all three options; no TOML other than the secrets file contains `SECRETVAL`.
- [ ] AC5 Existing import behaviour is otherwise unchanged: every existing test in crates/storage/src/import/** and crates/storage/tests/import.rs passes. Where AC1 changes an expected value, the test is updated and the PR description names it.

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
- crates/storage/src/import/openapi.rs, postman.rs and write.rs are not edited. They call `split_userinfo` and `secret_ref` and pick up the fix. If one of their inline tests fails because of it, stop and report.
- 99-AC19, AC20, AC42, AC43 (curl warning text, test-only and unknown-option items), and every F, G and other H item from task 99 (deferred).
- Making `import::write` transactional.

## Decisions
- 2026-11-14 (user decision, after review round 1 of PR #60) — A URL whose first authority segment is `name:<digits>`, the digit part non-empty, followed later by `@` (e.g. `http://localhost:8080/users/me@example.com`, indistinguishable from `https://u:1234/abcd@h/x`) is treated as host:port and kept; the import adds a warning naming no part of the URL or userinfo, saying the URL may contain credentials and should be checked before committing. Only clearly broken forms are refused, with an error naming no userinfo: an empty port (`u:/SECPW@h`) and a digit run cut by `?` or `#` (`u:12?34@h`). Reason: refusing every port+`@` URL blocked legitimate imports; the residual case (a password of only digits before its first raw `/`) is accepted and surfaced by the warning.

## Review round 1 — findings (HEAD 605f9e8)
- [P2] crates/storage/src/import/mod.rs:296-299 — the fallback guard `looks_like_userinfo` treats a segment as `host:port` whenever the part after the first `:` is all digits, including empty, so when a password's first raw `/`, `?`, `#` follows only digits or comes right after the colon, userinfo stays in the URL and is written to the request TOML (breaks AC2: no password in any written TOML, or the URL is refused with a warning). Fix: when a later `@` exists but the segment looks like host:port, refuse the URL with a warning that names no part of the userinfo, or handle the empty-port case. Inputs: `curl 'https://u:1234/abcd@h/x'` → url keeps the password; `curl 'https://u:/SECPW@h/x'` → url keeps it; `curl 'https://u:12?34@h/x'` → url `https://u:12` and `34@h/x` into query params.
  - Settled by Decisions 2026-11-14: `u:1234/abcd@h` is kept with the may-contain-credentials warning; `u:/SECPW@h` and `u:12?34@h` are refused. AC2 amended.
- [P2] crates/storage/src/import/mod.rs:293-310 — the same fallback treats an IPv6 literal host as userinfo when the path has an `@`; the real host goes into secrets as a username and the request is rewritten to a different host (breaks AC5, existing behaviour unchanged). Fix: a segment starting with `[` is never userinfo. Input: `curl 'http://[::1]:8080/a@b'` → url `http://b`, Basic auth with `{{get_username}}`.
- [P3, follow-up] curl.rs:349-351 — `--basic`/`--anyauth` unknown, so `--digest --basic -u u:p` imports Digest though curl's last option wins.
