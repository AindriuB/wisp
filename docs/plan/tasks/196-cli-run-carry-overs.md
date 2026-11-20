# 196 — Fix the `run` keychain-canary flake and close the CLI follow-ups

**Repo:** .
**Round:** 0
**Depends on:** 191
**Owns:**
- crates/cli/**
- crates/service/src/run.rs (tests and the flake fix only)
- scripts/check-boundaries.sh

## Goal
Find and fix why the service keychain-canary test sometimes sees a connect
failure. Remove the port race in the CLI tests. Close the 0.4.0 snapshot and
`run` test gaps, and close the boundary-check gaps.

## Context
- docs/plan/known-flakes.md, third entry: `run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7`, `crates/service/src/run.rs:985`. In run 44299517411 the "exit 5" case reported `Completed` where `Failed(Connect)` was expected.
- PLAN.md follow-ups from task 166 (:70-79), 173 (:32-38) and 160 (:136-139).
- crates/cli/tests/run.rs:150-154 `closed()`.
- crates/cli/src/cmd/snapshot.rs:72-74 (`environment_info` failure) and :141-145 (no `index.jsonl`); crates/cli/src/output.rs `snapshot_text`, `fsize`.
- scripts/check-boundaries.sh:52, the `resolve(` check.
- Worktree: `./.claude/scripts/wt-new.sh . 196 cli-run-carry-overs`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-196`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 The PR states the cause of the exit-5 flake. The fix removes that cause; it does not add a retry or a sleep. The exit 0, 1 and 7 cases assert the specific outcome: a pass for 0, the failed check for 1, the bad status for 7.
- [ ] AC2 `closed()` no longer binds and drops a port that another process could take. The PR says why the chosen address fails deterministically on all three CI OSes.
- [ ] AC3 CLI tests cover the rendering of a `missing-secret` problem in text and in `--json`, and assert that no secret value appears.
- [ ] AC4 In a snapshot, an environment whose `environment_info` fails gets a note and a `truncated` (or equivalent) entry naming it, instead of a silent `variable_count` 0. Test.
- [ ] AC5 The text output's `== recent ==` lines include each entry's request path, escaped. Test.
- [ ] AC6 Tests: the fallback unit test has `request` and `recent` present and checks that the minimal `truncated` names them; a migrated history dir with no `index.jsonl` is handled as documented.
- [ ] AC7 `fsize`'s counting writer stops once past the limit, so a hostile collection is not fully rendered at each fitting step.
- [ ] AC8 `text_rendering_is_bounded_too` is renamed `snapshot_text_rendering_is_bounded_too`.
- [ ] AC9 `scripts/check-boundaries.sh` flags a path-qualified `wisp_core::resolve::resolve(` call and an aliased import (`use wisp_core::resolve::resolve as r;`). It still passes on the branch head.
- [ ] AC10 No file outside Owns changes. No new dependency. Every test passes; the flaky test passes 20 consecutive runs locally (the command below). fmt, clippy and the workspace suite pass.

## Checks
- `flake-loop` command — AC1, AC10
  run: `for i in $(seq 1 20); do cargo test -p wisp-service --locked --lib run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7 2>&1 | grep -qE 'test result: ok\. 1 passed' || exit 1; done`
- `cli-tests` command — AC2, AC3, AC4, AC5, AC6, AC8
  run: `cargo test -p wisp-cli --locked 2>&1 | tee /dev/stderr | grep -E 'test result: ok\.' && cargo test -p wisp-cli --locked snapshot_text_rendering_is_bounded_too 2>&1 | grep -E 'test result: ok\. 1 passed'`
- `boundaries` command — AC9
  run: `scripts/check-boundaries.sh`
- `scope` command — AC10
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/cli/' -e '^crates/service/src/run.rs$' -e '^scripts/check-boundaries.sh$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC10
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `fixes` read — AC1, AC2, AC7, AC9
  how: the reviewer checks that the flake cause in the PR matches the change, that the address choice is deterministic, that the writer short-circuits, and that the script's new patterns catch the two forms (by reading them, or by a scratch run the PR quotes).

## Out of scope
- The task 166 Lows (the 200 ms sleep, the `carries_id` guard); NOTEs.
- `docs/cli.md` (scribe, task 197).
- Any behaviour change to `run` exit codes or output beyond AC4 and AC5.
