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
- [ ] AC1 The file is opened once; the same handle (rewound) supplies the body that is sent, and the bytes are re-hashed as they stream (AC6). No path-based reopen remains between hash and send.
- [ ] AC2 Test: the body file is replaced on disk (new inode, rename over the path) between hash and send, via a test hook; the bytes the server receives hash to the signed value, or the send fails with `BodyFile`. It never sends different bytes under the old hash. The test fails when the one-handle fix is removed (a path reopen before send); the implementer confirms this once and says so in the report.
- [ ] AC3 Test: a cancel during hashing of a large file ends the request with the cancelled outcome, promptly (bounded by the test's own receive timeout, no wall-clock assert on speed).
- [ ] AC4 One `timeout` budget covers the whole request: hashing plus sending, not each phase. Test: a request timeout shorter than the hashing time ends in `Timeout`; and a timeout longer than hashing but shorter than hashing plus the server's response delay also ends in `Timeout`.
- [ ] AC5 The 164 delivery tests stay green; no file outside Owns changes; fmt, clippy and the workspace suite pass.
- [ ] AC6 The body is re-hashed while it streams, on both the streamed and the gathered paths in `engine.rs`; if the streamed bytes' hash differs from the signed hash, the send fails with a `BodyFile`-kind error whose message is fixed text (no body content, no hash values), raised before or as the body ends. Test: the file is overwritten in place (same inode, same length) between hash and send via a test hook, on each path; the send fails with that error and the server never receives a complete request with the other bytes.
- [ ] AC7 `Arc` and `AtomicBool` are used through `use` imports, not fully qualified paths; no commit on the branch has a message containing "PARTIAL" (squash it).

## Checks
- `hash-file` command — AC2, AC3, AC4, AC6
  run: `cargo test -p wisp-core --locked hash_file 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([6-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `hash_file` in each new test name.)
- `delivery-core` command — AC5
  run: `cargo test -p wisp-core --locked --test engine nothing_sent 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/core/src/engine.rs$' -e '^crates/core/tests/engine.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `one-handle` read — AC1, AC6
  how: the reviewer traces the file body from open through hash to `rb.send()` and finds no second open by path; on both the streamed and gathered paths the sent bytes pass through a hasher compared against the signed hash, and the mismatch error carries fixed text only.
- `swap-guard` read — AC2
  how: the reviewer confirms the swap test's hook fires between hash and send and that the implementer's report states the test failed with the fix removed.
- `single-budget` read — AC4
  how: the reviewer finds one deadline computed once from `timeout` and applied to both hashing and sending.
- `no-partial` command — AC7
  run: `test "$(git log main..HEAD --format=%B | grep -c PARTIAL)" = 0`
- `imports` read — AC7
  how: no `std::sync::Arc` or `std::sync::atomic::AtomicBool` path appears inline in the diff.

## Out of scope
- Any other engine change; redirects and `nothing_sent` (164).

## Wave stopped — step 2 — orchestrator-routed review findings at 1b7c06e
- [P2] crates/core/src/engine.rs:665: rewinding the shared handle cannot stop an in-place overwrite (same inode, same length) between hash and send from going out under the old SigV4 hash; AC2 / task 99 AC2 require it never happens or the send fails. Needs a design choice: capture the bytes, or re-hash while streaming and fail on mismatch.
- [P3] engine.rs:843: hashing gets its own full `timeout` and the send another; whether `timeout` covers the whole request or each phase is unspecified.
In-spec findings held until the decision: [P2] the swap test (tests/engine.rs:1637) passes with the fix removed, so it does not guard AC2; [STYLE] fully qualified `std::sync::Arc`/`AtomicBool` at :616, :631, :692, :693; the first commit message ends "PARTIAL: tests being run".

## Amendment 1 (user, 2026-11-17)
Reason: the review at 1b7c06e (see `## Wave stopped` above) found that rewinding one handle does not stop an in-place overwrite (same inode, same length) between hash and send, and that the timeout scope was unspecified. User decisions: (1) re-hash the body while streaming and fail the send with a fixed-text `BodyFile` error on mismatch, on both streamed and gathered paths (new AC6; AC1 updated); (2) one `timeout` budget covers hashing plus sending (AC4 updated). The held in-spec findings are now criteria: the swap test must guard AC2 (AC2 updated), short imports for `Arc`/`AtomicBool` and no "PARTIAL" commit (new AC7). Checks updated to match; `hash_file` min-passing raised to 6.

## Wave resumed — user accepted the orchestrator recommendations (Amendment 1)

## Review round 1 — findings
Reviewed HEAD 1b7c06e9437605865ed598484e205a2492755e06 (full branch, after Amendment 1).
- [P2] crates/core/src/engine.rs:665 (hash_file, rewind before send). Sent bytes are never re-hashed, on either the streamed or the gathered path. Nothing compares them against the signed hash, and no fixed-text `BodyFile` mismatch error exists.
  route: implementer. Required by AC6 and AC1 as amended.
  input: a SigV4 non-s3 POST of a file body. After hashing finishes, overwrite the file in place (same inode, same length) before `rb.send()`. The server receives the new bytes under the old `x-amz-content-sha256`, and the request "succeeds".
- [P2] crates/core/src/engine.rs:840-848. Hashing gets its own `tokio::time::timeout(t, …)`, and the send then applies the full `timeout` again. That is a budget per phase, not one deadline.
  route: implementer. Required by AC4 as amended.
  input: `timeout` = 2s, hashing takes about 1.5s, the server delays its response by 1s. Total time is about 2.5s, but the request succeeds instead of ending in `Timeout`. No test covers AC4's second case (timeout longer than hashing but shorter than hashing plus response delay).
- [P2] crates/core/tests/engine.rs (`hash_file_swapped_mid_hash_never_sends_other_bytes_under_the_old_hash`, diff +35). The hook swaps the file mid-hash (rename over the path). The open handle never observes that swap, so the test passes with or without the one-handle fix and does not guard AC2. There is also no in-place overwrite test on either path (AC6).
  route: implementer. Required by AC2 and AC6 as amended.
  input: reintroduce a path reopen before send. The swap test still passes.
- [P2] crates/core/tests/engine.rs. The branch adds only 3 `hash_file` tests (swap, cancel, timeout shorter than hashing). The amended Check `hash-file` needs at least 6 passing tests, so it fails as written.
  route: implementer. Required by the Checks section of Amendment 1.
  input: `cargo test -p wisp-core --locked hash_file` reports 3 passed, and the grep needs 6 or more.
- [STYLE] crates/core/src/engine.rs:616, :631, :692, :693. `std::sync::Arc` and `std::sync::atomic::AtomicBool` are written as fully qualified paths inline.
  route: implementer. rule: AC7.
- [STYLE] commit a329025. The message ends "PARTIAL: tests being run".
  route: implementer. Squash or reword it (AC7 `no-partial` check). rule: AC7.

## Attempt 2 — failed
/verify at 5d4529055a030abf20d4e6a3ac8ec73cc6e11c53: reviewer APPROVE, tester FAIL.
- Check `hash-file`: 6 passed, 1 failed (needs 0 failed). Deterministic: failed on the first run and on a solo rerun.
- `hash_file_timeout_covers_hashing_plus_the_response` (crates/core/tests/engine.rs:1794): `unexpected terminal Completed { total: 1.36s ... }`; expected `Failed` with `ErrorKind::Timeout`. Timeout 1000 ms, after-hash hook 600 ms, server delay 700 ms — the request completed after the budget, so AC4's single budget is not enforced on this path (or the test's timing does not exercise it as intended). Note: the code-reviewer's scratch run and the implementer both saw it pass, so check for a load- or ordering-dependent cause.
- suite-and-lint not run (stopped after the failed check).

