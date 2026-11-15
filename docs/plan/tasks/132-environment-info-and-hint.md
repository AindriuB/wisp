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
- [ ] AC1 `pub fn environment_info(root: &Path, name: &str) -> Result<EnvironmentInfo, StorageError>` in `environment.rs`. `EnvironmentInfo { name, store: SecretsMode, var_names: Vec<String>, secret_names: Vec<String>, suggest_confirm: SuggestConfirm }`, where `SecretsMode` is an info-only enum such as `{ File, Store(StoreKind), Unknown(String) }` (see Decisions, 2026-11-15): `store` from `secrets = "<kind>"` (file when absent; an unrecognised kind string is `Unknown`, not an error); `secret_names` from the secrets file's keys in file mode and from `secret_names` in store mode. It takes no `SecretStore` and constructs none.
- [ ] AC2 `EnvironmentInfo` holds no variable or secret value. Canary test: an environment whose plain variable and file secret carry unique markers; neither marker is in `{:?}`/`{:#?}` of the result.
- [ ] AC3 `SuggestConfirm { No, Yes, Malformed }`: absent or `false` is `No`, `true` is `Yes`, any other type is `Malformed`; it is never an error and loading still succeeds.
- [ ] AC4 `LoadedEnvironment` gains `pub suggest_confirm: SuggestConfirm`, set at its one construction site from the same parse function `environment_info` uses.
- [ ] AC5 Tests cover present, absent, `false`, wrong type (string, integer) for both `environment_info` and the vault loader, using `MemoryStore`/`NoStores` only; no test constructs a keychain store.
- [ ] AC6 An invalid environment name or a path outside `environments/` is refused through the existing `check_name` path (test).
- [ ] AC7 Existing behaviour of `load_environment`, `save_environment` and the vault loader is unchanged; saving an environment preserves an existing `suggest_confirm` key (test).
- [ ] AC8 An environment with `secrets = "keepassxc"` and `secret_names` returns Ok with `store == Unknown("keepassxc")` and those `secret_names`; one with `secrets = 5` returns `StorageError::Invalid { key: "secrets", .. }` (tests).

## Checks
- `storage-env` command — AC1-AC8
  run: `cargo test -p wisp-storage --test vault --test environment_info && cargo test -p wisp-storage --lib environment`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-store` read — AC1, AC5
  how: `environment_info` has no store parameter and calls no `keychain::` item; tests use only `MemoryStore`/`NoStores`.

## Out of scope
- `crates/storage/src/lib.rs`, `collection.rs`, `validate.rs` (task 133); `ops.rs` (task 134).
- The guard itself, `Settings.confirm_send`, the notice UI: waves 2 and 3.
- `crates/app`: nothing there constructs `LoadedEnvironment`; if a literal is found, stop and report.

## Decisions
- 2026-11-15, orchestrator, code review round 1 of PR #82. Follows the spec's additive environment keys and the app loader's existing tolerance of unknown kinds. `EnvironmentInfo` reports the secrets mode as an info-only enum with an `Unknown(String)` case (e.g. `SecretsMode::{File, Store(StoreKind), Unknown(String)}`), so `environment_info` returns Ok for an environment written by a newer Wisp with an unrecognised `secrets = "<kind>"`. A wrongly typed `secrets` value (e.g. `secrets = 5`) stays `StorageError::Invalid { key: "secrets" }`. The unknown kind string is capped in length when stored.

## Review round 1 — findings (HEAD c9617d1)
- [P2] crates/storage/src/environment.rs:195-201 — an unknown store kind returns Invalid, so `wisp-cli inspect` can't describe a newer Wisp's environment. Settled by the 2026-11-15 Decisions entry (AC1, AC8).
- [P2] crates/storage/tests/environment_info.rs:82-88 — `bad_names_refused` passes without `check_name` (the targets don't exist, so the error is NotFound). Fix: create the file the escaping name would reach (e.g. `<root>/x.toml` for `"../x"`) and assert `StorageError::Invalid { key: "name", .. }`.
- [NOTE] A symlinked `environments/` is still followed for the plain file (pre-existing, documented). Relevant to the CLI symlink-escape fixture.
