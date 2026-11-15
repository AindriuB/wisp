# 132 — Add `environment_info` and the `suggest_confirm` hint in storage

**Repo:** .
**Depends on:** 129
**Owns:**
- crates/storage/src/environment.rs
- crates/storage/src/vault.rs
- crates/storage/tests/vault.rs
- crates/storage/tests/environment_info.rs (new)

## Goal
Give callers a value-free description of an environment (store kind, variable
names, secret names, hint) for `wisp-cli inspect` (2D) and read the optional
committed `suggest_confirm = true` hint into `LoadedEnvironment` for the
environment guard (3A). Neither path opens a secret store or returns a value.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "2b. Environment guard" (Committed hint, Tests) and "Feature 4" Commands (`inspect` environments)
- crates/storage/src/vault.rs:1-7 `secrets` and `secret_names` keys; :47-60 `StoreKind`; :255-269 `read_mode` (the precedent); :398-411 `LoadedEnvironment`; :448 its one construction site
- crates/storage/src/environment.rs:84 `list_environments`; :139 `secrets_file_names`; :146 `load_environment`
- Worktree `wt-new.sh . 132 environment-info-and-hint`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-132`

## Acceptance
- [ ] AC1 `pub fn environment_info(root: &Path, name: &str) -> Result<EnvironmentInfo, StorageError>` in `environment.rs`. `EnvironmentInfo { name, store: StoreKind, var_names: Vec<String>, secret_names: Vec<String>, suggest_confirm: SuggestConfirm }`: `store` from `secrets = "<kind>"` (file when absent); `secret_names` from the secrets file's keys in file mode and from `secret_names` in store mode. It takes no `SecretStore` and constructs none.
- [ ] AC2 `EnvironmentInfo` holds no variable or secret value. Canary test: an environment whose plain variable and file secret carry unique markers; neither marker is in `{:?}`/`{:#?}` of the result.
- [ ] AC3 `SuggestConfirm { No, Yes, Malformed }`: absent or `false` is `No`, `true` is `Yes`, any other type is `Malformed`; it is never an error and loading still succeeds.
- [ ] AC4 `LoadedEnvironment` gains `pub suggest_confirm: SuggestConfirm`, set at its one construction site from the same parse function `environment_info` uses.
- [ ] AC5 Tests cover present, absent, `false`, wrong type (string, integer) for both `environment_info` and the vault loader, using `MemoryStore`/`NoStores` only; no test constructs a keychain store.
- [ ] AC6 An invalid environment name or a path outside `environments/` is refused through the existing `check_name` path (test).
- [ ] AC7 Existing behaviour of `load_environment`, `save_environment` and the vault loader is unchanged; saving an environment preserves an existing `suggest_confirm` key (test).

## Checks
- `storage-env` command — AC1-AC7
  run: `cargo test -p wisp-storage --test vault --test environment_info && cargo test -p wisp-storage --lib environment`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-store` read — AC1, AC5
  how: `environment_info` has no store parameter and calls no `keychain::` item; tests use only `MemoryStore`/`NoStores`.

## Out of scope
- `crates/storage/src/lib.rs`, `collection.rs`, `validate.rs` (task 133); `ops.rs` (task 134).
- The guard itself, `Settings.confirm_send`, the notice UI: waves 2 and 3.
- `crates/app`: nothing there constructs `LoadedEnvironment`; if a literal is found, stop and report.
