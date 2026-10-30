# 36 — Raise the pretty-print output budget to max(32 MiB, 4 × input)

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/pretty.rs

## Goal
Raise the `pretty_json` output budget from max(1 MiB, 4 × input) to max(32 MiB, 4 × input) (user decision, 2026-10-30), so moderately nested minified documents with short values up to the ~5 MB pretty limit pretty-print instead of falling back to Raw. Examples are rows like `[[1,2,3],...]` and GeoJSON coordinates, which grow about 6× when pretty-printed. The indent cap stays at 32 levels, and output over the budget still returns `Err`, which the viewer shows as Raw. The task also folds in the task 26 review items for the same file: the nesting tests must assert the exact outcome, and `SPACES` must be tied to `MAX_INDENT_LEVELS`.

## Context
- crates/app/src/pretty.rs:193-205 — `MAX_INDENT_LEVELS`, `output_budget`, `newline` and its `SPACES` literal
- crates/app/src/pretty.rs:207-230 — `pretty_json`: BOM skip, the budget check at the top of each token loop, the `"output too large to pretty-print"` error
- crates/app/src/pretty.rs:53-80 — `sniff_kind` (the optional BOM item)
- crates/app/src/pretty.rs:387-440 — `deep_nesting_is_iterative`, `hostile_nesting_stays_within_budget`, `indent_is_capped_at_32_levels`, `five_mib_document`
- docs/plan/PLAN.md "Formatting (26, optional, fold into 36)" — the review items
- Sizing note: a bracket costs at most 1 + 1 + 64 bytes of output once the indent is capped. So `"[".repeat(n) + "]".repeat(n)` gives about 132 × n bytes. That is about 1.3 MB at n = 10,000 and about 4.3 MB at n = 32,768, both Ok under the new budget. At n = 300,000 it gives about 39.6 MB, which is over 32 MiB, while the opening half is only about 19.8 MB.
- Task 35 re-measures memory budgets (after one request ≤ 128 MB) with this new limit in place. This task does not measure RSS.
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-36`.

## Acceptance
- [ ] `output_budget(len)` returns `(32 * 1024 * 1024).max(len.saturating_mul(4))`. `MAX_INDENT_LEVELS` is still 32, and output over the budget still returns `Err` with message `"output too large to pretty-print"`.
- [ ] A compile-time assert ties the indent string to the cap, for example `const _: () = assert!(SPACES.len() == MAX_INDENT_LEVELS * 2);`. `SPACES` moves to module scope if it has to for the assert to name it.
- [ ] New test: a minified document `{"data":{"rows":[[1,2,3],[1,2,3],...]}}` of at least 2,000,000 bytes returns `Ok`. Its output is longer than 4 × the input length, which proves the old budget would have rejected it, and no longer than `output_budget(input.len())`.
- [ ] New test: an input whose pretty output would exceed max(32 MiB, 4 × input), such as `"[".repeat(300_000) + "]".repeat(300_000)`, returns `Err` whose `message` is `"output too large to pretty-print"` and whose `offset` is greater than 300,000, past the last opening bracket.
- [ ] Allocation stays bounded, and a reader can check this from the diff. The budget is still checked before every token, so `out` is never more than budget + one token + 66 bytes. The function never reserves capacity from `budget`: `String::with_capacity` stays proportional to the input length.
- [ ] `deep_nesting_is_iterative` has no `if let Ok`. Both of its cases (n = 10,000 `[`…`]` and n = 10,000 `{"a":`…`1`…`}`) `unwrap()` an `Ok` and assert `out.len() <= output_budget(s.len())`. The array case also asserts `out.len() > n`.
- [ ] `hostile_nesting_stays_within_budget` has no `if let Ok`. At n = 32,768 it asserts `Ok` with `out.len() <= output_budget(s.len())`. The budget-error test above can be this test, extended, or a separate test.
- [ ] Optional: `sniff_kind` skips a leading UTF-8 BOM (`EF BB BF`) before it looks for the first non-whitespace byte, so `content_kind(&[], b"\xEF\xBB\xBF{\"a\":1}")` is `ContentKind::Json`. If this is done, a matching assert is added to `kind_from_sniffing`.
- [ ] All existing tests in `pretty.rs` pass unchanged, apart from the two nesting tests rewritten above.
- [ ] With `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-36`, the following are clean: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`.

## Out of scope
- `settings.pretty_limit` (~5 MB) and how the response viewer offers or falls back from Pretty (task 33, `crates/app/src/ui/response/**`).
- Changing the 32-level indent cap or the two-space indent.
- RSS or binary-size measurement, and editing `docs/architecture.md#budgets` (task 35, scribe).
- Streaming or incremental pretty-printing, and pretty-printing XML or HTML.
- `pager.rs`, `cookies.rs` and `format.rs`, even though task 26 delivered them too.
