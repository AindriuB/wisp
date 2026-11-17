# 159 — Generate dynamic values through a DynamicSource (1A, E3 core)

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/core/src/resolve/engine.rs
- crates/core/src/resolve/mod.rs

## Goal
`{{$uuid}}` and `{{$timestamp}}` are generated through a caller-supplied
source, so a send can record the values it used and a preview can generate
nothing. `resolve()` keeps its signature and behaviour (spec Feature 3).

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 3" (:286-307) — binding design; Waves 1A (:455).
- crates/core/src/resolve/engine.rs:191 `resolve`, :212 `Resolver`, :284-291 the generation site (one value per occurrence, user decision).
- crates/core/src/resolve/mod.rs:86 re-exports.
- crates/core/src/resolve/report.rs:1062-1063 (Explain never generates) — read only, owned by 162.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 159 dynamic-source`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-159`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `engine.rs` defines `pub trait DynamicSource { fn generate(&mut self, name: &str) -> Option<String>; }` answering `uuid` and `timestamp` and `None` for any other name, with `Fresh` (today's generation code), `Recording` (wraps `Fresh`, exposes the recorded `Vec<DynamicValue>`) and `Placeholder` (`00000000-0000-4000-8000-000000000000` and `0`).
- [ ] AC2 `DynamicValue { name, value }` (both `String`; `name` includes the `$`, e.g. `$uuid`) and `resolve_with(req: &Request, scope: &Scope, dynamics: &mut dyn DynamicSource) -> Result<PreparedRequest, ResolveError>` are re-exported from `resolve/mod.rs` along with the trait and the three sources.
- [ ] AC3 `Resolver` holds `&mut dyn DynamicSource`; `resolve()` keeps its signature and delegates to `resolve_with` with `Fresh`. `crates/core/src/resolve/tests.rs` is unchanged.
- [ ] AC4 Test: `resolve_with` with `Placeholder` on a request using `$uuid` in URL, header and body twice is byte-identical across two calls.
- [ ] AC5 Test: `Recording` lists one entry per occurrence, in order of use, and the values equal the ones in the prepared request (two `$uuid` occurrences give two different recorded values).
- [ ] AC6 `PreparedRequest` is unchanged; nothing outside the two owned files changes; fmt, clippy and the workspace suite pass.

## Checks
- `dynamic-unit` command — AC1, AC4, AC5
  run: `cargo test -p wisp-core --lib resolve::engine::dynamic 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (name the new tests `resolve::engine::dynamic_tests::*` or any path matching `resolve::engine::dynamic`.)
- `untouched` command — AC3, AC6
  run: `git diff --exit-code main...HEAD -- crates/core/src/resolve/tests.rs crates/core/src/model.rs && test "$(git diff --name-only main...HEAD | grep -v -e '^crates/core/src/resolve/engine.rs$' -e '^crates/core/src/resolve/mod.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `api` read — AC1, AC2, AC3
  how: the reviewer checks the trait, the three sources, the re-exports and that `resolve` delegates with `Fresh`.

## Out of scope
- Using the sources in the service (160) or the URL bar (175); Explain (`report.rs`, task 162).
- One value per send instead of per occurrence (user decision: per occurrence stays).

## Attempt 1 — failed

Verified HEAD: e9e4ae2b5c4150588fb6e0d82d268d84c9d03581. Tester: PASS. Reviewer: REQUEST CHANGES on AC5.
- AC5 is unproven: `recording_lists_each_occurrence_in_order` (crates/core/src/resolve/engine.rs:809-814) compares only v[0] and v[1] with the URL. Nothing checks that v[2] equals the X-Id header value, or that v[3] and v[4] equal the two body uuids in order. The body is only matched as `PreparedBody::Bytes { .. }`.
- A source that recorded the right names but handed the resolver different values, or recorded the body values before the header value, would pass.
- Fix: assert that the X-Id header equals v[2].value and that the body bytes equal `format!("{} {}", v[3].value, v[4].value)`, or the fixture's exact body layout.

## Wave verified — 9b555a738a06df3de2d95621342b64f1a908e896

## Outcome

Merged via record/159-dynamic-source at 9b555a738a06df3de2d95621342b64f1a908e896 after one failed attempt (Attempt 1: AC5 unproven, fixed in 9b555a7). Tester PASS (workspace suite exit 0, clippy and `cargo fmt --all --check` clean) and reviewer APPROVE. Commit c27526f does not compile; 9b555a7 fixes it. Follow-up: `lookup_var` still uses `Fresh` (PLAN.md).
