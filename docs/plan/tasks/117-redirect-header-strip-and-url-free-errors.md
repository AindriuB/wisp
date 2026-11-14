# 117 — Drop credential headers on cross-origin redirects and keep URLs out of engine errors

**Repo:** `.`
**Depends on:** none
**Size impact:** small (expected well under 20 KB on the Windows binary; no new dependency)
**Owns:**
- crates/core/src/engine.rs (including its inline `#[cfg(test)]` module)
- crates/core/src/client.rs (only if the redirect decision needs a client-settings change)
- crates/core/tests/engine.rs

## Goal
A redirect to another origin must not carry the user's credentials. Today a cross-origin hop keeps user-set `x-amz-*`, `Host` and custom headers such as `X-Api-Key`, because the engine strips only five header names and reqwest, when it follows redirects itself, strips only the same few. Engine errors must also stop echoing the request URL, which can hold a secret resolved from the environment. Carried from task 99 AC1, AC40, AC41.

## Context
- crates/core/src/engine.rs:714-722 decides `own_redirects`: the engine follows redirects itself only for engine auth or a user `Cookie` header. Otherwise reqwest follows them and keeps every custom header across origins.
- crates/core/src/engine.rs:995-1011 is the cross-origin strip, a deny-list of five names.
- crates/core/src/engine.rs:1061-1073 `classify` builds the message from `e.to_string()`, which embeds the URL. crates/core/src/auth/oauth2/token.rs:155 (`clone_without_url`) is the existing pattern for removing it.
- crates/core/tests/engine.rs:821-895 (`auth_resend`, the cross-origin and same-origin tests) are the shape to follow for two wiremock origins.
- docs/conventions.md#tests: wiremock on localhost only; test names read as sentences; no sleeps.

## Acceptance
- [ ] AC1 (99-AC1) A request carrying user headers `X-Amz-Date`, `X-Amz-Security-Token` and `Host` that is redirected from origin A to origin B sends none of them to B. The same request redirected within origin A sends all three on the second hop. Shown by tests whose names contain `cross_origin` and `same_origin`.
- [ ] AC2 (99-AC40) On a cross-origin hop, every user-set header is dropped except a fixed allow-list of non-credential headers, declared once in `engine.rs` as a named constant. The list holds at least `Accept`, `Accept-Encoding`, `Accept-Language`, `Content-Type` and `User-Agent`, and it holds no name for which `wisp_core::redact::is_credential_name` is true. The allow-list applies whether the redirect is followed by the engine or would have been followed by reqwest: a request with no auth, no `Cookie`, and a user header outside the allow-list is followed by the engine.
- [ ] AC3 (99-AC40) A test sends `X-Api-Key: SECRETVAL` and `Accept: text/plain` to origin A, which 302-redirects to origin B. B receives `Accept` and no `X-Api-Key`. With a same-origin redirect, the second hop still receives `X-Api-Key: SECRETVAL`.
- [ ] AC4 (99-AC41) Every `reqwest::Error` reaches an `EngineError` with its URL removed (`without_url()` or an equivalent). A test sends to `http://127.0.0.1:<closed port>/x?api_key=SECRETVAL` and asserts that neither the `Display` nor the `Debug` of the resulting error contains `SECRETVAL` or `api_key`. The error kind is unchanged: a refused connection is still classified as it is today.
- [ ] AC5 Existing redirect behaviour is unchanged except as set by Decisions 2026-11-14: method rewriting on 301/302/303 matches reqwest's rules for every redirect the engine follows (see Decisions), streamed resend on 307/308, the 10-redirect limit, and `follow_redirects = false` surfacing the 3xx. The existing tests in crates/core/tests/engine.rs pass unmodified, except the `X-Keep` assertion in `a_cross_origin_redirect_drops_authorization_on_resend`, whose change is required by AC2/AC3.
  - 2026-11-14 correction (orchestrator): "reqwest's rules" means: on 301/302 only POST becomes GET with no body (`Content-Type`/`Content-Length` dropped), and every other method, including PUT/PATCH/DELETE/OPTIONS, keeps its method and body; on 303 every method except HEAD becomes GET with no body; 307/308 keep method and body. This applies to every redirect the engine follows itself. See the correction under Decisions 2026-11-14.

## Checks
cargo writes no JUnit XML here, so these are command checks. The test filter lines must show the counts stated.
- `redirect-tests` command — AC1, AC2, AC3, AC5
  run: `cargo test -p wisp-core --test engine -- origin`
  pass: exit 0, and the `test result` line shows at least 4 passed (the two existing `*_origin_*` tests plus at least the new AC1 and AC3 tests) and 0 failed.
