# 02 — Scaffold the Cargo workspace with every M0 dependency pinned

**Repo:** `.`
**Depends on:** 01
**Owns:**
- Cargo.toml
- Cargo.lock
- rust-toolchain.toml
- .gitignore (insertions only)
- crates/core/Cargo.toml
- crates/core/src/lib.rs
- crates/storage/**
- crates/app/Cargo.toml
- crates/app/src/main.rs

## Goal
Create the three-crate workspace and resolve the complete M0 dependency set into
`Cargo.lock` once, so tasks 04, 05 and 06 can build with `--locked` and never
touch the root manifest or the lockfile concurrently. `storage` is delivered
finished here as an empty stub.

## Decisions settled by the planner (apply exactly)
- Packages: `hc-core` (lib `hc_core`) in `crates/core`, `hc-storage` (lib
  `hc_storage`) in `crates/storage`, `hc-app` in `crates/app`. The shipped
  binary is named `httpclient` (`[[bin]] name = "httpclient"` in `hc-app`).
  Task 03 records this in `docs/conventions.md#naming`.
- `rust-toolchain.toml`: `channel = "1.99.1"`, `components = ["rustfmt", "clippy"]`, no `targets` list (CI adds targets).
- Root `[workspace]` with `resolver = "3"`, `members = ["crates/*"]`, `[workspace.package] edition = "2024"`.
- `[profile.release]`: `lto = true`, `codegen-units = 1`, `strip = true`, `opt-level = 3`. The M0 measurements (task 07) are taken with exactly this profile.
- `[workspace.dependencies]`, all with `default-features = false` plus only the features named:
  - `reqwest` — rustls TLS feature (`rustls-tls` or its current name), `http2`; nothing that pulls `native-tls`.
  - `tokio` — `rt-multi-thread`, `sync`, `time`, `macros`.
  - `async-channel` — the executor-agnostic event channel; `std`.
  - `serde` — `derive`, `std`.
  - `thiserror`, `tracing` (`std`), `anyhow` (`std`).
  - `wiremock` — dev only, for `hc-core` tests.
  - `gpui` and `gpui-component` — git dependencies, each pinned to an exact `rev`. Pick the latest `gpui-component` rev that builds on 1.99.1, then pin `gpui` to the exact zed rev that `gpui-component` rev itself pins, so the graph contains one `gpui`. Enable the Linux `wayland` and `x11` features if `gpui` gates them.
- Member manifests reference every dependency with `workspace = true`:
  - `hc-core`: reqwest, tokio, async-channel, serde, thiserror, tracing; dev: wiremock, tokio.
  - `hc-storage`: no dependencies.
  - `hc-app`: hc-core (path), hc-storage (path), gpui, gpui-component, anyhow, tracing. No direct reqwest or tokio.
- Stub sources: `crates/core/src/lib.rs` and `crates/storage/src/lib.rs` contain one `//!` doc line each; `crates/app/src/main.rs` is an empty `fn main() {}`.
- `.gitignore` gains `/target/`, `/logs/` and `*.log`.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:38-48 — workspace layout
- docs/conventions.md:30-48 — toolchain and naming
- docs/conventions.md:62-73 — GPUI pinning and dependency rules
- docs/architecture.md:135-147 — boundaries (rustls only; GPUI only in app)

## Acceptance
- [ ] `cargo build --workspace --locked` succeeds on macOS arm64 (this compiles `gpui`, proving the pin resolves and builds).
- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` are clean.
- [ ] `cargo tree --workspace --target all -i openssl-sys` and `cargo tree --workspace --target all -i native-tls` each report that the package is not in the graph. (`--target all` checks Windows and Linux graphs from macOS without building them.)
- [ ] `cargo tree -p hc-core -i gpui` and `cargo tree -p hc-storage -i gpui` report `gpui` is not a dependency of those packages.
- [ ] `cargo tree --workspace --target all -i gpui` shows exactly one `gpui` source and rev.
- [ ] `gpui` and `gpui-component` entries in `Cargo.toml` each carry `rev = "<40-char sha>"`, not `branch` or `tag`.
- [ ] `cargo metadata --format-version 1 --no-deps` lists packages `hc-core`, `hc-storage`, `hc-app`, and a bin target `httpclient`.
- [ ] Every member-crate dependency line uses `workspace = true`.
- [ ] The commit body states the chosen `gpui-component` and `gpui` revs and why that pair.
- [ ] Not checkable locally: that the Linux and Windows graphs *build*. Checked by task 05's CI run; here only `cargo tree --target all` evidence is required.

## Out of scope
- Any Engine code (task 04) or UI code (task 06) beyond the stub files above.
- CI workflows (task 05).
- Editing `docs/` or `CLAUDE.md` (task 03).
- If `gpui` or `gpui-component` transitively pulls `openssl-sys` or `native-tls`, do not work around it by patching: stop and report it, because it is evidence for the M0 GPUI exit decision.
