# 24 — Implement draft/dirty diffing, disk-change decisions and URL⇄params sync

**Repo:** .
**Depends on:** 21, 22
**Owns:**
- crates/app/src/draft.rs
- crates/app/src/url_sync.rs

## Goal
The pure logic behind the dirty marker, the "changed on disk" banner and the live URL bar. A tab is dirty exactly when its draft differs from its saved snapshot; a watcher event turns into one of reload / banner / deleted / ignore; and the URL bar text, `[[params]]` and `[path]` stay consistent in both directions.

## Context
- crates/app/src/draft.rs, crates/app/src/url_sync.rs — frozen signatures from task 21
- crates/core/src/request.rs:108-138 — `Request` and its `Default`
- `wisp_core::template::{var_refs, path_params, query_start}` — from task 22
- Spec section 2 "Query params: `[[params]]` is the single source of truth; the URL bar displays and edits them live" and "UI shows one input per detected param and keeps `[path]` in sync"
- PLAN carry-forward (14): reload must re-check the disk and tolerate `Changed` for non-file paths
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-24`.

## Acceptance
- [ ] `is_dirty(Some(s), d)` is `s != d`; `is_dirty(None, d)` is `d != blank_request()`; `blank_request()` is `Request::default()` with `name = "Untitled"`.
- [ ] `dirty_sections` sets exactly the fields whose `Request` members differ (a test flips each member in turn and checks only that flag is set); with `saved = None` every member that differs from `blank_request()` is flagged.
- [ ] `on_disk_change` returns: `Ignore` when the disk copy equals `saved` (our own save echoing back); `Reload` when not dirty and the disk differs; `ShowBanner` when dirty and the disk differs; `MarkDeleted` when `disk` is `None`; `ShowBanner` when `disk` is `Some(Err(_))` and dirty, `Ignore` when it is `Some(Err(_))` and clean (a half-written or broken file never replaces a draft). Each branch has a test.
- [ ] `display_url(url, params)` returns `url` followed by `?name=value&…` for enabled params only, raw (no percent-encoding, `{{vars}}` untouched); an empty value renders as `name` with no `=`, and `apply_url_text` reads both `name` and `name=` as an empty value. Returns `url` unchanged when no param is enabled.
- [ ] `apply_url_text(text, params)` splits at `template::query_start`, returns the URL without the query and the new param list: enabled params are rebuilt from the text in order, keeping each existing entry's identity when its name matches; disabled params are kept at their original relative positions. Tests: `?` inside `{{a?b}}` not split; `#fragment` stays in the URL; empty query `x?` gives no params; duplicate names kept; `display_url(apply_url_text(t, p))` reproduces `t` for normalised inputs.
- [ ] `sync_path_vars(url, path)` returns one `Var` per `template::path_params(url)` name in URL order, keeping existing values for names still present and dropping removed names; duplicate names in the URL yield one entry.
- [ ] Neither module imports `gpui` or `gpui_component`; `cargo test -p wisp-app` passes, and `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- Wiring these functions into views or the workspace (tasks 28, 31).
- Percent-encoding (that is `resolve`'s job).
- Any file other than the two owned.
