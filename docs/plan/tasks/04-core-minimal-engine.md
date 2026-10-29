# 04 — Implement the minimal core Engine over reqwest and rustls

**Repo:** `.`
**Depends on:** 02
**Owns:**
- crates/core/src/**
- crates/core/tests/**
- crates/core/Cargo.toml (no dependency additions; see Out of scope)

## Goal
Give `wisp-core` a service-shaped `Engine` that takes a `Send` or `Cancel`
command, performs the HTTP request on its own small tokio runtime, and reports
`Completed` or `Failed` on an executor-agnostic channel. This is the M0 subset
of spec section 3; the type names and shape must be the ones M1 extends, not a
throwaway.

## Required API (the app in task 06 is written against exactly this)
- `Engine::start(config: EngineConfig) -> (Engine, EventReceiver)` where `EventReceiver = async_channel::Receiver<Event>`.
- `Engine::submit(&self, cmd: Command) -> RequestId`. Returns immediately. For `Send` it returns the new id; for `Cancel(id)` it returns `id`.
- `Engine` is `Clone + Send + Sync`; dropping the last clone shuts the runtime down without blocking or panicking, from any thread, including inside another tokio runtime.
- `EngineConfig { worker_threads: usize, default_timeout: Option<Duration> }`; `Default` gives `1` and `Some(30 s)`. `worker_threads` is clamped to `1..=2`.
- `RequestId(u64)`, allocated monotonically by the engine.
- `Command::Send(PreparedRequest)`, `Command::Cancel(RequestId)`.
- `PreparedRequest { method: Method, url: String, headers: Vec<Header>, body: Option<Vec<u8>>, timeout: Option<Duration> }`; `Header { name: String, value: String }`; `Method` is an enum of `Get, Post, Put, Patch, Delete, Head, Options`.
- `Event::Completed { id, status: u16, reason: Option<String>, headers: Vec<Header>, total: Duration, size: u64, body: Vec<u8> }` and `Event::Failed { id, error: EngineError }`.
- `EngineError { kind: ErrorKind, message: String }`, `ErrorKind` one of `InvalidUrl, Dns, Connect, Tls, Timeout, Body, Cancelled, Other`. Defined with `thiserror`.
- Every public command, event and error type derives `Debug, Clone, PartialEq, Serialize, Deserialize`. No locks, handles or runtime types inside them.
- One `reqwest::Client`, built with rustls, reused across sends.

M0-provisional, to be replaced in M1 (state it in a one-line doc comment on each): `Completed.body: Vec<u8>` becomes a `BodyHandle`; `status`/`reason`/`headers` move to a `Headers` event; cancellation reports `Failed{kind: Cancelled}` until M1 adds `Event::Cancelled`.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:129-164 — core service API
- docs/architecture.md:53-77 — Engine shape and boundaries
- docs/conventions.md:50-87 — errors, logging, tests

## Acceptance
- [ ] `cargo test -p wisp-core --locked` passes, with tests (all against `wiremock` on 127.0.0.1, never the internet) named as sentences covering:
  - a GET returning 200 with a known body yields one `Completed` whose `status`, `body` and `size` match;
  - a POST sends the given headers and body, verified by a `wiremock` matcher;
  - a request to a port with nothing listening yields `Failed` with `ErrorKind::Connect`;
  - a response delayed past `PreparedRequest.timeout` (server-side `set_delay`, no client sleeps) yields `Failed` with `ErrorKind::Timeout`;
  - an unparsable URL yields `Failed` with `ErrorKind::InvalidUrl`;
  - `Cancel` on an in-flight delayed request yields `Failed` with `ErrorKind::Cancelled` and no `Completed` for that id;
  - events are received from a plain `std::thread` with no tokio runtime, via `recv_blocking`;
  - dropping the `Engine` inside a `#[tokio::test]` does not panic;
  - `Command` and `Event` values round-trip through `serde` (use `serde_json` only if it is already in `Cargo.lock`; otherwise assert the derives compile via a generic `fn assert_serde<T: Serialize + DeserializeOwned>()`).
- [ ] `cargo clippy -p wisp-core --all-targets --locked -- -D warnings` and `cargo fmt --check` are clean.
- [ ] `git diff main -- Cargo.lock Cargo.toml` is empty (built with `--locked`).
- [ ] No `unwrap`/`expect` outside `#[cfg(test)]` without an adjacent invariant comment.
- [ ] The runtime is built with `tokio::runtime::Builder::new_multi_thread().worker_threads(<clamped>)` — visible in the diff.
- [ ] No `tracing` call logs a header value.

## Out of scope
- `resolve`, variables, path params, auth, body store, `Started`/`Headers`/`Progress`/`Cancelled` events, per-settings client pool, cookie jar, history log — all M1.
- Adding or changing any dependency. If one is genuinely needed, stop and report: the lockfile is owned by task 02.
- `crates/app/**` and `crates/storage/**`.
