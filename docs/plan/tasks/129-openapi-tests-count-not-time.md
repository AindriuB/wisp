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
- [ ] AC3 Every counter and its increments are compiled only under `#[cfg(test)]`, with zero release-build cost. Outside `#[cfg(test)]` items the diff is limited to: the counter increment lines and their `cfg` attributes; the body of `substitute` (made linear, Decision 2026-11-15 (a)); and the parameter-merge key type (Decision (c)). No observable behaviour changes: `substitute` output and merge results are identical for every input.
- [ ] AC4 Each test keeps its existing functional assertions (`TooLarge` result, `params.len() == 60_000`, `substitute` output equality).
- [ ] AC5 Non-vacuity: for each test the PR description states one concrete quadratic regression (e.g. the merge scanning all prior parameters per parameter) and the count it would produce for the test's input, showing it exceeds the bound.
- [ ] AC6 The other wall-clock tests in `openapi.rs` are unchanged.
- [ ] AC7 Mutation proof: reintroducing the linear-scan merge (a `position()` scan over prior parameters instead of the map lookup) fails `many_parameters_merge_in_linear_time`.
- [ ] AC8 Mutation proof: removing the `None => break` early exit in `substitute` fails `brace_heavy_url_is_linear`.
- [ ] AC9 Mutation proof: restoring the old quadratic `substitute` (HEAD 0229992 body) fails `brace_heavy_url_is_linear`.

## Checks
- `openapi-tests` command — AC1, AC2, AC4
  run: `for i in 1 2 3; do cargo test -p wisp-storage --lib import::openapi || exit 1; done`
- `suite-and-lint` command — AC3, AC4
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-clock` read — AC1, AC3, AC6
  how: the three test bodies have no clock read; counters are `#[cfg(test)]`; other `Instant` uses in the file are untouched.
- `bound-is-meaningful` read — AC2, AC5
  how: the counter sits inside the loop that would go quadratic, and the PR's stated regression count exceeds the asserted bound.
- `mutation-proofs` read — AC7, AC8, AC9
  how: the PR description reports each mutation applied locally and the failing assertion it produced; the reviewer may re-apply any of them.

## Out of scope
- `docs/plan/known-flakes.md` — the scribe removes the entries at record.
- The other `Instant` asserts in `openapi.rs` (621-749 others, 784, 831, 859) and in `yaml.rs`, `postman.rs`, `curl.rs`, `write.rs`: follow-ups.
- If the counted loop lives outside `openapi.rs`, stop and report rather than editing that file.

## Decisions
- 2026-11-15 (user, after code review round 1 of PR #78): widen 129 beyond test-only. Owns stays `crates/storage/src/import/openapi.rs`.
  (a) Make `substitute` (`openapi.rs` ~:204-216) linear in bytes scanned while keeping today's matching exactly, including var names containing `{` — e.g. remember the absolute position of the next `}` and search again only once the cursor passes it.
  (b) The substitute counter counts bytes scanned, including what `find` scans, so the test is truthful. `brace_heavy_url_is_linear` asserts a separate bound per input: `big` a small constant, `closed` linear in its length.
  (c) The parameter-merge test counts key comparisons/hashes via `#[cfg(test)]`-gated counters on the merge key type (a small newtype key with manual `Eq`/`Hash` if needed), so replacing the `HashMap` lookup with a linear `position` scan fails the test.

## Review round 1 — findings (HEAD 0229992)
- [P2] openapi.rs:450/:793 — `MERGE_STEPS` counts outer iterations, so a quadratic merge passes (mutation: a `position()` scan passes after 200 s). Settled by Decision (c).
- [P2] openapi.rs:849 — brace test sums two inputs into one bound; removing the `break` at :216 still passes. Settled by Decision (b).
- [P2] openapi.rs:204-207 — `SUBST_STEPS` counts iterations, not bytes; `substitute` is quadratic today (~136 ms per 64 KB pathological URL, ~5 s on a hostile 10 MiB spec, bounded by `MAX_URL_BYTES` × `MAX_ENVIRONMENTS`). Settled by Decisions (a)+(b).
- [NOTE] `shared_ref_parameter_names_are_charged` still guards its property.