## Wave stopped — step 3 — new flake at 5d45290: `hash_file_timeout_covers_hashing_plus_the_response`
The tester failed it twice (first run and a solo rerun): `Completed { total: 1.36s }` under a 1 s budget. The attempt-2 implementer made no code change; its first run failed the same way (≈1.38 s), then 8 of 8 runs passed. It suspects a stale binary in target/wt-167 (unconfirmed); a debug print showed the send's remaining budget ≈390 ms, which should have timed out against the 700 ms delay. Changes result on rerun → flake, not in known-flakes.md. It may also be a real path where the single budget is not applied. Needs the user's call.

## Wave resumed — user: one fresh run under systematic-debugging to prove whether a path skips the single timeout budget (fix it if so; otherwise make the test deterministic and say why it failed)

## Wave verified — 39b5457b69c4d5630a26f2ed055dee05362474f0


## Outcome
Merged to main at 39b5457 (merge dbe1f1a), PASS + APPROVE: tester 1412 passed, `hash-file` 7/7 twice; reviewer APPROVE on AC1-AC7. The Attempt 2 failure (`hash_file_timeout_covers_hashing_plus_the_response`, `Completed` at 1.36 s under a 1 s budget) was a late timer, not a missed budget: the engine runs one worker thread, and under load its timer fires late. 39b5457 enforces the single budget on the response with a post-send deadline check. The local task branch was squashed; `origin/task/167-hash-file-one-handle` still holds the older history with a "PARTIAL" commit and was not pushed.
Follow-ups, not fixed (also in PLAN.md): a same-origin 307/308 SigV4 resend reopens the file by path with `digest: None`, so it is not re-hash-checked (redirect area, 164); the post-send deadline check covers the response head, not the body read (`engine.rs:1176-1194`); a head arriving past the deadline reports Timeout with delivery `unknown` (exit 6) where `responded` would be accurate; no test proves the post-send check is needed (needs a stalled runtime); `sparse_file` 4 GiB tests may reserve real disk on NTFS; `StopOnDrop` has no test of its own.
