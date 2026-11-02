# 50 — Upgrade reqwest to 0.13.5 and raise the HTTP/1 response-header limit to 1,000

**Repo:** `.`
**Depends on:** none
**Owns:**
- Cargo.toml (the workspace `reqwest` line; add a workspace `webpki-roots` and/or `rustls` line only if needed)
- Cargo.lock
- crates/core/Cargo.toml
- crates/core/src/engine.rs
- crates/core/tests/engine.rs

Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-50`.

## Goal
User decision (2026-11-02): move from reqwest 0.12.28 to 0.13.5 and call
`ClientBuilder::http1_max_headers(1000)`. Responses with up to 1,000 headers
then complete instead of failing at hyper's 100-header default. TLS stays as it
is today: the rustls **ring** provider (not aws-lc) and the bundled
**webpki-roots** (not rustls-platform-verifier or the OS trust store).

**Spike first, then measure.** Do the dependency bump and the builder change,
build the release binary (`cargo build --release -p wisp-app`), and record its
size. If it is larger than **20,971,520 B** (today 19,014,912 B), stop: commit
nothing and report `BLOCKED` with the measured size and what grew
(`cargo tree -d` or `cargo bloat` if it is available).

## Context
- Cargo.toml:18 — today `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "http2"] }`; crates/core/Cargo.toml:11 uses `reqwest.workspace = true`.
- crates/core/src/engine.rs:72-100 — `ClientCache::get` builds one client per `ClientSettings`: `use_rustls_tls()` (83), `danger_accept_invalid_certs(true)` when `!verify_tls` (85), `redirect::Policy::none()` when `!follow_redirects` (88), `Proxy::all` (91). Add `.http1_max_headers(HTTP1_MAX_RESPONSE_HEADERS)` here.
- crates/core/src/engine.rs:389 per-request `timeout`, :394 `resp.headers()`.
- crates/core/src/engine.rs:434-458 — `HTTP1_MAX_RESPONSE_HEADERS = 100` and its doc comment ("reqwest 0.12.28 has no setter"), plus the `classify` branch on "message head is too large" that produces "response headers too large (limit {N} headers, or the head exceeds hyper's size limit)" (task 47).
- crates/core/tests/engine.rs:340-391 — `serve_headers(n)` raw-TCP helper and the 150 and 90 header tests to update.
- Architect findings. **Check each against the 0.13.5 docs and source** (`~/.cargo/registry/src/*/reqwest-0.13.5/Cargo.toml` and `src/async_impl/client.rs`) before you rely on it:
  - 0.13.0 renames the feature `rustls-tls` to `rustls`.
  - The default rustls provider becomes aws-lc. Use `rustls-no-provider` and install or configure ring explicitly, for example with a `rustls` dependency with `ring` enabled and `ClientBuilder::use_preconfigured_tls` or a process-level `CryptoProvider::install_default`. Pick what 0.13.5 documents.
  - The roots features are removed and `rustls-platform-verifier` is the default verifier. Keep the bundled roots with `tls_certs_only(...)` and `webpki-roots` as a direct dependency, or whatever 0.13.5 documents.
  - `query` and `form` become opt-in features. engine.rs does not call `.query(` or `.form(` today (checked with rg). Enable them only if the build needs them.
  - `use_rustls_tls()` and `danger_accept_invalid_certs` are soft-deprecated. If the build warns, use the replacement 0.13.5 names, or allow the warning narrowly with a comment. Clippy must stay clean either way.
  - 0.13.5 pulls in base64 0.23, a second copy next to our 0.22 (Cargo.lock already has 0.23.1 through another crate), plus hyper-util `client-proxy` and tower-http follow-redirect.
  - reqwest's MSRV is 1.85 and rust-toolchain.toml pins 1.99.1, so the toolchain is fine.
- The existing tests do **not** cover TLS, invalid certificates or proxies at runtime. `verify_tls: false` and `proxy` appear only in the serde round-trip at tests/engine.rs:428-430. Redirect-none (167) and timeout (233) do have runtime tests.
- The repo has no `deny.toml` and no license check, so none is required.
- docs/conventions.md:73 — gpui-pinned dependency bumps are separate tasks. This task must not move any gpui-related entries in Cargo.lock beyond what reqwest pulls in.
- docs/plan/PLAN.md:26 — budgets: binary 20,971,520 B, idle RSS 120 MiB, after one request 128 MiB (macOS arm64, hardware renderer).

## Acceptance
- [ ] Cargo.lock has `reqwest` 0.13.5, and no 0.12.x copy remains.
- [ ] `cargo tree -p wisp-core -e normal | rg -i 'aws-lc|platform-verifier|openssl|native-tls'` prints nothing. The same command against `-p wisp-app` also prints nothing.
- [ ] `cargo tree -p wisp-core -e normal | rg 'ring v|webpki-roots v'` shows both.
- [ ] No C toolchain needed beyond what ring already needs: there is no `aws-lc-sys` or `cmake` in Cargo.lock.
- [ ] engine.rs has `HTTP1_MAX_RESPONSE_HEADERS: usize = 1000` and passes it to `http1_max_headers` on every client `ClientCache::get` builds. The doc comment no longer says reqwest has no setter.
- [ ] The overflow message reads `response headers too large (limit 1000 headers, or the head exceeds hyper's size limit)`, still `ErrorKind::Other`, still without "client error".
- [ ] Tests in crates/core/tests/engine.rs: 150 headers completes, 1,000 headers completes, and 1,001 headers fails with a message starting `response headers too large (limit 1000 headers`. If 1,000 headers hits hyper's byte buffer limit first, report it rather than shrinking the header lines silently. Keep `serve_headers` reading the request before replying.
- [ ] A test with `verify_tls: false` against a self-signed local TLS server completes, and the same request with `verify_tls: true` fails. Add this only if it needs no new dev-dependency beyond `rcgen`/`tokio-rustls` with the ring feature. Otherwise state in the report that it went untested and why.
- [ ] The existing redirect-none, timeout, connect and invalid-URL tests pass unchanged.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Report the release binary size (`target/wt-50/release/wisp`) and the delta from 19,014,912 B. It must be ≤ 20,971,520 B, or the task ends BLOCKED as described in Goal.
- [ ] Report idle and after-one-request RSS from `RENDERER=hardware scripts/measure/measure.sh` against 120 MiB and 128 MiB.

## Out of scope
- Switching to the OS trust store or rustls-platform-verifier.
- HTTP/2 header-list limits (`http2_max_header_list_size`) and hyper's HTTP/1 byte-size limit.
- Any change in crates/app, crates/storage, GPUI or gpui-component pins.
- Using reqwest 0.13's `query`/`form` helpers or other new features in the engine.
- Updating PLAN.md, HISTORY or docs (scribe's job).
