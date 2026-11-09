# 95 — Add Digest and AWS SigV4 auth

**Repo:** .
**Depends on:** 91
**Owns:**
- crates/core/src/auth/** (new: `mod.rs`, `digest.rs`, `sigv4.rs`)
- crates/core/src/lib.rs
- crates/core/src/request.rs
- crates/core/src/model.rs
- crates/core/src/engine.rs
- crates/core/src/resolve/engine.rs, crates/core/src/resolve/tests.rs
- crates/core/tests/** (field insertions in existing files; new `auth.rs`)
- crates/storage/src/codec.rs
- crates/app/src/ui/request/auth.rs

## Goal
Requests can use HTTP Digest (RFC 7616) and AWS Signature Version 4, hand-rolled
on `sha2`, `hmac` and `md-5`. Both need the final request, so `resolve()` stays
pure and only resolves their fields; the Engine applies them. The auth editor
offers both types, and every new secret-bearing type redacts in Debug.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#3-auth-core`
- `crates/core/src/request.rs:34-75` — `Auth` and its hand-written Debug
- `crates/core/src/resolve/engine.rs:455-500` — where auth is applied in `resolve`
- `crates/core/src/model.rs:46-110` — `PreparedRequest` and its redacting Debug
- `crates/core/src/engine.rs:546-720` — `perform`; `stream_body`/`gather` decide streamed vs in-memory bodies
- `crates/storage/src/codec.rs:365-430` — `read_auth`/`write_auth` arms
- `crates/app/src/ui/request/auth.rs:21-60,160-180,255-330` — `AuthKind`, `KINDS`, field editors
- `docs/conventions.md#logging` — Debug redaction and `debug_redacts_*` tests (both `{:?}` and `{:#?}`)
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/95-digest-sigv4`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-95`

## Contract
- `Auth::Digest { username, password }` and `Auth::AwsSigV4 { access_key, secret_key, session_token, region, service }` (all `String`; empty `session_token` means none). TOML: `type = "digest"` and `type = "awssigv4"` with those key names.
- `wisp_core::auth::EngineAuth` — resolved values: `Digest { username, password }` and `AwsSigV4 { .. }` as above. `PreparedRequest` gains `pub auth: Option<EngineAuth>` (serde, `#[serde(default)]`). Wave 2 adds OAuth2 variants here, so keep it an enum with one arm per scheme.
- `resolve` expands `{{vars}}` in the new fields (problems reported at the existing `Location::Auth`; no new `Location` or `ErrorKind` variants, because the app matches both exhaustively), adds no header for them, and sets `prepared.auth`.
- `Auth::redacted(&self) -> Auth` — for history (task 93): a field whose value is exactly one `{{var}}` reference (whitespace allowed inside the braces) is kept; any other value becomes `""`. Covers every variant, including Basic, Bearer and ApiKey.
- Digest in the Engine: send; on a 401 with a `WWW-Authenticate: Digest` challenge, compute the response and retry once. The first 401 emits no events. Algorithms MD5, SHA-256, SHA-512-256 and their `-sess` forms; `qop=auth` (choose it when offered; `auth-int` alone is an error); `userhash=true`; `nc=00000001`; random 16-byte hex cnonce. A Digest request whose body is `File` or contains a File part fails before sending with `ErrorKind::Body` and the message "Digest auth needs a replayable body; File bodies cannot be resent". A second 401 is returned normally.
- SigV4 in the Engine, just before send: adds `x-amz-date`, `x-amz-security-token` (when set), `x-amz-content-sha256` (service `s3` only) and `Authorization`. Signed headers are `host` plus every header of the prepared request plus those added. Payload hash is SHA-256 of an in-memory body, `UNSIGNED-PAYLOAD` for a streamed body.
- Signing is pure and testable: `sigv4::sign(method, url, headers, payload_hash, creds, time)` and `digest::respond(challenge, method, uri, creds, cnonce)` take the time and cnonce as arguments. UTC date formatting is hand-written (no new crate).
- Hand-written Debug prints `<redacted>` for `password`, `secret_key`, `session_token` and `access_key` in `Auth`, `EngineAuth` and through `PreparedRequest`.
- Auth editor: two new kinds, labels "Digest" and "AWS Signature", with the existing masked-field and reveal pattern for password, secret key and session token; region and service plain.

## Acceptance
- [ ] AC1 Digest unit tests reproduce the RFC 7616 §3.9.1 MD5 and SHA-256 responses and the §3.9.2 SHA-512-256 `userhash` example exactly (vectors copied from the RFC, cited in a comment).
- [ ] AC2 SigV4 unit tests reproduce at least 5 cases of the AWS SigV4 test suite (`get-vanilla`, `get-vanilla-query-order-key-case`, `post-vanilla`, `post-x-www-form-urlencoded`, `get-header-value-trim`) — canonical request, string to sign and Authorization header — plus one case with a session token. Vectors are copied from the published suite and cited; if the implementer cannot obtain them, stop and report rather than inventing them.
- [ ] AC3 Wiremock tests: a Digest server answers 401 then 200 and the client succeeds with exactly two requests; a server answering 401 twice yields status 401 after two requests; a Digest request with a File body fails with `ErrorKind::Body` and makes no request.
- [ ] AC4 Wiremock test: a SigV4 request arrives with `Authorization` starting `AWS4-HMAC-SHA256 Credential=<key>/<date>/<region>/<service>/aws4_request` and `x-amz-date`; a File body arrives signed with `UNSIGNED-PAYLOAD`.
- [ ] AC5 `debug_redacts_digest_auth`, `debug_redacts_aws_sigv4_auth`, `debug_redacts_engine_auth` and `debug_redacts_prepared_request_engine_auth` check `{:?}` and `{:#?}` contain no secret value.
- [ ] AC6 `Auth::redacted` tests: `{{pw}}` kept, `" {{ pw }} "` kept, `"abc"`, `"{{a}}x"` and `"{{a}}{{b}}"` emptied, for each variant with secret fields.
- [ ] AC7 Codec round-trips both new types through `read_auth`/`write_auth`, and an unknown `type` still errors as before.
- [ ] AC8 `resolve` puts a Digest or SigV4 auth with an unresolved var into problems at `Location::Auth` and adds no `Authorization` header.
- [ ] AC9 The auth editor's kind list includes Digest and AWS Signature, and switching kinds round-trips the draft (unit test on the non-GPUI helpers in `auth.rs`).
- [ ] AC10 `fmt`, `clippy -D warnings` and the whole workspace test suite pass.

## Checks
- `auth-unit` command — AC1, AC2, AC5, AC6, AC8. cargo test writes no JUnit XML here; the command asserts at least 20 passing.
  run: `n=$(cargo test -p wisp-core --lib -- auth:: debug_redacts redacted resolve::tests::auth 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 20`
- `auth-engine` command — AC3, AC4 (at least 5 passing)
  run: `n=$(cargo test -p wisp-core --test auth 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 5`
- `codec` command — AC7
  run: `cargo test -p wisp-storage --lib codec::`
- `app-auth` command — AC9
  run: `cargo test -p wisp-app ui::request::auth`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `vectors` read — AC1, AC2
  how: each vector cites its source section or suite case name; expected values are literals, not computed by the code under test.

## Out of scope
- OAuth2 variants and `Command::AuthorizeStart` (wave 2); leave `EngineAuth` open for them.
- `auth-int`, Digest on proxies (407), SigV4 chunked/streaming signatures, SigV4A.
- Postman/OpenAPI mapping of these auth types (task 94 emits a warning for them).
- History redaction call-sites (task 93).

## Notes
- Size: estimated +15-30 KB. Unconfirmed: whether `sha2 0.10`, `hmac` and `md-5` are already linked into the release binary (the lock also has `sha2 0.11`). Measured by task 98.
- If you launch the app to look at the editor, set `WISP_DATA_DIR` to a temp dir, record the pid you started and stop only that pid. Never `pkill`/`killall`; a bundled build uses bundle id `io.github.aindriub.wisp.t95`.
