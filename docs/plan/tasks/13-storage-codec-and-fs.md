# 13 — Add the storage TOML codec and atomic file writes

**Repo:** .
**Depends on:** 10
**Owns:**
- crates/storage/src/codec.rs
- crates/storage/src/fs.rs

## Goal
Add the shared pieces that request, folder, collection and environment files
all use. The codec reads and writes `[vars]`, `[[headers]]`/`[[params]]`-style
key-value arrays and `[auth]` on a `toml_edit::DocumentMut`. It edits in place,
so comments and formatting survive. The fs helpers do typed reads and atomic
writes, using a temp file in the same directory and then a rename. Tasks 17, 18
and 19 build on both.

## Context
- crates/storage/src/error.rs, crates/storage/src/lib.rs — `StorageError`, `TEMP_PREFIX` (from 10)
- crates/core/src/request.rs — `Var`, `KeyValue`, `Auth` (from 10)
- docs/conventions.md#toml — writes go through `toml_edit`
- docs/plan/specs/2026-10-29-http-client-v1.md — section 2 file examples

## Shape (crate-visible API)
- `fs::read_to_string(path) -> Result<String, StorageError>`
- `fs::atomic_write(path, bytes) -> Result<(), StorageError>`, which writes `<dir>/<TEMP_PREFIX><name>.<n>` and then renames it over `path`
- `codec::parse(path, text) -> Result<DocumentMut, StorageError>`, which maps syntax errors to `Parse` with the line number
- `codec::read_vars(doc_or_table, key, path)` and `codec::write_vars(..., &[Var])`. Values must be strings. Anything else is `Invalid { key: "vars.<name>" }`.
- `codec::read_key_values(.., key, path)` and `codec::write_key_values(.., key, &[KeyValue])` for arrays of tables with `name`, `value` and `enabled`. A missing `enabled` reads as `true`, and `enabled` is written only when it is `false`.
- `codec::read_auth(.., default: Auth, path)` and `codec::write_auth(.., &Auth)` for `[auth] type = none|inherit|basic|bearer|apikey`, with fields `username`, `password`, `token`, `name`, `value`, and `in = header|query`. An absent `[auth]` reads as `default`.
- `codec::read_str`, `codec::read_opt_str`, `codec::write_str` for scalar keys
- **Writer rules.** An entry whose value is unchanged is not touched. A changed scalar keeps its key's decor, meaning surrounding whitespace and comments. A removed entry is removed. A new entry is appended. An empty list or table removes its key when it was absent before.

## Acceptance
- [ ] Unit tests in `codec.rs` show the following:
  - Parsing a fixture that has comments, blank lines and odd spacing, then writing the same values back, produces byte-identical text.
  - Changing one var value changes exactly one line of the output, checked with a line diff.
  - Toggling one header's `enabled` to `false` changes only that header's table.
  - Each `Auth` variant round-trips.
  - `type = "digest"` yields `Invalid { key: "auth.type" }`.
  - `[vars] port = 8080` yields `Invalid { key: "vars.port" }`.
  - Broken TOML yields `Parse`, and its message contains the line number.
- [ ] Unit tests in `fs.rs`, using `tempfile` directories, show the following:
  - `atomic_write` replaces an existing file's contents, and afterwards no `TEMP_PREFIX` file is left in the directory.
  - Reading a missing file yields `Io` carrying the path.
- [ ] Nothing in `codec.rs` or `fs.rs` uses `toml::to_string` or a serde serialiser to write a file.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Request, folder, collection and environment file layouts (17, 18, 19)
- `.gitignore` handling (19, 15)
- fsync durability tuning beyond the rename
