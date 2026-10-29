# 17 — Load and save request files, with sidecar bodies

**Repo:** .
**Depends on:** 13
**Owns:**
- crates/storage/src/request.rs

## Goal
Add `load_request` and `save_request`, which convert between a request `.toml`
file (plus any sidecar body) and `wisp_core::request::Request`. Saving edits the
existing document in place through the codec. A user's comments, key order and
formatting survive a save that changes one field.

## Context
- crates/storage/src/codec.rs, crates/storage/src/fs.rs — the shared readers, writers and `atomic_write` (from 13)
- crates/core/src/request.rs — `Request`, `Body`, `TextBody`, `Part`, `RequestSettings` (from 10)
- docs/plan/specs/2026-10-29-http-client-v1.md — section 2, the request file example

## File layout
- Top-level keys are `name`, `method` (an uppercase string; anything unknown is `Invalid { key: "method" }`) and `url`. Tables are `[path]` (string values, read into `Vec<Var>` in file order), `[vars]`, `[[params]]`, `[[headers]]` and `[auth]` (an absent table means `Inherit`).
- `[settings]` holds `timeout_ms` (integer), `follow_redirects` and `verify_tls`. A key at its default value is omitted on write.
- `[body]` has `type = none|json|text|xml|form|multipart|file`.
  - For json, text and xml, the body is either `content = '''...'''` or `file = "<sidecar>"`. A sidecar is read into `TextBody.content` on load, and its name is kept in `TextBody.sidecar`.
  - `form` is `[[body.fields]]` with `name`, `value` and `enabled`.
  - `multipart` is `[[body.parts]]` with `name`, then `value` or `file`, then `enabled`.
  - `file` is `path = "..."`.
  - Relative paths are relative to the request file's directory. They become absolute on load. On save they are written relative when they lie under that directory and absolute otherwise.
  - An absent `[body]` means `None`.
- **Saving.**
  - An existing file is parsed, edited in place and written with `atomic_write`. A new file is written in the key order of the spec example.
  - With `sidecar = Some(name)`, the content goes to `<dir>/<name>` atomically and the TOML holds `file`, not `content`.
  - With `None`, the content goes inline. It is a multi-line literal string when the content has a newline and no `'''`, and an escaped basic string otherwise.
  - A sidecar the request no longer references is left on disk.

## Acceptance
- [ ] Unit tests in `request.rs`, using `tempfile` directories, show the following:
  - The spec's request example, used verbatim with its comments, loads to the expected `Request`. Saving it unchanged produces byte-identical text.
  - Changing `url` and saving changes exactly one line.
  - A JSON body with `sidecar = Some("create-user.body.json")` writes the sidecar, and reloading returns equal content.
  - Every `Body` variant round-trips through save and load with `PartialEq` equality.
  - A body content containing `'''` round-trips.
  - `method = "BREW"` yields `Invalid { key: "method" }`.
  - A missing sidecar yields `Io` whose path is the sidecar's.
  - A `Body::File` under the request directory is written as a relative path.
- [ ] No serde-based TOML serialisation is used to write the file.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Create, rename, move and delete of request files (left for M2's planner)
- Deciding automatically when a body becomes a sidecar (M2 sets `TextBody.sidecar`)
- Folder, collection and environment files (18, 19)
