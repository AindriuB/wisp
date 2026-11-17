# 167 — Hash and send a file body through one handle, honouring cancel and timeout (2B, H7)

**Repo:** .
**Round:** 0
**Depends on:** 164, 165
**Owns:**
- crates/core/src/engine.rs
- crates/core/tests/engine.rs

## Goal
Close task 99 AC2: a file body that is hashed (SigV4 or Digest) is hashed and
sent from one open handle, so a file swapped between the two cannot be sent
under the old hash, and hashing stops on cancel and timeout.

## Context
- docs/plan/tasks/99-hardening-0-2-0.md:36 AC2 (its `engine.rs:574` is now `:612`).
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" table H7 (:423), Waves 2B (:468).
- crates/core/src/engine.rs:612 `hash_file`; the pending body and `attach_segments` around :895-900; the single `rb.send()` and the per-send `nothing_sent` state added by 164 (keep: an unreadable body file stays `BodyFile` with `nothing_sent = true`).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 167 hash-file-one-handle`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-167`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 The file is opened once; the same handle (rewound, or its bytes captured while hashing) supplies the body that is sent. No path-based reopen remains between hash and send.
- [ ] AC2 Test: the body file is replaced on disk after hashing starts (a test hook or a file large enough to replace mid-hash); the bytes the server receives hash to the signed value, or the send fails with `BodyFile`. It never sends different bytes under the old hash.
- [ ] AC3 Test: a cancel during hashing of a large file ends the request with the cancelled outcome, promptly (bounded by the test's own receive timeout, no wall-clock assert on speed).
- [ ] AC4 Test: a request timeout shorter than the hashing time ends in `Timeout`.
- [ ] AC5 The 164 delivery tests stay green; no file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `hash-file` command — AC2, AC3, AC4
  run: `cargo test -p wisp-core --locked hash_file 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `hash_file` in each new test name.)
- `delivery-core` command — AC5
  run: `cargo test -p wisp-core --locked --test engine nothing_sent 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/core/src/engine.rs$' -e '^crates/core/tests/engine.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `one-handle` read — AC1
  how: the reviewer traces the file body from open through hash to `rb.send()` and finds no second open by path.

## Out of scope
- Any other engine change; redirects and `nothing_sent` (164).
