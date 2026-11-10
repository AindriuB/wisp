# 105 — Phase timings: DNS and Connect+TLS measured per request, shown in the response view

**Repo:** `.`
**Depends on:** 103, 104 (both edit the files this task owns: `engine.rs` and `client.rs`)
**Size impact:** UNCERTAIN, expected small. Uses reqwest's `dns_resolver` and `connector_layer`. A `Layer`/`Service` impl may need direct `tower-layer`/`tower-service` (or `hyper-util`) entries, all already in `Cargo.lock`. Adding a crate that is not already in the lock is not allowed; stop and report instead.
**Owns:**
- crates/core/src/timing.rs, crates/core/src/client.rs, crates/core/src/engine.rs
- crates/core/tests/timings.rs (new)
- crates/core/Cargo.toml, Cargo.toml, Cargo.lock, THIRD-PARTY-NOTICES (only for crates already in the lock)
- crates/app/src/ui/response/mod.rs

## Goal
`Event::Completed.timings` carries the real DNS and Connect+TLS durations for the connection a request opened, and `None` for a reused connection. The response view shows DNS, Connect+TLS, First byte and Total. History already stores the values (task 100). The spec requires attribution to be proved race-free under pooling and HTTP/2 before the feature ships. This task starts with that proof. If the proof fails, it ships DNS and total only and says why.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#6-phase-timings-core — the spike rule, `None` not zero, attribution through a per-connection slot
- crates/core/src/timing.rs (stub from 100), `Timings` in crates/core/src/model.rs
- crates/core/src/client.rs — `ClientCache::get`; keep the cookie provider from 104 and the TLS setup unchanged
- crates/core/src/engine.rs — `perform` and the own-redirect loop from 95 and 103. With redirects, report the phases of the first hop that opened a connection; say which in a doc comment.
- crates/core/tests/tls.rs — the self-signed rustls server pattern for the HTTP/2 (ALPN) case
- crates/app/src/ui/response/mod.rs:54-60, :140-150, :233-245 — `timing_text`, `done_tooltip`, the Done line
- docs/conventions.md#tests — no sleeps to wait for state
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-105`. App launch (optional, for the AC8 hand check): run only the binary you built, with `WISP_DATA_DIR` set to a fresh `mktemp -d` (task 101 makes this isolate `settings.toml` too). Stop only the pid you started; never `pkill` or `killall`. If you build a bundle, give it a bundle id other than `io.github.aindriub.wisp`.

## Acceptance
**Attribution (proved first, in its own commit)**
- [ ] AC1 Test, HTTP/1.1 keep-alive: a local server counts accepted connections. Twenty sequential and then twenty concurrent sends to it on one client produce exactly as many `Completed` events with `connect: Some(_)` as connections the server accepted. Every other `Completed` has `connect: None`.
- [ ] AC2 Test, HTTP/2 over TLS (ALPN, self-signed as in `tls.rs`): ten concurrent sends share one connection. Exactly one `Completed` has `connect: Some(_)`, and no request reports a phase from a connection it did not wait on.
- [ ] AC3 `dns` is `Some` only for a request that triggered a lookup. It is `None` for an IP-literal host and for a reused connection. Each value is at most that request's `total`. Tested.
- [ ] AC4 Fallback: if AC1 or AC2 cannot be made to pass without sleeps or retries, `connect` is always `None` and only DNS (if AC3 holds) and total ship. The PR then states the race found, and the doc comment on `Timings` says why. This AC passes only when either the fallback is fully in place or AC1-AC3 all pass; partial attribution does not count.

**Behaviour kept**
- [ ] AC5 Name resolution through the custom resolver gives the same results and `ErrorKind::Dns` failures as before. Proxy, redirect, TLS and cookie tests in `wisp-core` pass unchanged.
- [ ] AC6 Cancelling a request mid-connect leaves no slot behind: a test cancels during a stalled connect, then sends to a working server and checks that the second request's timings are its own.

**Display**
- [ ] AC7 A pure helper formats the phase line, e.g. `DNS 3 ms · Connect+TLS 41 ms · First byte 88 ms · Total 95 ms`, omitting any phase that is `None` (never "0 ms"). It is shown in the Done tooltip, or a line under the status, and has unit tests for all-present, reused (no DNS or connect) and total-only.
- [ ] AC8 Hand check (optional, macOS): one request to a fresh host shows all four phases; an immediate second request to the same host shows First byte and Total only. Reopening the first from History shows the same phases.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `timings` command — AC1-AC6
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-105; test "$(cargo test -p wisp-core --locked --test timings -- --list 2>/dev/null | rg -c ': test$')" -ge 5 && cargo test -p wisp-core --locked`
- `display` command — AC7
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-105; cargo test -p wisp-app --locked ui::response::`
- `lock-only-existing` command — size
  run: `! git diff main...HEAD -- Cargo.lock | rg -q '^\+name = '`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-105; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `size` command — size
  run: `gh workflow run measure.yml --ref task/105-phase-timings`; when it finishes, `gh run download <run-id> -n measure-x86_64-pc-windows-msvc -D <scratch> && jq -e '.binary_bytes <= 15500000' <scratch>/measure-x86_64-pc-windows-msvc.json`. Report the delta from task 104's figure.
- `fallback` read — AC4
  how: either AC1-AC3 pass, or `connect` is never set and the reason is in the PR and the doc comment.

## Out of scope
- A separate TCP and TLS split (excluded by the spec).
- Timings on `Failed` events.
- Showing timings in the history panel's list rows (`ui/history.rs` restores them on reopen already, from 100).
- OAuth2 or cookie behaviour changes in `engine.rs` or `client.rs` beyond what attribution needs.
