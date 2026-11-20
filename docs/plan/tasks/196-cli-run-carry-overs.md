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

## Review round 1 — findings

Reviewed HEAD: fc81449ba6d1513abb3abf2602b0c34336952561. Verdict CLEAN; one optional finding.

- [P3, optional] crates/cli/src/cmd/run.rs:296-320 — In `run_missing_secret_renders_in_text_and_json_without_a_value`, the "no secret value appears" check can never fail. The hand-built `RunReport` (`not_prepared`, :272-293) contains no `CANARY` string anywhere, so `!out.contains("CANARY")` holds whatever `map`, `output::json` or `run_text` emit. The kind, name and message assertions are real. Only the no-leak half of AC3 is empty.
  route: implementer
  input: change `map`/`run_text` to echo any report field into the problem line → the test still passes, because nothing in the input looks like a secret.
  fix (cheap): put a canary value in a field a renderer could leak, such as a sibling problem, `reason` or `dynamics`, and assert it is absent. The binary tests cannot fill a real keychain (`env_remove("WISP_KEYCHAIN_SERVICE")`, crates/cli/tests/run.rs:72), so this unit test is the right place for it.

## Review round 1 — settled (orchestrator, 2026-11-20)

- [DRIFT, accepted residual] This task file is changed on the branch only by the /review loop's own record (`## Review round N` sections). That is workflow bookkeeping, not implementer scope, and matches earlier task branches (#158, #160, #161). When running the `scope` check, also exclude `^docs/plan/tasks/196-cli-run-carry-overs.md$`. No other path outside Owns may change.

## Attempt 1 — failed

/verify at HEAD 386f399 (2026-11-20): tester FAIL, reviewer APPROVE (AC1-AC9).
- `cargo fmt --all --check` fails: "Diff in crates/cli/src/cmd/run.rs:294". rustfmt wants `let mut report = not_prepared(vec![RunProblem::MissingSecret { name: "kc".into() }]);` on one line. Introduced by review-round-1 commit 0d3b2ff (CANARY-reason test fix). Run `cargo fmt` and commit; change nothing else.
- Everything else passed on this host: flake loop 20/20, check-boundaries ok, scope ok, clippy -D warnings on core/storage/service/cli, non-app suite green. Workspace-wide test/clippy blocked by missing fontconfig (wisp-app).
