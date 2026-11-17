# Known flakes

Tests that have changed result on rerun. A flake listed here does not block a
merge; a new one is looked at first. Only `scribe` edits this file.


- `redacts_secrets_in_url_params_vars_and_bodies` (`wisp-storage --test history`, `crates/storage/tests/history.rs:356-360`). Failed once on ubuntu-24.04 with "1234 leaked" on PR #110 (run 43979433005, 2026-11-16) and passed on rerun; #110 changed no code. Likely cause: the test asserts the literal `1234` is absent from every raw history file, and those files also hold random entry ids and timestamps that can contain `1234`. Accepted by the user 2026-11-16. Fix: use a distinctive canary (for example `918273645`) for `pin_key`.