- `error-url` command — AC4
  run: `cargo test -p wisp-core --test engine -- without_url_or_secret` (name the AC4 test so it contains `without_url_or_secret`)
  pass: exit 0 and at least 1 passed.
- `core-suite` command — AC5
  run: `cargo test -p wisp-core`
- `lint` command — all
  run: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `allow-list` read — AC2
  how: one named constant holds the allow-list; the cross-origin branch keeps only names in it (case-insensitive); the `own_redirects` condition also covers user headers outside it; no allow-listed name matches `is_credential_name`.

## Standing constraints
- Work in a worktree made by `./.claude/scripts/wt-new.sh . 117 redirect-header-strip`, with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-117`.
- Secrets never logged or in docs; tests use only the marker `SECRETVAL`.
- Tests never touch the real OS keychain; none is needed here.
- Never run `pkill` or `killall`; stop only a pid you started.
- Any test that needs a data dir uses a temp `WISP_DATA_DIR`.
- No Windows-only code is expected. If any is added, it must be `cfg`-free or covered by a test that also runs on unix; the Windows CI job (54-CI) lands after this task.

## Out of scope
- 99-AC2 (`hash_file` reopen and cancel during hashing) and the same-size file resend follow-up.
- crates/core/src/auth/sigv4.rs and crates/core/tests/auth.rs: SigV4 signing itself is unchanged.
- 99-AC3 (masking the SigV4 access key field in the app).
- Error message wording other than removing the URL.

## Decisions
- 2026-11-14 (user decision, code review round 1 of PR #59): every redirect the engine follows itself uses the same method rules as reqwest, `curl -L` and browsers. On 301 and 302 a method other than GET/HEAD becomes GET with no body, and body headers such as `Content-Type`/`Content-Length` are dropped. On 303 every method except HEAD becomes GET with no body. 307 and 308 keep the method and resend the body. This applies whatever made the engine follow the redirect (auth, a user `Cookie`, or a non-allow-listed header), so behaviour no longer depends on which headers or auth a request has. It changes the previous RFC-strict behaviour on the auth path. AC5 is amended to match, and the earlier change to the `X-Keep` assertion in `a_cross_origin_redirect_drops_authorization_on_resend` is accepted as required by AC2/AC3.
  - 2026-11-14 correction (orchestrator): the user's decision was "match reqwest/`curl -L`". The entry above mis-states those rules. The actual behaviour of reqwest 0.13.5 (via tower-http `follow_redirect`), `curl -L` and browsers is this. On 301 and 302 only POST becomes GET with no body, and body headers (`Content-Type`/`Content-Length`) are dropped; every other method, including PUT, PATCH, DELETE and OPTIONS, keeps its method and body. On 303 every method except HEAD becomes GET with no body. 307 and 308 keep the method and body. This applies to every redirect the engine follows itself. Where the entry above disagrees, this note governs.

## Review round 1 — findings (HEAD 9801aa3)
- [P1] crates/core/src/engine.rs:722 (with :796-800, :985) — once any non-allow-listed user header is present the engine follows redirects itself, but `replay` is built only when `req.auth.is_some()`; with no auth it stays Replay::None, so on 307/308 the resend has an empty body and still reports Completed. Small file bodies (<= MEMORY_BODY_LIMIT, plan None) lose their body too. Breaks AC5. Fix: build replay whenever own_redirects is true. Input: POST Bytes "hello" + header X-Trace: 1, auth None, follow_redirects, 307 /first → /second → /second gets 0-byte body; same for a 100,000-byte File body.
- [P2] crates/core/src/engine.rs:960-963 — the new trigger moves 301/302 of PUT/PATCH/DELETE from reqwest's rules to the engine's, which keeps method+body; one custom header changes the method on the second hop. Settled by the Decisions entry above. Input: PUT Bytes + X-Trace, no auth, 302 → before GET /second no body; now PUT /second empty body.

## PR #59 review round 1 (HEAD 4b7a26c)
- 59-1 (Medium, security) crates/core/src/engine.rs:725 — redirects reqwest still follows send a Referer carrying the previous URL and query to the new origin. Fix: `.referer(false)` in ClientCache; cross-origin test asserts no Referer.
- 59-2 (Medium) crates/core/src/engine.rs:997 — POST→GET rewrite keeps a user-set Transfer-Encoding header. Fix: strip it with content-type/content-length when the body is dropped.
- 59-3 (Low, follow-up) engine.rs:1078 URL removal parses reqwest Display text.
- 59-4 (Low, follow-up) engine.rs:795 replay clones the whole body for any custom header.
- 59-5 (Nit, follow-up) engine.rs:958 comment wording.
