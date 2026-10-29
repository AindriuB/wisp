# 06 — Build the GPUI walking-skeleton window that sends one request

**Repo:** `.`
**Depends on:** 02, 04
**Owns:**
- crates/app/src/**
- crates/app/Cargo.toml (no dependency additions)

## Goal
The `httpclient` binary opens one window with a method dropdown, a URL input, a
Send button, a status line and the raw response body, wired to the `hc-core`
`Engine`. It is the thing M0 measures and the evidence for whether GPUI stays.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:166-198 — UI and state (only the request bar and response pane apply)
- docs/plan/tasks/04-core-minimal-engine.md — the exact Engine API to call
- `gpui-component` examples at the rev pinned in task 02 — `Select`/dropdown, `Input`, `Button` usage
- docs/conventions.md:50-66 — `anyhow` allowed here; GPUI only here

## Acceptance
- [ ] `cargo build --release --locked -p hc-app` produces `target/release/httpclient` on macOS arm64.
- [ ] `cargo clippy -p hc-app --all-targets --locked -- -D warnings` and `cargo fmt --check` are clean.
- [ ] The window shows a dropdown with the seven `hc_core::Method` values (default GET), a URL input, a Send button, a status line and a scrollable raw-body area.
- [ ] Send builds a `PreparedRequest` and calls `Engine::submit`; one GPUI background task drains the `EventReceiver` and updates the view. The UI thread never blocks on the channel.
- [ ] On `Completed` the status line reads `<status> <reason> · <ms> ms · <size>` (size in B/KB/MB) and the body area shows the body as lossy UTF-8, truncated to the first 1 MiB with a visible `truncated` marker beyond that.
- [ ] On `Failed` the status line reads `Error (<kind>): <message>`.
- [ ] Measurement hook for task 07: when env var `HC_AUTOSEND_URL` is set, the app submits one GET to it after the window opens and, on the resulting event, prints exactly one stdout line `HC_EVENT completed status=<n> size=<bytes>` or `HC_EVENT failed kind=<kind>`. Without the variable it prints nothing and sends nothing.
- [ ] Hook checked locally: with `python3 -m http.server 8765 --bind 127.0.0.1` running in a temp dir holding a known file, `HC_AUTOSEND_URL=http://127.0.0.1:8765/<file> target/release/httpclient` prints `HC_EVENT completed status=200 size=<file size>` within 10 s.
- [ ] Manual check, reported not asserted: the implementer runs the binary, sends one request by hand, and attaches a screenshot path (in `logs/`, gitignored) in the close-out. No code comment may claim a visual outcome.
- [ ] `cargo tree -p hc-app --depth 1` lists no `reqwest` or `tokio`.
- [ ] `git diff main -- Cargo.lock Cargo.toml` is empty.
- [ ] **Not verifiable locally:** the window on Windows and Linux. Covered by task 05 (builds) and task 07 (best-effort CI launch); not a criterion here.

## Out of scope
- Tabs, sidebar, environments, headers/body editors, pretty-printing, themes, settings, keyboard shortcuts — M2.
- `#![windows_subsystem = "windows"]`; keep the console so the hook's stdout works on Windows in M0.
- Any change under `crates/core/**` or `crates/storage/**`. If the Engine API is missing something, stop and report.
- Adding dependencies.
