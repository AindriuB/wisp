# Known flakes

Tests that have changed result on rerun. A flake listed here does not block a
merge; a new one is looked at first. Only `scribe` edits this file.


- `redacts_secrets_in_url_params_vars_and_bodies` (`wisp-storage --test history`, `crates/storage/tests/history.rs:356-360`). Failed once on ubuntu-24.04 with "1234 leaked" on PR #110 (run 43979433005, 2026-11-16) and passed on rerun; #110 changed no code. Likely cause: the test asserts the literal `1234` is absent from every raw history file, and those files also hold random entry ids and timestamps that can contain `1234`. Accepted by the user 2026-11-16. Fix: use a distinctive canary (for example `918273645`) for `pin_key`.

- `h3_load_without_a_data_dir_refuses_a_keychain_environment` (`wisp-storage --test vault`, `crates/storage/tests/vault.rs:2725`, task 169). Failed once on Windows CI for PR #132 at 512dfff (run 44273174919, 2026-11-18) and passed on rerun. Likely cause: the test stores the secret value `t1` and asserts the error text does not contain `t1`, but the error includes a random temp-dir path that can contain `t1`. Accepted by the user 2026-11-18. Fix: use a value a random path cannot contain (for example `zz-hunter`). Fixed by task 186 (AC5); remove this entry once CI stays green.

- `run::tests::run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7` (`crates/service/src/run.rs:985`, task 166). Failed once on ubuntu-24.04 for task 171's branch at 23ac3cb (run 44299517411, 2026-11-18): the "exit 5" case left `Completed`, right `Failed(Connect)`. It passed on re-run, and 171 changes nothing in `crates/service`. Cause not yet found. Accepted by the user 2026-11-18 as unrelated to 171. Follow-up: find why that case sometimes reports a connect failure and fix it.
