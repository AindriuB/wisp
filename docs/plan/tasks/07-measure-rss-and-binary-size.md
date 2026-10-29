# 07 — Measure idle RSS, RSS after one request, and binary size per platform

**Repo:** `.`
**Depends on:** 05, 06
**Owns:**
- scripts/measure/**
- .github/workflows/measure.yml

## Goal
A repeatable script that launches `httpclient`, records idle RSS, RSS after one
request and binary size, and emits one JSON record per platform. Run it locally
on macOS arm64 and in CI for the other targets, so task 08 can turn the numbers
into the budgets in `docs/architecture.md`.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:209-222 — what M0 measures and why
- docs/plan/tasks/06-app-walking-skeleton-window.md — the `HC_AUTOSEND_URL` hook and its stdout line
- docs/plan/tasks/05-ci-release-workflows.md — matrix, targets and build command to reuse
- docs/architecture.md:149-155 — the Budgets section the numbers feed

## Acceptance
- [ ] `scripts/measure/measure.sh` (macOS, Linux) and `scripts/measure/measure.ps1` (Windows) take the binary path and an output path, and write one JSON object with keys: `target`, `commit`, `binary_bytes`, `idle_rss_kib`, `after_request_rss_kib`, `settle_seconds`, `renderer` (`hardware` | `software` | `unknown`), `status` (`measured` | `unmeasured`), `reason` (null unless unmeasured), `os_version`.
- [ ] Method, stated in a short header comment in each script: serve a fixed 64 KiB file from `python3 -m http.server` on 127.0.0.1; launch once without the hook, wait `settle_seconds` (5), sample RSS, kill; launch again with `HC_AUTOSEND_URL`, wait for the `HC_EVENT completed` line, wait `settle_seconds`, sample RSS, kill. RSS is `ps -o rss=` on macOS/Linux and `WorkingSet64` on Windows (the script also records `PrivateMemorySize64` as `private_kib`). Fixed waits are allowed here; this is a measurement, not a test.
- [ ] The script exits non-zero and writes `status: "unmeasured"` with a `reason` if the window fails to start or no `HC_EVENT` line arrives within 30 s; it never writes made-up numbers.
- [ ] Local run on this Mac against `cargo build --release --locked -p hc-app` produces `scripts/measure/results/aarch64-apple-darwin.json` with `status: "measured"`, committed.
- [ ] `scripts/measure/results/x86_64-apple-darwin.json` is committed with `binary_bytes` from the local cross-build and `status: "unmeasured"`, `reason` noting that RSS under Rosetta is not representative.
- [ ] `measure.yml` triggers on `workflow_dispatch` only, builds the four targets as in `release.yml`, runs the matching script (Linux under `xvfb-run` with Mesa lavapipe for software Vulkan), uses `continue-on-error: true` per matrix entry, and uploads `measure-<target>.json` artifacts.
- [ ] `actionlint .github/workflows/measure.yml` exits 0 and `shellcheck scripts/measure/measure.sh` exits 0.
- [ ] `measure.ps1` cannot be run or linted here (`pwsh` is not installed on this Mac); it is reported `UNVERIFIED — runs first in measure.yml` and is checked by that run's Windows artifact.
- [ ] **Not verifiable locally:** Windows x64, Linux x64 and macOS x64 RSS. They come from a `measure.yml` run after a remote exists. CI runners render in software (lavapipe, WARP) or in a VM, so CI RSS is recorded with `renderer: "software"` or `"unknown"` and is flagged as not comparable with the local hardware figure. The close-out lists these as pending with that reason.

## Out of scope
- Writing the numbers into `docs/architecture.md` or setting budgets (task 08).
- Enforcing budgets in CI — a later task, once budgets exist.
- Any change to `crates/**`, `release.yml` or `ci.yml`.
- Startup-time or CPU measurements.
