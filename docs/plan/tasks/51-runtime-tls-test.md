# 51 — Add runtime TLS tests against a self-signed local server

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/core/tests/tls.rs (new)
- crates/core/Cargo.toml (`[dev-dependencies]` only)
- Cargo.toml (new `[workspace.dependencies]` entries for the dev-deps, plus the one-line `webpki-root-certs` change described below; nothing else)
- Cargo.lock

Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-51`.

## Goal
Closes the task-50 residual in docs/plan/PLAN.md:34-35 ("Reqwest 0.13 follow-ups"):
no test runs TLS at runtime. Add integration tests that start a TLS server on
127.0.0.1, using a self-signed certificate generated at test time, and send
requests through the real `Engine`. They prove that `verify_tls: true` rejects
the certificate and `verify_tls: false` accepts it. The change is tests and
dev-dependencies only. Production code does not change, except for one
optional line in Cargo.toml.

## Context
- crates/core/src/engine.rs:78-111 — `ClientCache::get`: `install_crypto_provider()` (83), bundled roots through `tls_certs_only` with `webpki_root_certs` (85-90), `http1_max_headers` (91), `tls_danger_accept_invalid_certs(true)` when `!verify_tls` (92-94).
- crates/core/src/engine.rs:445-451 — `install_crypto_provider` installs ring as the process default.
- crates/core/src/engine.rs:454-496 — `classify`: a cause chain containing "certificate", "tls" or "handshake" maps to `ErrorKind::Tls` (485-487). The message is the top-level text followed by each cause, joined with ": ".
- crates/core/tests/engine.rs:9-38 — the `get(url)` and `collect_until_terminal` helpers to copy. Integration test files cannot share them unless you add a `tests/common` module; copying them is fine.
- crates/core/tests/engine.rs:341-366 — `serve_headers(n)`: a raw server that binds port 0, reads the request, then replies. Follow the same shape. The 1000-header test is at :375-380.
- Cargo.toml:15-28 — workspace deps: rustls 0.23 with `ring`, `std` and `tls12`, no defaults (19); `webpki-root-certs = "1.0"` (20); tokio without `net` or `io-util` (21).
- Cargo.lock already has `tokio-rustls` 0.26.6 and `rustls-pki-types` 1.15.1 through reqwest. tokio-rustls 0.26 defaults to `["logging","tls12","aws_lc_rs"]`, so use `default-features = false, features = ["ring", "tls12"]`.
- rcgen is not in the lock. Check its `[features]` in the version you pick. Turn defaults off and enable only `crypto` + `ring`, plus `pem` if you need it. Do not enable `aws_lc_rs`.
- `tokio-rustls` needs tokio's `net` and `io-util` features. Add them on the **dev-dependency** line in crates/core/Cargo.toml, for example `tokio = { workspace = true, features = ["net", "io-util"] }`. Do not add them to the workspace `tokio` line. Resolver 2 keeps dev-only features out of the release build.
- You may serve TLS with `rustls::ServerConnection` + `rustls::StreamOwned` on a `std::thread` instead of tokio-rustls. rustls is already a normal dependency. If you do, you need neither tokio-rustls nor the extra tokio features. Either approach is acceptable. Say in the report which one you used.
- The server config must use the ring provider explicitly, e.g. `ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))`. Do not rely on the process default: test ordering decides whether the engine has installed it yet.
- PLAN.md:36 — release binary 19,114,864 B after task 50.

## Acceptance
- [ ] crates/core/tests/tls.rs exists. Each server binds `127.0.0.1:0`, and the test passes the bound port to the URL. There are no fixed ports and no sleeps.
- [ ] The server certificate is generated at test time with rcgen for `localhost` and/or `127.0.0.1`. No key or cert files are committed.
- [ ] Test: `verify_tls: true` against the self-signed server ends in `Event::Failed` with `error.kind == ErrorKind::Tls`. The test also asserts that `error.message` contains a human-readable certificate reason, e.g. it contains "certificate" (case-insensitive) or the rustls `UnknownIssuer` text. Use whichever the real message contains, and quote that message in the report.
- [ ] Test: `verify_tls: false` against the same kind of server ends in `Event::Completed` with status 200. The body read back through the engine equals the bytes the server sent.
- [ ] Optional, include it only if it costs a few lines on top of the server helper: a TLS response with 1,000 headers completes.
- [ ] Production code is unchanged: `git diff main -- crates/core/src crates/app crates/storage` is empty.
- [ ] Optional one-line cleanup: Cargo.toml:20 becomes `webpki-root-certs = { version = "1.0", default-features = false }`. Make it only if `cargo build` and the tests pass and the release binary size does not change. Otherwise leave the line as it is and say so.
- [ ] `cargo tree -p wisp-core -e normal | rg -i 'aws-lc|openssl|native-tls'` prints nothing. The same command with `-p wisp-app` also prints nothing.
- [ ] `cargo tree -p wisp-core -e dev | rg -i 'aws-lc|openssl|native-tls'` prints nothing. If no feature choice of rcgen or tokio-rustls avoids aws-lc, stop and report BLOCKED. Name the crate and the feature that pulls it in.
- [ ] Cargo.lock gains no `aws-lc-sys`, `aws-lc-rs` or `cmake`.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Not flaky: `cargo test -p wisp-core --test tls` passes 10 consecutive runs. Report the count.
- [ ] Report the release binary size (`cargo build --release -p wisp-app`, then `target/wt-51/release/wisp`). It must equal 19,114,864 B, or differ only by the one-line `webpki-root-certs` change if you made it. State the delta.

## Out of scope
- Any change to crates/core/src/engine.rs. That includes the other task-50 residuals: the `filter_map` that drops unparseable roots, and `install_crypto_provider` ignoring the `install_default` Err. If a test reveals an engine bug (for example, the wrong `ErrorKind` or `verify_tls: false` still rejecting the cert), stop and report it. Do not fix it.
- Changing crates/core/tests/engine.rs, including moving its helpers into a shared module.
- Proxy, HTTPS-redirect, HTTP/2-over-TLS or client-certificate tests.
- Any change to the workspace `tokio`, `reqwest` or `rustls` lines, or to crates/app or crates/storage.
- Updating PLAN.md, HISTORY or docs (scribe's job).
