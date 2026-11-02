# 47 — Raise or clearly report the response header-count limit

**Repo:** .
**Depends on:** none
**Owns:**
- crates/core/src/engine.rs
- crates/core/tests/engine.rs

## Goal
A response with more than 100 headers fails with a generic "error sending
request ... client error" because hyper's HTTP/1 parser stops at 100 headers by
default. Raise the limit to a generous bounded figure (1,000) if the pinned
reqwest exposes it; if it does not, report the failure with a clear message
that names the limit instead of the generic client error.

## Context
- docs/plan/HISTORY.md, heading `## 2026-11-02 — Scrollbars on the sidebar, response Headers and Cookies, and KvTable`: where the defect was found.
- docs/plan/PLAN.md, "Pane-scrollbar defects", "P2, header limit".
- crates/core/src/engine.rs:77-99: `ClientCache::get`, the only place a `reqwest::Client` is built.
- crates/core/src/engine.rs:434-466: `classify`, which maps a `reqwest::Error` to an `ErrorKind` and builds the message from the source chain.
- Pinned versions (Cargo.lock): reqwest 0.12.28 (`default-features = false`, `rustls-tls`, `http2`), hyper 1.12.0, hyper-util 0.1.21.
- Planner's survey, to be confirmed: reqwest 0.12.28 `src/` has no http1 max-headers setter (only `http2_max_header_list_size`, a byte limit, at `async_impl/client.rs:1622`). hyper 1.12.0 has `max_headers` on its conn builder (`src/client/conn/http1.rs:501`) and hyper-util's legacy client has one, but reqwest does not plumb either. hyper's default is `DEFAULT_MAX_HEADERS = 100` (`src/proto/h1/role.rs:31`); `httparse::Error::TooManyHeaders` becomes `Parse::TooLarge` (`src/error.rs:661`), displayed as "message head is too large" (`src/error.rs:525`). The same text also covers a head over hyper's byte buffer limit, so a message must not claim it is only the count.
- crates/core/tests/engine.rs:212-220: the `failure` helper. Tests use wiremock (`MockServer`) or `std::net::TcpListener` for raw servers (:223, :331).
- `ErrorKind` (crates/core/src/model.rs:86) is persisted in history (crates/storage/src/history.rs:28) and matched in `crates/app/src/ui/response/mod.rs:32`. This task does not add a variant.

## Acceptance
- [ ] `git diff --stat main` lists only files in the Owns set. `Cargo.toml` and `Cargo.lock` are unchanged; no dependency or feature is added.
- [ ] The report states whether reqwest 0.12.28 exposes an HTTP/1 max-headers setting, citing `path:line` in the registry source.
- [ ] If it does: `ClientCache::get` sets it to a named constant of 1,000, and a test in `crates/core/tests/engine.rs` against a local server that sends 150 response headers gets `Event::Headers` with all 150 and then `Event::Completed`.
- [ ] If it does not: `classify` detects the too-large-head cause and returns `ErrorKind::Other` with a message that starts `response headers too large (limit 100 headers` and does not contain "client error". A test in `crates/core/tests/engine.rs` against a local server sending 150 response headers asserts that kind and prefix. A second assertion or test shows a 90-header response still completes.
- [ ] No other error's kind or message changes: the existing `classify` tests and `words_in_the_url_do_not_change_the_error_kind` pass unchanged.
- [ ] The report gives one paragraph on memory: per-response cost of the chosen limit (headers are held in memory for the Headers and Cookies panes), and that any byte limit on the head stays as hyper's default.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-47 cargo fmt --all -- --check` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-47 cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-47 cargo test --workspace` passes. The report gives the pass count and the commit.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-47 cargo build --release -p wisp-app`. The report gives the binary size in bytes and the delta against 19,014,912 B, and the size is at or below 20,971,520 B.
- [ ] Orchestrator hand check: send to a local server returning 150 headers. Either the Headers tab lists all 150, or the response pane shows the "response headers too large (limit 100 headers..." message, not "client error".

## Out of scope
- A new `ErrorKind` variant (touches model, storage history and the app's `error_words`); file it as a follow-up if wanted.
- HTTP/2 header-list byte limits, and any other `ClientBuilder` setting.
- The `ClientCache` mutex residual and other engine carry-forwards in PLAN.
- Any `crates/app` change.
