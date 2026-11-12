# 110 — Run refresh-token store calls off the engine workers

**Repo:** `.`
**Depends on:** none
**Size impact:** none expected (no new dependencies; `no-deps` check)
**Owns:**
- crates/core/src/auth/oauth2.rs
- crates/core/src/auth/oauth2/manager.rs
- crates/core/src/auth/mod.rs
- crates/core/tests/oauth2.rs

## Goal
This closes the P2 that must land before a keychain store ships (PLAN, follow-ups from task 103). Today `RefreshTokenStore` calls run synchronously on engine workers, of which there are one or two, so a blocking keychain prompt would stall all engine I/O. Move every store call onto `spawn_blocking`, keep calls for one key in order, and let the app turn persistence on for a single prepared request. `resolve()` stays pure, and still sets `persist_refresh: false`.

## Context
- crates/core/src/auth/oauth2.rs:53-61 — the trait doc already promises spawn_blocking
- crates/core/src/auth/oauth2/manager.rs:224-360 — every store call site (`load`, `save`, `forget`) and the per-key `store_locks`
- crates/core/src/auth/mod.rs:48, :91, :164 — where `persist_refresh` lives on `EngineAuth`
- crates/core/src/resolve/engine.rs:548 — stays `false`; the app sets the flag after resolve (task 115)
- docs/conventions.md#tests — no sleeps to wait for state

## Acceptance
- [ ] AC1 Every `RefreshTokenStore::{load, save, forget}` call reachable from the engine runs inside `tokio::task::spawn_blocking`. No store method is called directly on an async worker.
- [ ] AC2 The per-key store lock is a `tokio::sync::Mutex` held across the awaited `spawn_blocking`. Calls for one `TokenKey` therefore run one at a time, in order. A test with a recording store shows that save-then-forget for one key reaches the store in that order.
- [ ] AC3 The engine is started with one worker, and a test store's `load` blocks on a channel the test controls. A plain no-auth send to a mock server completes with `Event::Completed` while that `load` is still blocked. The test then releases the store.
- [ ] AC4 Fail closed. A store `load` error during a send ends that send with `Event::Failed`, and the message carries the store's message. No refresh POST is made. A failed `spawn_blocking` (`JoinError`) maps to a fixed `RefreshStoreError` message. Neither message contains a token, and a test asserts this with a sentinel value.
- [ ] AC5 `EngineAuth::set_persist_refresh(&mut self, on: bool)` sets the flag on every variant that has one and does nothing on the others. A unit test covers one variant of each kind.
- [ ] AC6 The existing OAuth2 tests (`crates/core/tests/oauth2.rs`) still pass, and `resolve()` is unchanged.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `oauth2` command — AC2, AC3, AC4, AC6
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-110; test "$(cargo test -p wisp-core --locked --test oauth2 store_ -- --list 2>/dev/null | rg -c ': test$')" -ge 3 && cargo test -p wisp-core --locked`
  (new integration tests have `store_` in their name)
- `unit` command — AC5
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-110; cargo test -p wisp-core --locked --lib set_persist_refresh`
- `resolve-pure` command — AC6
  run: `git diff --exit-code main...HEAD -- crates/core/src/resolve`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock crates/core/Cargo.toml`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-110; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `blocking` read — AC1
  how: no store method is called outside a `spawn_blocking` closure. The lock is held across the await, not taken inside the closure.

## Out of scope
- Any keychain code; the store implementation is task 115.
- Other task-103 follow-ups: the in-memory stale marker (43-x "stale marker"), invalid_grant handling (43-9), single-flight refresh, and the 401 invalidation race.
- Setting `persist_refresh` from resolve or from the environment.

## Review round 1 — findings

Reviewed HEAD: 420154071dd229d5d5bbdc166207bd5169b6ca1a

- [P2] crates/core/src/auth/oauth2/manager.rs:384-389 — In `bearer`, the per-key store lock is a borrowed guard inside the send future. A cancelled send drops the future and releases the lock while the `spawn_blocking` `load` keeps running, so a second store call for the same key can overlap it (breaks AC2 "run one at a time"). Input: persist_refresh on, no cached refresh token, store `load` blocks; user cancels the send (engine.rs:257-270 drops `perform`) and resends or completes an Authorize → two store calls for one key at once. Fix: take `lock_owned()` and move the owned guard into the `spawn_blocking` closure (lock still taken outside the closure, held until the store call returns), or run the load in a spawned task as `refresh` does.
- [P2] crates/core/tests/oauth2.rs:1613-1640 — `store_calls_for_one_key_run_in_order` runs two authorizes sequentially, so it passes with the per-key lock removed (verified by replacing the guard with `()`). Make it overlap two store calls for one key (e.g. `save` blocks on a channel while a second authorize or refresh starts) and assert the order; it should fail without the lock.
- [STYLE] crates/core/src/auth/oauth2.rs:55 — doc-comment line is 124 chars; neighbouring docs wrap at 100.
