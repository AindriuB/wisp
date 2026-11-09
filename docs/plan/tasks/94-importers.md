# 94 — Add curl, Postman v2.1 and OpenAPI 3 importers

**Repo:** .
**Depends on:** 91
**Owns:**
- crates/storage/src/import/** (`mod.rs`, `curl.rs`, `postman.rs`, `openapi.rs`, `yaml.rs`, `write.rs`)
- crates/storage/tests/import.rs (new)
- crates/storage/tests/fixtures/import/** (new)

## Goal
Pure importers turn a curl command, a Postman v2.1 collection or an OpenAPI 3
document (JSON or YAML) into one `ImportResult`, and a writer puts that result
into an open collection through the existing storage write paths. Nothing is
dropped silently: every unsupported construct becomes a warning.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#1-importers-storageimportcurl-postman-openapi`
- `crates/core/src/request.rs:12-180` — `Request`, `Body`, `KeyValue`, `Var`, `Auth`, `CollectionDefaults`
- `crates/storage/src/ops.rs` — create/save paths and name checks to call (do not edit); `crates/storage/src/request.rs` `save_request`; `crates/storage/src/environment.rs` env save (secrets file 0600)
- `crates/storage/src/collection.rs` `load/save_collection_defaults` for collection vars
- `docs/architecture.md#on-disk-collection-format` — `{{var}}` and `{name}` syntax
- Worktree `/Users/Andrew/workspace/wisp/.worktrees/wisp/94-importers`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-94`

## Contract
- `ImportResult { name, items: Vec<ImportItem>, collection_vars: Vec<Var>, environments: Vec<ImportedEnv>, warnings: Vec<ImportWarning> }`; `ImportItem::{Folder { name, items }, Request { name, request: Request }}`; `ImportedEnv { name, vars, secrets }`; `ImportWarning { at: String /* item path */, message }`.
- `curl::parse(&str)`, `postman::parse(&[u8])`, `openapi::parse(&[u8])` return `Result<ImportResult, ImportError>`; `detect(&[u8]) -> Option<Format>`. `ImportError` is a `thiserror` type local to `import` (do not edit `error.rs`).
- `write(root, folder_rel, &ImportResult, secrets_env: &str) -> Result<WriteReport, ImportError>`; `WriteReport { requests, folders, environments, renamed: Vec<(String, String)> }`. Name collisions with existing nodes or environments get " 2", " 3", ... suffixes and are listed in `renamed`.
- Hostile-input caps: input at most 10 MiB, nesting depth at most 64, at most 5,000 requests, YAML alias expansion bounded (at most 100,000 nodes after expansion). Exceeding any is `ImportError::TooLarge`/`TooDeep`, never a panic or unbounded allocation.

| Source | Mapping |
|---|---|
| curl | quoting (single, double, `\` continuations, `$'..'`); `-X`, `-H`, `-d`/`--data`/`--data-raw`/`--data-binary`/`--data-urlencode` (POST default; JSON body when Content-Type is JSON), `-F` (multipart; `@file` parts become File parts), `-u` (Basic auth), `-b` (Cookie header), `-k` (verify off), `-L` (follow redirects); `-G` moves data to the query. Other flags: warning |
| Postman v2.1 | items and folders nested; `:name` path segments become `{name}`; `{{var}}` kept; raw/urlencoded/formdata bodies; auth basic, bearer, apikey (others, including digest and awsv4: warning, auth Inherit); literal auth values become `{{<request>_<field>}}` refs with values in `ImportedEnv { name: secrets_env }.secrets`; collection `variable` to collection vars; `event` scripts and pre-request scripts: one warning per item; GraphQL bodies: warning. Schema not v2.1: error |
| OpenAPI 3.0/3.1 | JSON when the first non-space byte is `{`, else YAML via the task-91 crate; one request per operation, foldered by first tag; name from `summary`, else `operationId`, else `METHOD path`; URL `{{baseUrl}}<path>`; each `servers` entry becomes an environment with `baseUrl` (server variables replaced by their defaults); query and header params from examples or empty; JSON request body from `example`/`examples`; local `#/components/...` `$ref`s resolved with cycle detection; external `$ref`: warning. Swagger 2.0: error |

## Acceptance
- [ ] AC1 curl: at least 12 table-driven cases covering every flag in the table, quoting and continuations; an unknown flag yields a warning, not an error.
- [ ] AC2 Postman: a fixture with nested folders, `:id` path, three body modes, basic and bearer auth with literal values, collection variables and a pre-request script yields the expected tree, `{id}` paths, secrets only in the secrets env, collection vars, and one warning per script.
- [ ] AC3 OpenAPI: a JSON and a YAML fixture of the same API give equal `ImportResult`s; two `servers` give two environments; a `$ref` cycle terminates; an external `$ref` gives a warning.
- [ ] AC4 Hostile fixtures (11 MiB input, depth 65 JSON, YAML alias bomb, 5,001 operations) each return the matching error within 2 s and without panicking.
- [ ] AC5 `write` into a temp collection, then `open_collection` and `resolve` with the imported environment, succeeds for each fixture; a second identical import yields " 2" names and `renamed` entries; no literal secret from the Postman fixture appears in any non-`*.secrets.toml` file.
- [ ] AC6 `serde_yaml` is not used (only the task-91 YAML crate).
- [ ] AC7 `fmt`, `clippy -D warnings` and the storage suite pass.

## Checks
- `import-unit` command — AC1, AC2, AC3, AC4. cargo test writes no JUnit XML here; asserts at least 25 passing.
  run: `n=$(cargo test -p wisp-storage --lib -- import:: 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 25`
- `import-write` command — AC5 (at least 4 passing)
  run: `n=$(cargo test -p wisp-storage --test import 2>&1 | sed -nE 's/^test result: ok\. ([0-9]+) passed.*/\1/p' | head -1); test "${n:-0}" -ge 4`
- `no-serde-yaml` command — AC6
  run: `! rg -n 'serde_ya?ml' crates/storage`
- `suite-and-lint` command — AC7
  run: `cargo test -p wisp-storage && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

## Out of scope
- The Import dialog and command-bar entry (task 96).
- Mapping Digest/SigV4/OAuth2 auth from Postman or OpenAPI security schemes (warning only).
- Postman script translation, Postman v2.0, Swagger 2.0, multi-file OpenAPI, Insomnia/HAR.
- Edits to `ops.rs`, `request.rs`, `environment.rs`, `collection.rs` or `error.rs`; if a write path is missing, stop and report.

## Notes
- **Size: uncertain.** The YAML crate is the largest unmeasured wave-1 addition (spec estimate +60-120 KB on Windows). Task 98 measures it; if the gate fails, YAML is what gets dropped or replaced.
