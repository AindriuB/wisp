# 199 — Compare JSON assertion numbers by value, not by type

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/core/src/assert.rs (production code and its `#[cfg(test)] mod tests`)

## Goal
JSON `Equals` and `Contains` assertions must compare numbers by numeric value, so
that `1` and `1.0` are equal, as they are in JavaScript and in Postman's `.to.eql`.
Today an integer expectation fails against a float-typed body number of the same
value, and the other way round. This affects every JSON assertion, both
hand-written and imported from Postman. The user decided this on 2026-11-20 after
reviewing PR #161 (task 194).

## Context
- crates/core/src/assert.rs:641-651 — `scalar_equals`. Lines 645-648 are the bug:
  `Expected::Int` matches only `is_i64()`/`is_u64()` numbers, and `Expected::Float`
  matches only `is_f64()` numbers.
- crates/core/src/assert.rs:660-697 — `eval_json`. `Equals` and array `Contains`
  both go through `scalar_equals`, so a fix there covers both.
- crates/core/src/assert.rs:493-503 — `eval_status`. It does not use `scalar_equals`
  and stays unchanged.
- crates/core/src/assert.rs:884-903 — `json_equals_is_type_strict`. Its case at
  lines 898-901 (`Int(1)` must fail against `1.0`) asserts the old behaviour. Flip
  it to a pass. The string-versus-number cases in that test stay strict.
- Workspace `serde_json` uses `features = ["std"]` with no `arbitrary_precision`
  (Cargo.toml:44). A body number is an `i64`, a `u64` or an `f64`.
- Task 194 (PR #161, Postman test translation) records the type-strict comparison
  as a residual until 199 lands.

## Rule (the comparison to implement)
| JSON number | Expected | Passes when |
|---|---|---|
| i64 / u64 | `Int(x)` | the integers are exactly equal (unchanged) |
| f64 `f` | `Float(x)` | `f == x` (unchanged) |
| f64 `f` | `Int(x)` | `f` is integral and inside the i64 range, and `f` as i64 `== x` |
| i64 / u64 `n` | `Float(x)` | `x` is integral and exactly equal to `n` |

No comparison goes through a lossy `i64 as f64` cast. An integer above 2^53
therefore never equals the float nearest to it. The range check before the cast
must exclude 2^63, because `as` saturates. Comparing in `i128` is one sound way.
NaN and infinity cannot appear in a JSON body (serde_json rejects them), and a
non-finite `Float` expectation simply matches nothing. State this in a short
comment on the function.

## Acceptance
- [ ] AC1 `Equals` passes for `Int(1)` against body `1.0`, and for `Float(1.0)` against body `1`.
- [ ] AC2 `Equals` fails for `Float(1.5)` against body `1`, and for `Int(1)` against body `1.5`.
- [ ] AC3 `Equals` fails for `Int(9007199254740993)` (2^53+1) against body `9007199254740992.0`, and for `Float(9007199254740992.0)` against body `9007199254740993`. It passes for `Int(9007199254740992)` against body `9007199254740992.0`.
- [ ] AC4 `Contains` passes for `Int(1)` and for `Float(1.0)` against body `[1.0, "x"]` and against body `[1, "x"]`. It fails for `Int(2)` against both arrays.
- [ ] AC5 `scalar_equals` carries a comment saying that NaN and infinity cannot appear in JSON. No comparison casts an i64 or u64 to f64.
- [ ] AC6 Status assertions are unchanged: `eval_status` and its tests are untouched.
- [ ] AC7 No new dependency. No file outside `Owns` changes. `cargo fmt`, `clippy` and the workspace suite pass.

## Checks
- `assert-unit` command — AC1, AC2, AC3, AC4, AC6
  run: `cargo test -p wisp-core --locked --lib assert:: 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (3[2-9]|[4-9][0-9]|[1-9][0-9]{2,}) passed; 0 failed'`
  (The repo's `cargo test` writes no JUnit XML, so this gates on the libtest summary line. 31 tests exist today, so at least 32 must pass. New tests that count as one more are acceptable. The reviewer confirms each AC case is in the diff.)
- `scope` command — AC7
  run: `test "$(git diff --name-only main...HEAD | grep -vx crates/core/src/assert.rs | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `rule` read — AC3, AC5, AC6
  how: the reviewer checks that the code follows the rule table, that there is no `as f64` on an integer, that the range guard excludes 2^63, that the NaN/infinity comment is present, and that `eval_status` is unchanged.

## Out of scope
- String-versus-number coercion (`"1"` vs `1` stays a failure).
- Deep equality of objects or arrays, and `Matches` on numbers.
- Any change to the Postman translator (task 194) or to `Expected` parsing.
- Status and header assertions.

## Supersession note (2026-11-20)
The user's decision on 2026-11-20 (PR #161 review, option a) supersedes docs/plan/specs/2026-11-14-wisp-0-3-0.md:186 ("compared by type") for numbers: JSON numbers now compare by value; strings vs numbers stay type-strict ("200" vs 200 still fails, so :230 and retired 131 AC2 stand). Follow-up for the scribe at record: amend spec :186 to say so and cite task 199.
Follow-up (P3): no test reaches the u64 branches (body numbers above i64::MAX); a lossy `u as f64` mutation passes the suite.

## Wave verified — 7b39f4483d6b3bb293d6d1ccb28819f95fad7649

## Outcome
Landed at 7b39f4483d6b3bb293d6d1ccb28819f95fad7649 (PR below, on branch `task/199-json-numeric-equality`). `scalar_equals` in `crates/core/src/assert.rs` now compares JSON numbers by value in `i128`, with no `as f64` cast on an integer, so `Int(1)` equals body `1.0` and `Float(1.0)` equals body `1`; integers above 2^53 never equal the nearest float. Strings against numbers stay type-strict. The tester passed, the reviewer approved AC1-AC7 and the full code review was clean. Spec `2026-11-14-wisp-0-3-0.md:186` was amended in the same commit.
Open follow-ups, recorded in PLAN.md: no test reaches the u64 branches of `scalar_equals` (`assert.rs:654-662`); the compare view (`crates/service/src/compare.rs:480`) still treats `1` and `1.0` as unequal.
