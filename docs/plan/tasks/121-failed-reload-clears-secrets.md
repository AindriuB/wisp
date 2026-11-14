# 121 — Clear active secrets when an environment reload fails

**Repo:** `.`
**Depends on:** none
**Size impact:** small (a few lines; no new dependency)
**Owns:**
- crates/app/src/workspace.rs (only `finish_env_load` and the inline test module)

## Goal
When a reload of the active environment fails (for example the keychain binding no longer checks, or the secrets file is now refused), the workspace keeps the previously loaded secrets in use while the status says the environment is in error. That fails open: requests keep resolving secrets the loader just refused. A failed reload must leave no secrets active, so status and secrets agree. Carried from task 99 AC73.

## Context
- crates/app/src/workspace.rs:1809-1853 `finish_env_load`. In the `Err` arm with `switching == false` (:1840-1845) it sets `env_status` to the error but leaves `active_env`, and its `secrets`, in place. The switching arm restores the previous environment by design and is not changed.
- crates/app/src/vault.rs:533-560 `LoadResult` and `load_err`.
- The gpui test harness added by task 115 (commit bde7cdf, "gpui workspace tests for env load generation, switch restore …") is the pattern for driving `finish_env_load` in a test.

## Acceptance
- [ ] AC1 (99-AC73) After a failed non-switching reload, the collection's active environment holds no secrets: either `active_env` is `None` or its `secrets` list is empty, and resolving a request that references a secret reports it unresolved rather than sending the old value. Plain vars may stay or go; the PR says which.
- [ ] AC2 (99-AC73) A gpui workspace test loads an environment whose secret `token` has value `SECRETVAL`, then completes a reload with an error result, and asserts that no active secret has value `SECRETVAL` and that the status is the error.
- [ ] AC3 A failed environment *switch* still restores the previous environment and status, as today; the existing switch-restore test passes unmodified.

## Checks
cargo writes no JUnit XML here, so these are command checks.
- `workspace-tests` command — AC1, AC2, AC3
  run: `cargo test -p wisp-app workspace`
  pass: exit 0, 0 failed, passed count at least the count on main 7ec7505 plus 1 (re-measure the base first; state both in the PR).
- `lint` command — all
  run: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `reload-arm` read — AC1, AC3
  how: only the non-switching `Err` arm of `finish_env_load` changes.

## Standing constraints
- Worktree via `./.claude/scripts/wt-new.sh . 121 failed-reload-clears-secrets`, `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-121`.
- Secrets never logged or in docs, including the notice text; the test uses the marker `SECRETVAL`.
- Tests never touch the real OS keychain: use the injected `MemoryStore` stores from task 115's harness, or a `wisp-test-<uuid>` service and delete only entries the test created.
- A temp `WISP_DATA_DIR` for every test.
- Never run `pkill` or `killall`; the user may be running Wisp.
- No Windows-only code is expected; the Windows CI job (54-CI) lands later.

## Out of scope
- crates/app/src/vault.rs (owned by task 120 in this wave).
- 99-AC52 (history routing by cached id), AC72 (no-data-dir bindings), AC75 (test `Workspace::new` and the real data dir): deferred.
- Any change to the switching path or to the notice wording beyond what AC1 needs.

## PR #61 review round 1 (HEAD 7ce5d00)
- 61-1 (Medium, security) workspace.rs:1843: re-selecting the active environment takes the switching path, so a failed load restores the refused secrets and the old Loaded status. Fixed: same name is a reload.
- 61-2 (Medium, security) workspace.rs:1843: a failed switch while a reload of the previous env is pending restores old secrets with status stuck at Loading. Fixed: restore with secrets cleared and re-run its reload.
- 61-3 (follow-up) test skips AC1's resolution clause and plain-vars-kept claim.
- 61-4 (follow-up) a cleared secret name can fall through to a same-named env or collection var.
