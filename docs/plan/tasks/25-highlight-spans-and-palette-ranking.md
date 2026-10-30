# 25 — Implement variable highlight spans, hover text and palette ranking

**Repo:** .
**Depends on:** 21, 22
**Owns:**
- crates/app/src/highlight.rs
- crates/app/src/palette.rs

## Goal
Pure logic for two UI features. Highlighting turns a URL or field value into contiguous spans marking variables (resolved, dynamic, unresolved, error), path params and the query, plus the hover text for a variable. The palette builds its item list from the collection, environments and commands, and ranks them against a query with fuzzy matching.

## Context
- crates/app/src/highlight.rs, crates/app/src/palette.rs — frozen signatures from task 21
- `wisp_core::template` and `wisp_core::resolve::lookup_var` — from task 22
- crates/storage/src/collection.rs:17-50 — `CollectionTree`, `Node`, `RequestMeta`
- docs/conventions.md#logging — secrets never shown or logged in plain text
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-25`.

## Acceptance
- [ ] `url_spans` returns spans that are sorted, non-overlapping, on char boundaries and together cover `0..text.len()` exactly (property-style test over a table of inputs including multibyte text). Variables are `Var` with state from `lookup_var` (`Found`→`Resolved`, `Dynamic`, `Unresolved`, `Error`; an unterminated `{{` is `Error`); `{name}` in the path portion is `PathParam` with `filled` true when the request's `path` has a non-empty value; everything from `query_start` onward that is not a variable is `Query`.
- [ ] `text_spans` reports variables only (no path params, no query), same coverage guarantee.
- [ ] `unresolved_names` returns the distinct `Unresolved` variable names in first-seen order.
- [ ] `hover_text` formats `name = value · <source>` where source reads `request`, `folder <name>`, `environment <env>`, `secret <env>` or `collection`; for a secret it shows `name = •••• · secret <env>` and the test asserts the secret value is absent from the string; `Dynamic` reads `generated per send`; `Unresolved` reads `not defined — blocks sending`; values longer than 200 chars are cut with `…`.
- [ ] `palette::items` with `PaletteMode::All` lists every `Node::Request` (label = name, detail = relative path, `Request(path)`), then every `CommandId` (label in plain English, e.g. "Send request"); `Invalid` nodes are skipped. With `PaletteMode::Environments` it lists `No environment` (`Environment(None)`) then each name, and marks the active one in `detail` as `active`.
- [ ] `palette::filter` is case-insensitive subsequence matching over `label` and `detail`; an empty query returns all items in input order up to `limit`; contiguous runs, word-start and label-prefix matches outrank scattered matches (tests with fixed expected orders); ties keep input order; `positions` are char indices into `label` for highlighting; non-matches are excluded.
- [ ] Neither module imports `gpui` or `gpui_component`; `cargo test -p wisp-app` passes, and `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- Rendering spans or the palette (tasks 31, 34).
- Highlighting inside JSON/text body editors.
- Recent-item history or persisted palette state.
