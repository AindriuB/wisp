# 63 — Make measure.yml record truthful per-platform numbers

**Repo:** .
**Depends on:** none
**Owns:**
- .github/workflows/measure.yml
- scripts/measure/measure.sh
- scripts/measure/measure.ps1
- docs/plan/tasks/63-truthful-measure.md (the `## Notes` section only)

## Goal
`measure.yml` and `scripts/measure/*` must give per-platform numbers for current
`main` that we can trust. They are the baseline for later shrink work and for
per-platform budgets. The Linux and Windows figures in docs/architecture.md come
from the task 09 skeleton run (2026-10-29) and are stale. Linux RSS (171 MiB)
mostly counts shared llvmpipe driver pages under xvfb, not the app's own memory.
The JSON needs fields that show the app's own memory and which renderer
produced the number.

## Context
- scripts/measure/measure.sh:100 — `rss_of` uses `ps -o rss=`, which counts shared driver libraries. Linux can read `/proc/<pid>/smaps_rollup` (fields `Pss:` and `Anonymous:`; RssAnon is also `RssAnon:` in `/proc/<pid>/status`). Sample at the same two points as the existing RSS.
- scripts/measure/measure.sh:48-69 — `write_json`; one python heredoc builds the JSON. Add fields here; keep every existing key and its meaning.
- scripts/measure/measure.ps1:6-7, :42-53 — Windows already samples `PrivateMemorySize64` into nested `private_kib`. Promote it to a headline top-level field. Keep `idle_rss_kib` / `after_request_rss_kib` (working set) and `private_kib` unchanged for continuity.
- .github/workflows/measure.yml:52-57 — "Diagnose graphics stack" already runs `xvfb-run -a vulkaninfo --summary`. That is a possible source for the Linux device name, e.g. `deviceName = llvmpipe (LLVM ...)`. The app's `RUST_LOG=info` stderr (`measure-*.{idle,req}.err.log`) may also log the adapter. Check it before adding anything.
- .github/workflows/measure.yml:89-102 — the artefact upload already exists per target. Check that the JSON actually lands in the artefact on every OS, including when a measure step fails.
- .github/workflows/measure.yml:38 — `continue-on-error: true` makes a job look green even when its measurement failed. "Succeeds" in Acceptance means the uploaded JSON has `"status": "measured"`, not a green job.
- Cargo.toml:9-12 — `[profile.release]` has `strip = true`, so the built binary is already stripped. `binary_bytes` stays as it is now.
- docs/architecture.md:160-181 — the current figures (read only; scribe updates it at /record).
- History: tasks 07 and 09 (`docs/plan/HISTORY-INDEX.md` rows "RSS and binary-size measurement scripts…" and "Measure diagnostics and first CI measurements"). macOS x64 RSS hung in `ISIconManager` on `macos-26-intel`.
- The implementer cannot run Linux or Windows locally. Push the branch and run `gh workflow run measure.yml --ref task/63-truthful-measure`, then `gh run watch <id>` and `gh run download <id>`. Prefer no local build. If you do need one, use `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-63` and delete it afterwards (disk is tight).

## Acceptance
- [ ] Linux result JSON keeps `idle_rss_kib` / `after_request_rss_kib`. It adds integer `pss_kib` and `rss_anon_kib`, each with `idle` and `after_request` values read from `/proc/<pid>/smaps_rollup`. It also adds a string `renderer_device` (e.g. contains `llvmpipe`) taken from `vulkaninfo` or the app log. It is `null` with a reason only if neither source gives a name. macOS/Windows JSON carries the same keys, set to `null` where not applicable.
- [ ] Windows result JSON has a top-level `private_working_set_kib` with `idle` and `after_request` integers, next to the unchanged `idle_rss_kib`, `after_request_rss_kib` and `private_kib`. The `measure.ps1` header comment names it as the headline figure.
- [ ] `binary_bytes` is still the size of the `--release` (stripped) binary on every OS. The `WISP_AUTOSEND_URL` request run still gates `after_request_*` on the `WISP_EVENT completed` line. `git diff main -- crates/ Cargo.toml Cargo.lock` is empty.
- [ ] A `gh workflow run measure.yml --ref task/63-truthful-measure` run on the final commit uploads one artefact per target, each holding its result JSON. Linux, Windows and `aarch64-apple-darwin` JSON have `"status": "measured"`. The `x86_64-apple-darwin` outcome (measured, or unmeasured with its `reason`) is recorded whichever way it goes.
- [ ] `## Notes` below holds that run id, its commit SHA, and per target: `binary_bytes`, idle/after RSS, plus PSS, RssAnon and `renderer_device` (Linux) or private working set (Windows).
- [ ] `bash -n scripts/measure/measure.sh` passes; `shellcheck scripts/measure/measure.sh` reports nothing new versus `main` if shellcheck is available.

## Out of scope
- Changing any budget, the Cargo profile, features or dependencies; any shrink work.
- App code (`crates/**`). If the renderer name can only come from an app change, record `renderer_device: null` with a reason, and in your report give the smallest app change that would fix it. Do not make it.
- docs/architecture.md, PLAN.md, HISTORY*.md (scribe, at /record).
- The committed records in `scripts/measure/results/` (do not rewrite them).
- Fixing the macOS x64 `ISIconManager` hang beyond recording its outcome.
- ci.yml or any other workflow.
- Opening a pull request (opened at /record per docs/workflow.md). Pushing the task branch to run the workflow is allowed.

## Notes
Run 41602926409, commit 7231d4945025a622d1482471f51b988d1b5d103e (the notes commit after it changes only this file). One artefact per target, each with its JSON.

| target | status | binary_bytes | idle / after RSS KiB | extra |
|---|---|---|---|---|
| x86_64-unknown-linux-gnu | measured | 37993000 | 173976 / 173160 | PSS 147808 / 147025; RssAnon 57728 / 45264 (Anonymous from smaps_rollup); renderer_device `llvmpipe (LLVM 21.1.8, 256 bits)` |
| x86_64-pc-windows-msvc | measured | 28947456 | 42708 / 52152 | private working set 8972 / 12980; private_kib 35080 / 41472 |
| aarch64-apple-darwin | measured | 19181056 | 74688 / 79952 | pss/anon/device null (n/a) |
| x86_64-apple-darwin | unmeasured | 20714504 | none | reason: no 'WISP_EVENT completed' line within 30 s (stderr 2 lines) |

Linux note: PSS and RssAnon are both far below RSS; anon is the closest to the app's own memory.
