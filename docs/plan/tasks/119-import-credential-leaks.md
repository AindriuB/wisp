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
- [ ] AC2 (99-AC31) A URL whose password holds a raw `/`, `?` or `#` keeps no password in any written TOML: the authority is taken to end at the last `@` before the first `/` that follows that `@`, or the URL is refused with a warning that names no part of the userinfo. Tests import `https://u:SEC/RET@h/x`, `https://u:SEC?RET@h/x` and `https://u:SEC#RET@h/x` through curl and assert no request or plain environment TOML contains `SEC`, `RET` or the password.
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
