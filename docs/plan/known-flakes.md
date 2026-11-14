# Known flakes

Tests that have changed result on rerun. A flake listed here does not block a
merge; a new one is looked at first. Only `scribe` edits this file.

- `wisp-storage import::openapi::tests::many_parameters_merge_in_linear_time`: wall-clock assertion. Failed once under parallel load (clippy building) on 2026-11-10 in task 102's worktree and passed on rerun. The fix is tracked as the wall-clock-assert follow-up (34-19) for task 99.
- `wisp-storage import::openapi::tests::brace_heavy_url_is_linear`: wall-clock assertion. Failed under parallel load on 2026-11-10 in task 105's worktree and passed on rerun. Same fix as the entry above (task 99, 34-19).
- `wisp-storage import::openapi::tests::shared_ref_parameter_names_are_charged`: wall-clock assertion. Failed under parallel load on 2026-11-14 in the task 118, 119 and 120 worktrees and passed on rerun. Same fix as the two entries above (task 99, 34-19).
