# Known flakes

Tests that have changed result on rerun. A flake listed here does not block a
merge; a new one is looked at first. Only `scribe` edits this file.

- `crates/core/tests/timings.rs:169 a_failed_lookup_is_still_a_dns_error`: failed once on macos-latest CI (the PR #80 push run, 2026-11-15) and passed on rerun. Under investigation; the cause is not known.
