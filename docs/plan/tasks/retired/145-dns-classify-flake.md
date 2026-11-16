# 145 — Unit-test DNS error classification and bound the network DNS test

**Repo:** .
**Depends on:** none
**Owns:**
- crates/core/src/engine.rs (`classify` :1078-1124 and the `tests` module only)
- crates/core/tests/timings.rs (`a_failed_lookup_is_still_a_dns_error` :162-172 only)

## Goal
`a_failed_lookup_is_still_a_dns_error` flakes when the runner's resolver stalls:
the 30 s engine timeout fires first and `classify` returns Timeout. Prove the
DNS classification with a unit test that needs no resolver, and stop the
network test from depending on the runner.

## Context
- docs/plan/known-flakes.md (the entry for this test; run 43663344531)
- docs/plan/PLAN.md:27 "Planned small task: DNS flake fix"; :332 (46-4, NXDOMAIN rewriting)
- crates/core/src/engine.rs:1078-1124 `classify` takes a `reqwest::Error`, which a test cannot build; the timeout check precedes the DNS string match
- Worktree: `./.claude/scripts/wt-new.sh . 145 dns-classify-flake`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-145`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 The kind decision in `classify` moves into a private pure function over the error's flags (timeout, builder, connect, body/decode, reading body) and the lower-cased source chain; `classify` calls it, and its kind order and message text are unchanged.
- [ ] AC2 Unit tests in engine.rs feed synthetic chains: each of `dns error`, `failed to lookup address information`, `name or service not known` with connect=true gives `Dns`; a timeout flag gives `Timeout`; a `certificate` chain gives `Tls`; connect with no DNS text gives `Connect`. No network, no resolver.
- [ ] AC3 The network test sets a short request timeout (at most 5 s) and accepts `Dns`, or `Timeout` with a message saying the runner's resolver stalled (printed with `eprintln!`); any other kind fails.
- [ ] AC4 No production behaviour change (read check); full suite, fmt and clippy pass.

## Checks
- `classify-unit` command — AC1, AC2
  run: `cargo test -p wisp-core --lib engine::tests 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `dns-network` command — AC3
  run: `cargo test -p wisp-core --test timings a_failed_lookup_is_still_a_dns_error 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. 1 passed'`
- `suite-and-lint` command — AC4
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-behaviour-change` read — AC1, AC4
  how: the extracted function keeps the original branch order; the message-building code is untouched.

## Out of scope
- `known-flakes.md` (scribe removes the entry when this merges).
- Splitting DNS from connect timing, a custom resolver, or any other engine change.

## Outcome
Merged as PR #93 (1f52bde) at 39c5e75. `classify` delegates to a pure `kind_of` with unit tests; the network test has a 5 s timeout and accepts Timeout with a note on stderr. The `known-flakes.md` entry is removed.
