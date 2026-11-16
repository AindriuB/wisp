# 150 — Mask the SigV4 access key field in the auth editor

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/request/auth.rs (the `access_key` input construction :321 and its render row :688 only)

## Goal
The AWS SigV4 access key shows in clear text in the request auth editor while
every other credential field is masked and `Debug` already redacts it (spec
amendment 1, H8 / AC3; PLAN 33-13). Make the access key a masked input with the
same show/hide toggle as the secret key. A one-line-class change; no new behaviour.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md:388 (H8, AC3); docs/plan/roadmap.md:122; docs/plan/PLAN.md:597 (33-13). The cited `auth.rs:219` is stale; the lines are now :321 and :688.
- crates/app/src/ui/request/auth.rs:310-315 — the `plain` and `secret` constructors; `secret` sets `.masked(true)`
- crates/app/src/ui/request/auth.rs:321 — `let access_key = plain("Access key", window, cx);`
- crates/app/src/ui/request/auth.rs:687-691 — the SigV4 render rows; Secret key uses `Input::new(..).mask_toggle()`
- Worktree: `./.claude/scripts/wt-new.sh . 150 sigv4-access-key-mask`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-150`. Run every command in the foreground; leave nothing running.
- Never log a credential value; tests use literal fake values only and never touch the real keychain.

## Acceptance
- [ ] AC1 `access_key` is built with the `secret` constructor (masked) and keeps the placeholder "Access key".
- [ ] AC2 The SigV4 "Access key" row renders `Input::new(&self.access_key).mask_toggle()`, matching the Secret key row.
- [ ] AC3 Nothing else in auth.rs changes: `Slot`, `slots_of`, `build_auth`, the other fields and the Region/Service rows are untouched (read check).
- [ ] AC4 The existing `ui::request::auth` tests still pass; the full suite, fmt and clippy pass.

## Checks
- `auth-unit` command — AC4
  run: `cargo test -p wisp-app --bin wisp ui::request::auth 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC4
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `diff-scope` command — AC3
  run: `git diff --name-only main...HEAD | grep -vx 'crates/app/src/ui/request/auth.rs' | wc -l | grep -qx ' *0'`
- `mask-shape` read — AC1, AC2, AC3
  how: the diff is two changed lines in auth.rs: `plain` to `secret` for `access_key`, and `.mask_toggle()` added to its row. No other hunk.

## Out of scope
- Storage over-redacting non-secret `auth.*` fields (PLAN :510), SigV4 signing fixes (PLAN :587, :595, :599), and the `replay` body copy (:588).
- Masking Region, Service, Client ID or any other field; changing the `InputState` mask widget.
- Adding a GPUI test for masking; the change is covered by the read check.

## Outcome
Merged as PR #96 (da2b120) at 188e9e7. The SigV4 access key input is masked with a show/hide toggle. PLAN item 33-13 is closed.
