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

### Results
Per target: `binary_bytes` / `idle_rss_kib` / `after_request_rss_kib` / `status`.
Linux adds `pss_kib` and `rss_anon_kib` (idle/after); Windows adds
`private_working_set_kib` (idle/after). "m" = measured, "u" = unmeasured.
All runs are measure.yml, all four jobs built. A..C are cumulative on A unless noted.

| Variant | Run | Commit | macOS arm64 | macOS x64 | Windows x64 | Linux x64 |
|---|---|---|---|---|---|---|
| Baseline | 41607658851 | 0d450de | 19,181,056 / 74,816 / 80,032 / m | 20,714,504 / - / - / u | 28,947,456 / 42,784 / 52,180 / m; pws 9,024/13,112 | 37,993,000 / 175,192 / 172,864 / m; pss 148,989/146,736; anon 57,964/44,920 |
| A panic=abort | 41620961190 | 60799b4 | 15,460,832 / 74,016 / 78,960 / m | 17,923,784 / - / - / u | 18,963,968 / 41,584 / 54,004 / m; pws 8,956/13,296 | 33,452,456 / 167,760 / 170,080 / m; pss 141,621/144,012; anon 53,032/45,020 |
| B1 A + deps "s" | 41620991658 | 892e8b9 | 12,886,608 / 73,632 / 78,496 / m | 14,461,936 / - / - / u | 14,643,712 / 41,392 / 50,084 / m; pws 9,060/12,908 | 25,240,624 / 163,584 / 165,376 / m; pss 137,461/139,321; anon 53,048/45,044 |
| B2 A + deps "z" | 41621022500 | daac80e | 10,420,080 / 73,008 / 77,104 / m | 12,884,616 / - / - / u | 13,450,240 / 41,516 / 50,008 / m; pws 9,156/12,920 | 23,701,032 / 163,184 / 164,100 / m; pss 137,000/138,050; anon 53,332/45,044 |
| B3 A + whole-profile "s" | 41621094953 | b240014 | 12,406,976 / 73,280 / 78,032 / m | job failed, no artifact | 14,100,992 / 41,164 / 49,844 / m; pws 9,000/13,028 | 24,793,128 / 167,572 / 165,084 / m; pss 141,447/138,926; anon 57,688/45,272 |
| C A + Linux link flags (no B) | 41621126555 | 362e6f5 | 15,460,832 / 74,160 / 78,848 / m | 17,923,784 / - / - / u | 18,963,968 / 41,736 / 50,668 / m; pws 9,088/12,940 | 32,658,864 / 173,024 / 170,600 / m; pss 146,808/144,460; anon 57,912/45,100 |
| **Final A + B2 + C** | 41631211960 | 10e3b3f | 10,420,080 / 73,152 / 77,728 / m | 12,884,616 / - / - / u | 13,450,240 / 41,560 / 50,016 / m; pws 9,208/12,940 | 23,353,896 / 167,640 / 163,652 / m; pss 141,438/137,523; anon 58,168/44,972 |

Final vs baseline: macOS arm64 -45.7% (budget 20,971,520 met, 10,420,080), Windows -53.5%,
Linux -38.5%, macOS x64 -37.8%. ci.yml is green on 10e3b3f. Local
`cargo test --workspace --locked` and clippy `-D warnings` pass on macOS.

