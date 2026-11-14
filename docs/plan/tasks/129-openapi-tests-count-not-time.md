# 129 — Make the three flaky openapi import tests count work instead of timing it

**Repo:** .
**Depends on:** none
**Owns:**
- crates/storage/src/import/openapi.rs

## Goal
Three tests in `openapi.rs` assert wall-clock bounds and fail under parallel
load (follow-up 34-19; listed in `docs/plan/known-flakes.md`). Make them
deterministic by asserting a bound on counted work, so they prove the same
"no quadratic blow-up" property without depending on machine speed.

## Context
- crates/storage/src/import/openapi.rs:664-679 — `shared_ref_parameter_names_are_charged` (assert at 678)
- crates/storage/src/import/openapi.rs:752-768 — `many_parameters_merge_in_linear_time` (assert at 767)
- crates/storage/src/import/openapi.rs:805-820 — `brace_heavy_url_is_linear` (assert at 819, calls `substitute` directly)
- docs/plan/known-flakes.md — the four entries this task makes obsolete
- docs/plan/PLAN.md:513 — 34-19
- Worktree: `./.claude/scripts/wt-new.sh . 129 openapi-tests-count-not-time`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-129`

## Acceptance
- [ ] AC1 None of the three named tests contains `Instant`, `elapsed` or any other clock read.
- [ ] AC2 Each of the three tests asserts an explicit upper bound on a counted unit of work (for example a `#[cfg(test)]` counter incremented in the loop whose cost the test guards: parameter merge comparisons, `$ref` parameter-name charges, brace scan steps), and the bound is linear in the input size the test builds (stated as `k * n` with `n` derived from the test's own input, not a magic total).
- [ ] AC3 Every counter and its increments are compiled only under `#[cfg(test)]`; non-test code paths are byte-for-byte unchanged in behaviour (the diff outside `#[cfg(test)]` items is limited to the counter increment lines and their `cfg` attributes).
- [ ] AC4 Each test keeps its existing functional assertions (`TooLarge` result, `params.len() == 60_000`, `substitute` output equality).
- [ ] AC5 Non-vacuity: for each test the PR description states one concrete quadratic regression (e.g. the merge scanning all prior parameters per parameter) and the count it would produce for the test's input, showing it exceeds the bound.
- [ ] AC6 The other wall-clock tests in `openapi.rs` are unchanged.

## Checks
- `openapi-tests` command — AC1, AC2, AC4
  run: `for i in 1 2 3; do cargo test -p wisp-storage --lib import::openapi || exit 1; done`
- `suite-and-lint` command — AC3, AC4
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-clock` read — AC1, AC3, AC6
  how: the three test bodies have no clock read; counters are `#[cfg(test)]`; other `Instant` uses in the file are untouched.
- `bound-is-meaningful` read — AC2, AC5
  how: the counter sits inside the loop that would go quadratic, and the PR's stated regression count exceeds the asserted bound.

## Out of scope
- `docs/plan/known-flakes.md` — the scribe removes the entries at record.
- The other `Instant` asserts in `openapi.rs` (621-749 others, 784, 831, 859) and in `yaml.rs`, `postman.rs`, `curl.rs`, `write.rs`: follow-ups.
- If the counted loop lives outside `openapi.rs`, stop and report rather than editing that file.
