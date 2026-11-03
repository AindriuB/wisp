# 64 — Measure release-profile shrink variants and keep only what pays

**Repo:** .
**Depends on:** none
**Owns:**
- Cargo.toml (`[profile.*]` sections only; may add `[profile.release.package."*"]` etc.)
- .cargo/config.toml (new, only if a Linux-only link flag is kept)
- .github/workflows/measure.yml (build flags/env of the Build step only)
- .github/workflows/release.yml (build flags/env of the Build step only)
- docs/plan/tasks/64-shrink-profile.md (`## Notes` section only)

## Goal
Shrink the shipped Windows and Linux binaries by changing release-build
settings, with every change backed by a `measure.yml` run on all four targets.
Keep only variants that reduce size without breaking a target, the tests or the
request round-trip. Record every variant tried, including the dropped ones and
why they were dropped.

## Context
- Cargo.toml:9-13 — current `[profile.release]`: `lto = true`,
  `codegen-units = 1`, `strip = true`, `opt-level = 3`, no `panic` key.
- docs/architecture.md#budgets — baseline, measure.yml run 41607658851 on
  0d450de: Linux x64 37,993,000 B; Windows x64 28,947,456 B; macOS arm64
  19,181,056 B (budget <= 20,971,520 B); macOS x64 20,714,504 B.
- .github/workflows/measure.yml:73-74 and .github/workflows/release.yml:59-60 —
  both build with `cargo build --release --locked -p wisp-app --target <t>`.
  Neither sets RUSTFLAGS today. ci.yml does not build release.
- scripts/measure/measure.sh:66-90 — JSON fields: `binary_bytes`,
  `idle_rss_kib`, `after_request_rss_kib`, `status`, `settle_seconds`, plus
  `rss_anon_kib`/`pss_kib` (Linux) and `private_working_set_kib` (Windows).
  `status` records whether the request round-trip completed (30 s cap).
- Unwinding: our crates have no `catch_unwind`, `JoinError::is_panic` or
  panic hook (`rg -n 'catch_unwind|is_panic|set_hook' crates`). The upstream
  uses found in a first pass:
  - gpui-pre 0.3.10: test-only uses (`src/test.rs`, `app.rs` tests,
    `text_system/line.rs` tests).
  - gpui-pre-windows `platform.rs:1821-1826`: wndproc guard, `cfg!(debug_assertions)`
    only, and it aborts anyway.
  - gpui-base 0.8.1 `text_selection.rs:2825-2830`: catches, cleans up, then
    `resume_unwind`s.
  - tokio: catches task panics into `JoinError`; under abort, a panic in a
    spawned task ends the process.
  The implementer confirms this list against the locked sources (including
  reqwest and gpui-component) before relying on it.
- Profiles: `cargo test` uses the `test`/`dev` profiles, which never take
  `panic` from `release`. Confirm that a `[profile.release]` change leaves
  `cargo test --workspace --locked` unaffected.
- Use `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-64`. One target
  dir is about 6 GB. Measure each variant in CI
  (`gh workflow run measure.yml --ref task/64-shrink-profile`, then
  `gh run download`). Clean the target dir between local release builds and
  after the task.

## Variants
Measure in this order, building on each one that is kept:
- A. `panic = "abort"` in `[profile.release]`. Do this only after the
  unwinding audit above finds nothing that needs unwinding at runtime.
- B. `[profile.release.package."*"] opt-level = "s"` (our `wisp-*` crates
  stay at 3). Also try `"z"`, or whole-profile `"s"`, if B looks promising.
  Keep whichever measures smallest without failing the round-trip.
- C. Linux-only link flags (`-C link-arg=-Wl,--gc-sections`; `--icf=all` via
  lld if the runner has it). Scope them to Linux targets only, with a
  `[target.x86_64-unknown-linux-gnu]` section in `.cargo/config.toml` or a
  per-OS env in the workflows. They must have no effect on the macOS or
  Windows builds.

## Acceptance
- [ ] `## Notes` holds a results table. Each row covers one variant: baseline,
      each variant tried, and the final variant. Each row gives the run id,
      the commit, and for all four targets `binary_bytes`, `idle_rss_kib`,
      `after_request_rss_kib` and `status`, plus `rss_anon_kib`/`pss_kib` on
      Linux and `private_working_set_kib` on Windows.
- [ ] Every variant dropped has a one-line reason in Notes. Notes also record
      the unwinding audit result (each runtime `catch_unwind` found,
      `path:line`, and why abort is safe there or why A was dropped).
- [ ] The committed settings are the best measured variant. In the final
      variant's measure run, all four targets build and every target's
      `status` shows the round-trip completed. macOS arm64 `binary_bytes` is
      <= 20,971,520.
- [ ] `cargo test --workspace --locked` and
      `cargo clippy --workspace --all-targets --locked -- -D warnings` pass
      locally on macOS. ci.yml is green on the branch.
- [ ] Any build flag or env exists identically in both measure.yml and
      release.yml, so shipped binaries match measured ones. Notes list any
      timings the scripts already record, compared with baseline. Notes also
      name text shaping, rendering and scrolling as a manual responsiveness
      check for the user.

## Out of scope
- GPUI feature changes, including the wayland/x11 backend choice (the user
  decides that next).
- Patching, forking or vendoring GPUI or gpui-component.
- Dependency version changes and `Cargo.lock` churn beyond what a profile
  change forces.
- Setting or changing budgets, and editing docs/architecture.md or PLAN.md.
  The scribe records the results.
- Changes to the measure scripts and new benchmarks.
- Opening a PR. That happens at `/record`.

## Notes
