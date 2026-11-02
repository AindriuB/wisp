# 56 — Surface dropped roots and provider install errors; point TLS tests at 127.0.0.1

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/core/src/engine.rs (`ClientCache::get`, `install_crypto_provider` and the `#[cfg(test)] mod tests` block only)
- crates/core/tests/tls.rs

Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-56`.

## Goal
M3 (v1 hardening). Closes the open reqwest 0.13 follow-ups in PLAN.md (tasks 50 and 51). The root-certificate `filter_map` silently drops roots that fail to parse. `install_crypto_provider` silently discards the `install_default` Err. The TLS tests use `https://localhost` while the server binds only to 127.0.0.1. All three become explicit, with no change in normal behaviour.

## Context
- crates/core/src/engine.rs:78-108 — `ClientCache::get`; the roots `filter_map(... from_der(..).ok())` at :85-87.
- crates/core/src/engine.rs:445-452 — `install_crypto_provider` with `let _ = ...install_default()`.
- crates/core/src/engine.rs:500-553 — existing unit tests.
- crates/core/tests/tls.rs:42 — the cert has SANs `localhost` and `127.0.0.1`; the URLs at :90, :107 and :128 use `localhost`.
- docs/conventions.md#logging — `tracing`; no certificate contents in logs.

## Acceptance
- [ ] `ClientCache::get` counts roots that fail `from_der`. When the count is non-zero it emits one `tracing::warn!` with the count (no certificate bytes). Valid roots are still passed to `tls_certs_only`.
- [ ] New unit test: every entry of `webpki_root_certs::TLS_SERVER_ROOT_CERTS` parses with `reqwest::Certificate::from_der` (count equal to the slice length).
- [ ] `install_crypto_provider` handles the Err branch explicitly: it logs at `debug` that a provider was already installed. A unit test asserts that `rustls::crypto::CryptoProvider::get_default().is_some()` after the call. There is no `let _ =` on `install_default` any more.
- [ ] tls.rs URLs use `https://127.0.0.1:{port}/`; `rg -n 'localhost' crates/core/tests/tls.rs` matches only the rcgen SAN list.
- [ ] `cargo test -p wisp-core --test tls` passes 10 consecutive runs (report the count).
- [ ] `git diff main -- crates/core/src/engine.rs` touches only the three regions named in Owns. `perform`, `read_file`, `build_multipart` and the body-writer loop are unchanged (task 57 owns them next).
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- [ ] Release binary size (`target/wt-56/release/wisp`) is reported with its delta from 19,114,864 B (d6a8bbb).

## Out of scope
- Streaming request bodies and the spill-write fix (task 57).
- Client-cache locking (`ClientCache` builds under a std `Mutex`).
- Any Cargo.toml, reqwest feature or rustls change.

## Amendment (orchestrator, 2026-11-02)
Code review found that with reqwest 0.13.5 built with `rustls-no-provider` (no native-tls), `reqwest::Certificate::from_der` does no parsing and always returns Ok (reqwest-0.13.5/src/tls.rs:166-172). So the per-root drop counter and its `warn!` (engine.rs:85-101) can never fire, and `every_bundled_root_parses` (engine.rs:564-571) can never fail. Real parsing happens in `reqwest::tls::rustls_store` (tls.rs:637-643) at `ClientBuilder::build()`. One bad root makes the build fail for every verify_tls=true client, and that failure is already reported as "cannot build client". Nothing is silently dropped, so the Goal's premise is wrong.

User decision:
- Remove the dead counter and its `warn!`. Restore the plain mapping: no `filter_map(.ok())` hiding anything. Collect with `from_der` and `?` or `expect`, whichever reads cleanest, since it is infallible.
- Keep the existing loud build failure.
- Replace the test with one that loads every bundled `webpki-root-certs` DER into a rustls `RootCertStore` (`add_parsable_certificates`, via the ring-free pki-types path already in the dependency tree). It asserts that zero are ignored and that the added count equals the bundle length.

This supersedes the Acceptance items for the drop count with `warn!` and for the `from_der` parse test.

Rejected alternatives: pre-parsing with rustls and passing only good roots on (more code for a failure the pinned bundle cannot have); deleting the counter with no test.

## Review round 1 — findings
- [P2] engine.rs:85-101 and :564-571 — the drop counter is dead code and `every_bundled_root_parses` is always true, because `from_der` never fails with these features. Settled by the amendment above.
- [NOTE, optional] engine.rs:573-577 — `install_crypto_provider_leaves_a_default_provider` would also pass with the old `let _ =` code, so it does not exercise the new Err branch.
