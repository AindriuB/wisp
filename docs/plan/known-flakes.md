# Known flakes

Tests that have changed result on rerun. A flake listed here does not block a
merge; a new one is looked at first. Only `scribe` edits this file.

- `crates/core/tests/timings.rs:169 a_failed_lookup_is_still_a_dns_error`: failed once on macos-latest CI (the PR #80 push run, 2026-11-15) and passed on rerun. On macOS CI run 43663344531 the host resolver stalled on `no-such-host.invalid`, so the engine's 30s default timeout fired and `classify` (`engine.rs:1105-1113`, timeout checked before DNS strings) returned Timeout instead of Dns; the test depends on the runner's resolver. Planned fix: unit-test classify with a synthetic DNS error, and give the network test a short timeout.
