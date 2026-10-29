# 19 — Read and write environments and their secrets files

**Repo:** .
**Depends on:** 13
**Owns:**
- crates/storage/src/environment.rs

## Goal
Add listing, loading and saving of environments. Plain vars live in
`environments/<name>.toml` and secret vars in `environments/<name>.secrets.toml`,
both with the same `[vars]` shape. A secrets file must never be committed by
accident, and its values must never reach a log.

## Context
- crates/storage/src/codec.rs, crates/storage/src/fs.rs (from 13)
- crates/core/src/request.rs — `Environment`, `Var` (from 10)
- docs/architecture.md — "Boundaries" (secrets); docs/conventions.md#logging

## Shape
- `list_environments(root) -> Result<Vec<String>, StorageError>` returns names from `environments/*.toml`, excluding `*.secrets.toml`, sorted. A missing `environments/` gives an empty list.
- `load_environment(root, name) -> Result<Environment, StorageError>` takes `vars` from `<name>.toml` and `secrets` from `<name>.secrets.toml`. A missing secrets file means empty secrets.
- `save_environment(root, &Environment) -> Result<(), StorageError>` edits both files in place through the codec.
  - A secrets file is written only when `secrets` is non-empty or the file already exists.
  - A var that moves from plain to secret, or back, is removed from the other file.
  - Before any secrets file is written, `environments/.gitignore` is made to contain a `*.secrets.toml` line. It is created, or the line is appended, and an existing file's other lines are kept.
- **Logging.** tracing records environment names and var names only, never values.

## Acceptance
- [ ] Unit tests in `environment.rs`, using `tempfile` directories, show the following:
  - `dev.toml` plus `dev.secrets.toml` load into `vars` and `secrets` respectively.
  - `list_environments` returns `["dev", "prod"]` for `dev.toml`, `dev.secrets.toml` and `prod.toml`.
  - Saving an environment with a secret creates `environments/.gitignore` containing `*.secrets.toml`. An existing `.gitignore` with `foo` keeps `foo` and gains the line once, even after two saves.
  - Moving `token` from vars to secrets removes it from `dev.toml` and adds it to `dev.secrets.toml`.
  - A `dev.toml` with comments, saved with one var changed, differs by exactly one line.
  - Saving with no secrets and no existing secrets file creates no secrets file.
- [ ] `rg -n 'tracing::.*(value|secret)' crates/storage/src/environment.rs` shows no call logging a var value.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- An OS keychain (not in v1)
- Edits to the root `.gitignore`
- Choosing the active environment, and the env switcher (M2)