### Dropped / kept
- A panic=abort: KEPT. Saves 3.7 MB arm64, 10 MB Windows, 4.5 MB Linux.
- B1 deps "s": superseded by B2 (smaller still).
- B2 deps "z": KEPT, smallest on every target; round-trip completes.
- B3 whole-profile "s": DROPPED, larger than B2 on every target (and would slow our own wisp-* crates). Its macOS x64 job failed (the same job is `unmeasured` in every run, see below); not investigated.
- C Linux `--gc-sections` + `--icf=all`: KEPT in `.cargo/config.toml`, scoped to
  `[target.x86_64-unknown-linux-gnu]`. Alone it saves 0.8 MB (-2.4%) on A, and 0.35 MB (-1.5%) on A+B2.
  `--gc-sections` is already rustc's default (no-op, kept explicit); the gain is ICF.
  rustc 1.99.1 links with rust-lld by default on x86_64 Linux, so no `-fuse-ld` needed.
  Linux idle RSS is about 4,000 KiB higher with C (167,640 vs 163,184 for B2 alone);
  after-request RSS is lower, so likely noise or ICF page layout. Flag for the user.
  Config file applies identically to measure.yml and release.yml (neither sets RUSTFLAGS), so no workflow edits were needed.
- Not tried: `lto = "fat"` is already `true`; `opt-level = "s"` for wisp-* crates (task says they stay at 3).

### Unwinding audit (variant A)
Our crates: `rg 'catch_unwind|is_panic|set_hook' crates` finds nothing. Upstream, locked sources
(gpui-*, gpui-base 0.8.1, reqwest 0.13.5, tokio 1.54.1, and transitive wgpu/smol/etc.):
- gpui-pre-windows 0.3.10 `src/platform.rs:1821-1826`: guard only under `cfg!(debug_assertions)`, and it aborts anyway. Safe.
- gpui-base 0.8.1 `src/text_selection.rs:2825-2830`: catch, clean up, `resume_unwind`. Cleanup is a scope reset; with abort the process ends instead. Safe (no recovery).
- gpui-base `text/editor_seam.rs:77`, `text/editor_host.rs:1244`, `text_selection.rs:3736`, gpui-pre `app.rs:3732,3748`, `text_system/line.rs:1958-1973`, `profiler/hang.rs:1001`, gpui-pre-scheduler `tests.rs`/`test_scheduler.rs`: test code only.
- wgpu-hal 30.0.1 `vulkan/instance.rs:96-148`, `gles/mod.rs:1180`, `auxil/dxgi/exception.rs:84`: `let _ = catch_unwind` around log calls inside driver callbacks; only guards against a panicking logger. Safe.
- async-task 4.7.1 `raw.rs:549`, smol `spawn.rs:50`, async-global-executor `threading.rs:88,115`, async-io `reactor.rs:363,462`, calloop `sources/futures.rs:245`: panics in tasks/wakers caught to keep executors alive; with abort a panic in a spawned task ends the process (same fate as the unwinding default for the main/UI thread). We have no code that relies on a task panic being survived.
- stacker windows backend `windows.rs:32,96`, hyper `ffi/macros.rs:8`: catch and rethrow / C FFI only (hyper ffi unused). Safe.
- tokio: `JoinError` panic capture; no crate of ours inspects `is_panic`. Under abort a panic in a spawned task ends the process. reqwest: no catch_unwind in 0.13.5.
Conclusion: nothing needs unwinding at runtime; A kept. `cargo test` uses test/dev profiles, which ignore `panic` in `[profile.release]` (tests pass, see above).

### Other
- macOS x64 is `unmeasured` in every run including baseline: the app logs `before open_window` then no completion within 30 s on the Intel runner (pre-existing, not caused by this task). Binary builds and size is recorded.
- Timings recorded by the scripts: `settle_seconds` is 5 in every run, baseline included; no other timings exist, so no time regression is visible.
- Manual responsiveness check for the user (smaller opt-level in dependencies could slow them): text shaping, rendering and scrolling of a large response, on a Windows and a Linux build.

## Review round 1 — findings

Reviewed HEAD: 723f030.

- [STYLE] .cargo/config.toml:1-3 — the comment names "(variant C)" and restates that macOS/Windows are unaffected (the target header already says so). Per docs/conventions.md:103-104 keep only the rust-lld / `--icf=all` reason.
- [NOTE, optional] Results table rows B3 and C cite commits b240014 and 362e6f5, which no branch contains. Add one line each in Notes saying what those commits changed, so the rows stay traceable after GC.
