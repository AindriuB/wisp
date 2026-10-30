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
- crates/storage/src/codec.rs, crates/storage/src/fs.rs — including `atomic_write_private` (from 13)
- crates/core/src/request.rs — `Environment`, `Var` (from 10)
- docs/architecture.md — "Boundaries" (secrets); docs/conventions.md#logging

## Shape
- `list_environments(root) -> Result<Vec<String>, StorageError>` returns names from `environments/*.toml`, excluding `*.secrets.toml`, sorted. A missing `environments/` gives an empty list.
- `load_environment(root, name) -> Result<Environment, StorageError>` takes `vars` from `<name>.toml` and `secrets` from `<name>.secrets.toml`. A missing secrets file means empty secrets.
- `save_environment(root, &Environment) -> Result<(), StorageError>` edits both files in place through the codec.
  - A secrets file is written only when `secrets` is non-empty or the file already exists.
  - A secrets file is always written with `fs::atomic_write_private` (from 13), never with `atomic_write`.
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
- [ ] `rg -n 'atomic_write\b' crates/storage/src/environment.rs` shows no call writing a `*.secrets.toml` path; secrets writes go through `fs::atomic_write_private`.
- [ ] `rg -n 'tracing::.*(value|secret)' crates/storage/src/environment.rs` shows no call logging a var value.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- An OS keychain (not in v1)
- Edits to the root `.gitignore`
- Choosing the active environment, and the env switcher (M2)

## Review round 1 — findings

Reviewed HEAD: 5ba35e247b90092efdca2988065a95fdab9b33c5

- [P2] crates/storage/src/environment.rs:24 — `check_name` accepts a name ending in `.secrets`, so environment `dev.secrets` gets `dev.secrets.toml` as its plain file — the same file as `dev`'s secrets file (data loss; secrets written through plain `atomic_write`; `load_environment("dev.secrets")` exposes `dev`'s secrets as plain vars).
  route: implementer — reject names ending in `.secrets` (case-insensitively) in `check_name`.
  input: save `dev{vars:[host=a], secrets:[token=s3cret]}`, then save `Environment{name:"dev.secrets", vars:[x=y]}` → `load_environment(root,"dev").secrets == [("x","y")]`.
- [P2] crates/storage/src/environment.rs:25 — `check_name` lets `:` through; on Windows (a v1 platform) `C:evil` makes `Path::join` drop the root and `dev:x` writes an alternate data stream.
  route: implementer — use an allowlist for names (e.g. ASCII letters, digits, `-`, `_`, `.` not leading, no `..`), rejecting `:`, `\`, `/`, control chars and Windows reserved names.
- [NOTE] (optional) crates/storage/src/environment.rs:55-60 — `list_environments` can return names that `load_environment` rejects (`a..b`) and includes a directory named `x.toml`; make it skip non-files and names that fail `check_name`, keeping the two consistent.
  route: implementer
