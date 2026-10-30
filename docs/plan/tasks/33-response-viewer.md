# 33 — Build the response viewer: status line, Body Pretty/Raw, Headers and Cookies

**Repo:** .
**Depends on:** 21, 26, 28
**Owns:**
- crates/app/src/ui/response/**

## Goal
Show the active tab's response: a status line (status, time, size, progress while sending), and Body, Headers and Cookies sub-tabs. Body Pretty is computed on GPUI's background executor for JSON under the configured limit (about 5 MB); everything else, and anything larger, is Raw, read page by page from the `BodyHandle` and rendered through a virtualised line list so memory stays near the visible window.

## Context
- crates/app/src/ui/response/** — frozen constructors from task 21 (`ResponsePane` mounts `BodyPane`, `HeadersPane`, `CookiesPane`)
- crates/app/src/tab.rs — `ResponseState`, `Phase`, `response_tab`, `body_view`, `TabEvent::ResponseChanged` (task 28)
- crates/app/src/workspace.rs — `bodies()`, `settings.pretty_limit` (task 28)
- crates/app/src/pretty.rs, pager.rs, cookies.rs, format.rs (task 26)
- crates/core/src/body.rs:184-230 — `BodyStore::read`, `is_spilled`, `release`
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/virtual_list.rs, src/table/
- docs/architecture.md#budgets — after-request RSS ≤ 128 MB
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-33`.

## Acceptance
- [ ] Status line: `Idle` shows nothing; `Sending` shows a spinner, received bytes (`format_size`) and a Cancel button (`Workspace::cancel`); `Done` shows `status_text · format_duration(total) · format_size(size)` with the status coloured by class (2xx/3xx/4xx/5xx), and TTFB in a tooltip; `Failed` shows the error kind in words and the message; `Cancelled` shows "Cancelled".
- [ ] Body: a Pretty|Raw toggle stored in `tab.body_view`. Pretty is offered only when `content_kind` is Json and `size ≤ settings.pretty_limit`; it runs `pretty_json` on `cx.background_executor()` over the bytes read from the handle, shows a spinner meanwhile, caches the result per `BodyId`, drops it when the body changes, and falls back to Raw with the parse error on failure. Over the limit, the toggle is disabled with a tooltip saying why.
- [ ] Raw reads pages with `pager::page` + `BodyStore::read` on the background executor, decodes with `Utf8Decoder`, and appends lines to a virtualised list as the user scrolls near the end, holding at most `RAW_DISPLAY_CAP` bytes of text; beyond the cap a line reads "Showing the first 4 MB — Save to read the rest". Binary bodies show "Binary body, N bytes" instead of text.
- [ ] A Save button writes the whole body (page by page from the handle, never loading it into one buffer) to a path chosen with `prompt_for_new_path`; errors show as a notice.
- [ ] Headers: a two-column selectable table of `ResponseState::headers` in received order, with the HTTP version shown above it.
- [ ] Cookies: a table (name, value, domain, path, expires/max-age, flags) from `parse_set_cookies(headers)`, with a one-line note "Cookies from this response only; Wisp does not store or send cookies in this version." and an empty-state line when there are none.
- [ ] No view keeps a second copy of the whole body: the pretty string and the raw window are the only text held, and both are dropped on `ResponseChanged` with a new body or on tab close.
- [ ] Hand check (orchestrator, window frontmost): a JSON response is pretty with key order kept; a 20 MB response (local `python3 -m http.server`) opens Raw, scrolls, and shows the cap line; Save writes a file whose `shasum` matches the server file; a response with two `Set-Cookie` headers shows two rows.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- Image or HTML preview, syntax colouring (spec: not in v1).
- A cookie jar (engine change; user decision).
- Search within the body.
- Phase timings beyond TTFB and total.
