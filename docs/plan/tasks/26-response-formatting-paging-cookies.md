# 26 — Implement response pretty-printing, body paging and Set-Cookie parsing

**Repo:** .
**Depends on:** 21
**Owns:**
- crates/app/src/format.rs
- crates/app/src/pretty.rs
- crates/app/src/pager.rs
- crates/app/src/cookies.rs

## Goal
Pure logic for the response viewer. JSON is pretty-printed by re-indenting its token stream (preserving key order and number text, no value tree, so memory stays near input size); bodies are read in fixed pages with UTF-8 boundaries carried across pages; and the Cookies tab parses this response's `Set-Cookie` headers, since the engine has no cookie jar in v1.

## Context
- crates/app/src/format.rs:1-77 — existing status-line and size helpers and tests
- crates/app/src/pretty.rs, pager.rs, cookies.rs — frozen signatures from task 21
- crates/core/src/body.rs:150-230 — `BodyStore::read(handle, offset, len)`
- RFC 6265 section 5.2 — Set-Cookie parsing rules
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-26`.

## Acceptance
- [ ] `pretty_json` re-indents with two spaces, one key per line, `": "` after keys, empty `{}`/`[]` kept on one line; key order, number spelling (`1.0`, `1e5`, `-0`) and string escapes are byte-identical to the input; it builds no `serde_json::Value` (`rg -n 'Value' crates/app/src/pretty.rs` finds none) and does not add `serde_json` to the app crate.
- [ ] `pretty_json` returns `Err(PrettyError{offset,..})` for truncated input, trailing garbage, unbalanced brackets and invalid literals; accepts a UTF-8 BOM; a 5 MiB generated document is pretty-printed in a test without stack overflow (iterative, nesting depth tested to 10,000).
- [ ] `content_kind` uses `Content-Type` (case-insensitive name; `application/json`, `+json` suffix, `text/xml`, `application/xml`, `+xml`, `text/html`, `text/*`) and falls back to sniffing the first bytes (`{`/`[` after whitespace → Json, `<` → Xml/Html, NUL byte or invalid UTF-8 → Binary).
- [ ] `page_count` and `page` split a body into `PAGE_SIZE` pages (`page_count(0) == 0`, last page short, out-of-range index is `None`).
- [ ] `Utf8Decoder::push` never emits a replacement char for a multibyte char split across pushes (test splits 2-, 3- and 4-byte chars at every offset); genuinely invalid bytes become U+FFFD; `finish` flushes a dangling partial sequence as U+FFFD.
- [ ] `parse_set_cookies` reads every `Set-Cookie` header (case-insensitive), splits name/value at the first `=`, trims, parses `Domain`, `Path`, `Expires`, `Max-Age` (integer, else ignored), `Secure`, `HttpOnly`, `SameSite` case-insensitively, ignores unknown attributes, and skips a header with an empty name or no `=`.
- [ ] `format.rs` keeps its existing tests and adds `pub fn format_duration(d: Duration) -> String` (`<1 s` as `N ms`, otherwise `N.NN s`) and `pub fn status_text(status: u16, reason: Option<&str>) -> String`, with tests; `BODY_LIMIT`, `body_text` and `completed_line` stay unchanged (task 28 is deleting `view.rs` in parallel; task 35 removes whatever is then unused).
- [ ] No module imports `gpui` or `gpui_component`; `cargo test -p wisp-app` passes, and `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- XML or HTML pretty-printing (shown Raw in v1).
- Syntax colouring of the pretty output (would need `tree-sitter` features; a user decision).
- A cookie jar or sending cookies back (engine change, user decision).
- Rendering (task 33).

### Amendment (2026-10-30, pretty-print memory bound — orchestrator)
Decision: `pretty_json` keeps memory near input size on hostile input by (1) capping indentation at 32 levels — deeper lines get the same 64-space indent, nesting still handled iteratively — and (2) an output budget of `max(1 MiB, 4 × input length)`: if the output would exceed it, return `Err` so the view falls back to Raw. Reason: code review found `2*depth` spaces per line make output quadratic in nesting depth (`"["*32768 + "]"*32768`, a 64 KiB body, allocated 2 GiB; a 5 MiB body would abort). The "depth 10,000 handled iteratively" criterion stands; its output now stays bounded.
Additional acceptance: the 64 KiB `[`×32768 input returns within the budget (Err or output ≤ 4× input) with no large allocation; a normal nested document ≤ 32 levels is indented exactly as before; a document deeper than 32 levels indents at most 64 spaces.

## Review round 1 — findings

Reviewed HEAD: 0e85c9dc536f65ef76251b51a659dc44bf389af0

- [P1] crates/app/src/pretty.rs:193 — `newline` writes `2*depth` spaces per line, so output is quadratic in nesting depth; a 64 KiB hostile body allocates 2 GiB. Settled by Amendment (2026-10-30, pretty-print memory bound) — implement it.
  route: implementer
  input: `"[" * 32768 + "]" * 32768` → 2,147,483,648-byte String, ~2.35 GB RSS.
- [P3] crates/app/src/cookies.rs:54 — `Max-Age` uses `i64::parse`, accepting a leading `+` (RFC 6265 §5.2.2: ignore unless first char is a digit or `-`), and drops values past i64 range instead of saturating.
  route: implementer
  input: `Max-Age=+5` → Some(5), expected None; `Max-Age=99999999999999999999` → None, expected i64::MAX.
- [NOTE] (optional) crates/app/src/pretty.rs:63 — `sniff_kind` doesn't skip a UTF-8 BOM while `pretty_json` accepts one.
