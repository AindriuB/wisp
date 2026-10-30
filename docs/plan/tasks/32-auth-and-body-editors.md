# 32 — Build the Auth and Body sub-tab editors

**Repo:** .
**Depends on:** 21, 26, 28, 29
**Owns:**
- crates/app/src/ui/request/auth.rs
- crates/app/src/ui/request/body.rs

## Goal
The Auth sub-tab edits `none | inherit | basic | bearer | apikey`, showing for `inherit` what is actually inherited; the Body sub-tab edits every v1 body type: none, JSON, text, XML, form, multipart and file. Both write into the tab's draft on every edit and resync on reload.

## Context
- crates/app/src/ui/request/auth.rs, body.rs — frozen constructors from task 21 (already mounted by `RequestPane`)
- crates/core/src/request.rs:26-90 — `Auth`, `ApiKeyLocation`, `TextBody` (keep `sidecar` untouched), `Part`, `PartValue`, `Body`
- crates/app/src/tab.rs — `edit`, `TabEvent::DraftReplaced` (task 28); crates/app/src/workspace.rs — `scope_for` (task 28)
- crates/app/src/ui/kv.rs — `KvTable` for form bodies (task 29); crates/app/src/pretty.rs — `pretty_json` (task 26)
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/input/editor.rs — multi-line `Editor`; src/select.rs; `Input::mask_toggle`
- docs/conventions.md#logging — secret values never logged
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-32`.

## Acceptance
- [ ] Auth: a type select with None, Inherit, Basic, Bearer, API key. Basic shows username and a masked password; Bearer a masked token; API key shows name, masked value and a Header/Query location select. Every secret field uses `mask_toggle`.
- [ ] Inherit shows a read-only line "Inherits <type> from <folder name|collection>" computed by a GPUI-free function in `auth.rs` that walks `Scope` nearest-first past `Inherit` entries (unit tests: folder override, collection fallback, all-inherit → None); secret values are never rendered there.
- [ ] Switching auth type keeps the previous type's values in memory for the life of the tab, so switching back restores them; only the selected variant is written to the draft.
- [ ] Body: a type select with None, JSON, Text, XML, Form, Multipart, File. JSON/Text/XML use a multi-line editor bound to `TextBody.content`, preserving `sidecar`; switching among these three keeps the content. JSON shows a non-blocking "invalid JSON at offset N" line when it fails to parse (reuse `crate::pretty::pretty_json` for validation) and a Format button that replaces the content with its pretty form.
- [ ] Form: a `KvTable` with toggles bound to the `Vec<KeyValue>`. Multipart: rows of enabled checkbox, name, Text/File toggle, and either a text input or a file path with a Choose… button (`prompt_for_paths`), plus delete and an empty trailing row. File: a path field with Choose…; a missing file shows a warning line.
- [ ] A small GPUI-free helper in `body.rs` converts between body types (which content survives a switch) and has unit tests.
- [ ] Both editors resync from the draft on `TabEvent::DraftReplaced` and only then; one user edit produces one `tab.edit`.
- [ ] Hand check (orchestrator, window frontmost) on the `my-api` fixture copy: `create-user` shows its sidecar JSON; editing and ⌘S writes the sidecar file, not inline content (`git diff` of the copy); switching auth to Bearer and back to Inherit restores the inherited line; a multipart file part sends (checked against a local `python3 -m http.server`-style echo or by status line).
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- Syntax colouring in the body editor.
- Digest, OAuth2, AWS SigV4 auth (spec: not in v1).
- Editing folder or collection auth from the UI.
- GraphQL bodies.
