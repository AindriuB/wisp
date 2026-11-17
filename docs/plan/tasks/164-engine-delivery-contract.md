# 164 — Engine delivery contract: nothing_sent, engine-followed redirects, no reqwest retries (1E, E4 core)

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/core/src/engine.rs
- crates/core/src/client.rs
- crates/core/src/model.rs
- crates/core/tests/engine.rs

## Goal
The engine reports, on every failure, whether it can prove that no byte of the
request reached any connection (`nothing_sent`), so `run` can tell "not sent"
from "outcome unknown" and never claim a retry is safe when it is not. To make
that provable the engine follows all redirects itself and reqwest never resends.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Delivery and outcome_unknown (E4)" (:171-224) — binding: the three rules :175-179, what counts :181, the table :183-191, the event change :195, the redirect behaviour change :197-202, the tests :204-215, the GUESS list :217-223; Waves 1E (:456).
- crates/core/src/model.rs:221 `Event`, :245 `Failed`.
- crates/core/src/engine.rs: `Event::Failed` constructor :266, the OAuth2 token fetch :734-744, `Replay::Bytes` :795, Digest/SigV4 pre-send errors :891, :931, the single `rb.send()` :909-913, the redirect method rules and 9-hop limit :956-1012 (:977), `kind_of` :1087-1100, error text :1109-1126.
- crates/core/src/client.rs:55-75 the client builder (`Policy::none` today only when `follow_redirects` is off, :64-66).
- Existing per-hop tests to keep green: `engine.rs:1102`, `:1208`; `crates/core/tests/cookies.rs:460`; serde round-trip `crates/core/tests/engine.rs:429`.
- Dev-dependencies already present in `crates/core/Cargo.toml`: `wiremock`, `h2`, `tokio-rustls`, `rcgen`, `bytes` (no manifest change needed).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 164 engine-delivery-contract`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-164`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `Event::Failed` gains `nothing_sent: bool` with `#[serde(default)]`. Tests: serde round-trip with the field; an old JSON `Failed` without it reads `nothing_sent == false`.
- [ ] AC2 Every client the engine builds uses `Policy::none` and `.retry(reqwest::retry::never())` (or the confirmed equivalent in reqwest 0.13.5 `src/retry.rs`; the PR cites the source line). While `follow_redirects` is on, the engine follows every redirect itself for all methods, at most 9 hops, through the one `rb.send()` call site; there is no second send call in `engine.rs`.
- [ ] AC3 A per-send state owned by the `Command::Send` task, passed to `perform` by reference, is set just before each `rb.send()` and read after the `select!` (so it survives a cancel). `nothing_sent` is true only when no send started, or exactly one send started and it failed with `is_connect()` with no response head. Error-kind text is not consulted.
- [ ] AC4 Tests (core, wiremock or raw `TcpListener`), each asserting `nothing_sent` on the `Failed` event: POST 307 to a closed port → false; POST with Digest, 401 with `Connection: close`, listener dropped before the retry → false; POST 303 to a closed port (hop is GET) → false; POST to a closed port, no redirect → true; a server that reads the request and then resets the TLS connection → false; an unreachable OAuth2 token endpoint → `Auth` and true; an unreadable body file → `BodyFile` and true.
- [ ] AC5 Test: an HTTP/2 server (dev-dep `h2`) that answers a POST with REFUSED_STREAM counts exactly one stream opened, and the request ends in `Failed` (no silent retry).
- [ ] AC6 Redirect behaviour preserved: the existing per-hop method, header and cookie tests pass unchanged; a new test shows a 10th redirect fails with the existing too-many-redirects error, and a request without auth or Cookie that redirects is now followed by the engine (observable as one `rb.send()` per hop through the timing hook or hop count).
- [ ] AC7 The PR body answers each GUESS in spec :217-223 with a dependency source citation (crate, version, file:line) or states which test covers it; any GUESS found false is reported first.
- [ ] AC8 `Command`, `ErrorKind` and `PreparedRequest` are unchanged; no file outside Owns changes; fmt, clippy and the workspace suite pass (app matches already use `..`).

## Checks
- `delivery-core` command — AC1, AC3, AC4, AC5
  run: `cargo test -p wisp-core --locked --test engine nothing_sent 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `nothing_sent` in the name of each AC1, AC4 and AC5 test.)
- `redirects` command — AC6
  run: `cargo test -p wisp-core --locked redirect 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `single-send` command — AC2
  run: `test "$(rg -c '\.send\(\)' crates/core/src/engine.rs)" = 1 && rg -n 'retry' crates/core/src/client.rs`
- `scope` command — AC8
  run: `git diff --exit-code main...HEAD -- crates/app crates/storage crates/cli crates/core/src/resolve crates/core/Cargo.toml Cargo.lock`
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `guesses` read — AC2, AC7
  how: the reviewer checks the PR's citations for each GUESS and the retry builder call.

## Out of scope
- `hash_file` and AC2 of task 99 (167, same file, after this task).
- `delivery`/`retry_safe` derivation and history (161, 166).
- Loop detection beyond the 9-hop limit (spec :199: none).
