# 86 — Resend streamed File and Multipart bodies on 307/308 redirects

**Repo:** .
**Depends on:** none
**Owns:**
- crates/core/src/engine.rs
- crates/core/tests/engine.rs
- docs/plan/tasks/86-redirect-resend.md (the `## Notes` section only)

Branch `task/86-redirect-resend`, worktree `.worktrees/wisp/86-redirect-resend`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-86`. Commit on the
branch; do not push.

## Goal
A File or Multipart body over 1 MiB is streamed. reqwest cannot clone a stream,
so on a 307 or 308 it does not resend the body: it returns the 3xx, which Wisp
shows as the result (PLAN.md, **Residuals (57)**, first sentence). Bodies at or
under 1 MiB are already resent from memory. With follow-redirects on, the engine
must follow a 307/308 itself for a streamed body, re-reading the files from
disk, or fail with a clear error when that cannot be done safely.

## Context
- crates/core/src/engine.rs:281-282 (`MEMORY_BODY_LIMIT`), :287-310 (`Segment`: files are opened and measured up front), :374-418 (`stream_body`), :421-434 (`attach_segments`), :500-600 (the send path: headers at :515-534, body at :539-554, then `send`, `Event::Headers` and the body read).
- crates/core/src/engine.rs:97-99 — with `follow_redirects` off, reqwest follows nothing; that must not change.
- crates/core/tests/engine.rs:495-545 — `pattern`, `post_with`, `temp_file`, `a_small_file_body_is_resent_after_a_307_redirect`, `assert_streamed`: the helpers and shape to follow.

## Contract
This applies only when `client.follow_redirects` is true, the body was streamed (total > `MEMORY_BODY_LIMIT`), and the response is 307 or 308 with a `Location` header.

| Condition | Engine does |
|---|---|
| `Location` resolves (relative to the current URL) to http/https, and hops so far < 10 | Re-open every file segment. If each file's length still matches the first send, resend the same method, headers and body to the new URL with the same exact `Content-Length`. |
| new URL's origin (scheme, host, port) differs from the current one | as above, but drop `Authorization` and `Cookie` (reqwest's own cross-origin rule) |
| a file's length changed or it cannot be reopened | `Event::Failed` with `ErrorKind::BodyFile` and a message naming the path and "changed before the redirect resend" |
| `Location` missing, unparsable or not http/https | the 3xx is the result, as today |
| the 10th 307/308 hop for one request | `Event::Failed` with `ErrorKind::Other`, message "too many redirects (10)" |

- Only the final response emits `Event::Headers`, and the body read follows it. Intermediate 3xx responses are not shown.
- The request timeout covers the whole exchange, resends included. Cancel during a resend gives `Cancelled`, the same as today.
- 301/302/303 behaviour does not change (Residuals (57) PUT/PATCH note: out of scope).

## Acceptance
- [ ] AC1 The tests below exist in crates/core/tests/engine.rs. Each uses wiremock and a body file of at least 2 MiB.
  - `a_streamed_file_body_is_resent_after_a_307_redirect`: the second request has the full bytes and an exact `content-length` (`assert_streamed`).
  - `a_streamed_file_body_is_resent_after_a_308_redirect`
  - `a_streamed_multipart_body_is_resent_after_a_307_redirect`: both parts arrive intact.
  - `a_cross_origin_redirect_drops_authorization_on_resend`: two MockServers, so the ports differ.
  - `a_same_origin_redirect_keeps_authorization_on_resend`
  - `a_streamed_redirect_loop_fails_after_10_redirects`
  - `a_body_file_that_shrinks_before_a_redirect_resend_fails_with_body_file`: a custom `Respond` truncates the file before answering 307.
  - `follow_redirects_false_surfaces_a_307_for_a_streamed_body`
- [ ] AC2 The existing engine tests pass unchanged, including `a_small_file_body_is_resent_after_a_307_redirect` and the 20 MiB streaming tests.
- [ ] AC3 The body is still streamed on resend: no code path reads a file segment over `MEMORY_BODY_LIMIT` fully into memory.
- [ ] AC4 No new dependency; the `Cargo.toml` files and `Cargo.lock` are unchanged.
- [ ] AC5 Every temp file a new test creates is removed even when an assertion fails (a guard type or similar). This is the "temp file left behind" note in **Engine tests and limits (16)**, for the new tests only.
- [ ] AC6 `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Checks
- `redirect-tests` command — AC1. cargo test does not write JUnit XML here, so the command asserts the pass count itself (≥ 10: 8 new tests plus the 2 existing ones with "redirect" in their name).
  run: `cargo test -p wisp-core --test engine redirect 2>&1 | grep -E '^test result: ok\. ([1-9][0-9]+) passed'`
- `engine-tests` command — AC2
  run: `cargo test -p wisp-core --test engine`
- `no-new-deps` command — AC4
  run: `git diff --exit-code main...HEAD -- Cargo.toml Cargo.lock crates/core/Cargo.toml`
- `lint` command — AC6
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `streaming` read — AC3, AC5
  how: the resend path builds its body with `stream_body` from freshly opened segments, and the new tests clean up their files through a guard.

## Out of scope
- Resending PUT/PATCH bodies on 301/302; special files; the 64 KiB chunk-limit test; `block_in_place` per chunk; memory measurement of the 20 MiB tests (the other Residuals (57) items).
- Any change to `EngineConfig`, the model types (task 84 owns model.rs) or the client cache.
- Following redirects for in-memory bodies: reqwest already does it.

## Notes

## Amendment — orchestrator (2026-11-09)
The Contract row for a cross-origin resend lists `Authorization` and `Cookie`, but its note says to follow reqwest's own cross-origin rule, which is the intent. reqwest 0.13.5 (redirect.rs:241-249) removes `Authorization`, `Cookie`, `Cookie2`, `Proxy-Authorization` and `WWW-Authenticate`; the streamed resend must remove the same set.

## Review round 1 — findings
- [P2] crates/core/src/engine.rs:661-663 — cross-origin resend removes only Authorization and Cookie; also remove Cookie2, Proxy-Authorization and WWW-Authenticate (same list as reqwest 0.13.5 redirect.rs:241-249). Extend `a_cross_origin_redirect_drops_authorization_on_resend` to set Proxy-Authorization (and Cookie2) and assert the second origin receives neither.
