# 16 — Give the engine the full event set, real cancel, body store and per-settings clients

**Repo:** .
**Depends on:** 10, 12
**Owns:**
- crates/core/src/model.rs
- crates/core/src/engine.rs
- crates/core/src/lib.rs (re-export additions only)
- crates/core/tests/engine.rs
- crates/app/src/** (only what is needed to build against the new events)

## Goal
Replace the M0-provisional `Event` with the spec's six events. Stream response
bodies into the `BodyStore` instead of `Vec<u8>`. Make `Cancel` emit
`Cancelled`. Send `File` and `Multipart` bodies. Keep one `reqwest::Client` per
`ClientSettings`. The command and event types stay plain serde data with no
locks or handles. That keeps a socket transport possible later.

## Context
- crates/core/src/model.rs:60-86 — the M0-provisional `Event`, with comments saying what moves where
- crates/core/src/engine.rs:104-160 — submit and cancel; :160-215 — `perform`
- crates/core/src/body.rs — `BodyStore`, `BodyWriter`, `BodyHandle` (from 12)
- crates/app/src/view.rs:114-145 — `on_event`, and the `WISP_EVENT completed|failed` stdout lines that `scripts/measure/measure.sh` waits for
- docs/plan/specs/2026-10-29-http-client-v1.md — section 3

## Shape (the contract consumed by M2)
- `Event`:
  - `Started { id, at: SystemTime }`
  - `Headers { id, status: u16, reason: Option<String>, version: HttpVersion, headers: Vec<Header>, ttfb: Duration }`
  - `Progress { id, bytes: u64 }`
  - `Completed { id, total: Duration, size: u64, body: BodyHandle }`
  - `Failed { id, error: EngineError }`
  - `Cancelled { id }`
- `HttpVersion`: `Http09 | Http10 | Http11 | Http2 | Http3`, a serde enum in `model.rs`
- `ErrorKind` loses `Cancelled`. The rest of 10's set stays.
- `Command` stays as `Send(PreparedRequest) | Cancel(RequestId)`.
- `EngineConfig` gains `body_memory_limit: u64`, defaulting to `body::DEFAULT_MEMORY_LIMIT`. `Engine::bodies(&self) -> BodyStore` gives access for reading and releasing bodies.
- **Ordering.** For each request the order is `Started`, then optionally `Headers`, then zero or more `Progress`, then exactly one of `Completed`, `Failed` or `Cancelled`. Nothing is emitted for that id after the terminal event.
- **Progress throttling.** At most one `Progress` per 100 ms per request. The throttle is a separate function or struct that takes `Instant`s, so it can be unit-tested without the clock.
- **Bodies.** `File` is read with blocking I/O off the async worker. A missing or unreadable file yields `Failed { kind: BodyFile }`, and its message names the path. `Multipart` builds a `multipart/form-data` body by hand with a random boundary and file parts read from disk. reqwest's `multipart` feature is not enabled.
- **Clients.** Clients are cached by `ClientSettings` in a private map. `verify_tls: false` turns on `danger_accept_invalid_certs`. `follow_redirects: false` uses `Policy::none()`. `proxy` uses `Proxy::all`, and an invalid proxy yields `Failed { kind: InvalidUrl }`.
- **Cancel.** `Cancel` on an in-flight id drops its `BodyWriter`, which removes any spill file, and emits `Cancelled`. Cancelling an unknown or finished id emits nothing.
- **Logging.** tracing records header names only, never header values or URLs with a query.

## Acceptance
- [ ] Integration tests in `crates/core/tests/engine.rs` run against wiremock and show the following:
  - A 200 response yields `Started`, `Headers` (status 200, `Http11`), then `Completed`, in that order. `bodies().read(&body, 0, n)` returns the served bytes.
  - A 1 MiB body with `body_memory_limit` of 64 KiB completes with `is_spilled` true and correct bytes at offsets 0 and 1,000,000.
  - Cancelling a 5 s delayed response yields `Cancelled` and no `Failed` or `Completed` for that id before a sentinel request completes.
  - A `PreparedBody::File` pointing at a missing path yields `Failed` with kind `BodyFile`.
  - A two-part multipart body reaches the mock with a `content-type` starting `multipart/form-data; boundary=`, and both part names appear in the received body.
  - `follow_redirects: false` against a 302 yields `Headers { status: 302 }`.
  - The serde round-trip covers every `Event` variant.
- [ ] A unit test of the progress throttle shows that ten updates 10 ms apart produce one emission, and two updates 150 ms apart produce two.
- [ ] A unit test shows two `Send`s with equal `ClientSettings` share one cached client and different settings create two.
- [ ] The app still prints `WISP_EVENT completed status=<n> size=<n>` and `WISP_EVENT failed kind=<k>`. It shows the first 64 KiB of the body, read from `bodies()`, and releases the previous handle when a new response replaces it.
- [ ] `rg -n 'Mutex|Arc|Sender|Receiver' crates/core/src/model.rs` finds nothing.
- [ ] `scripts/measure/measure.sh` on macOS arm64 hardware (with `RENDERER=hardware`) reports a binary of at most 20,971,520 B, idle RSS of at most 120 MB and RSS after one request of at most 128 MB. The figures go in the close-out and are not committed.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- The cookie jar (it needs reqwest's `cookies` feature, which is a dependency decision for later)
- Phase timings beyond TTFB and total
- Any UI beyond keeping the M0 window working
- `scripts/measure/**` and its results files
- Editing `crates/core/Cargo.toml` (10 owns the dependency set). If a dependency is truly missing, stop and report.
