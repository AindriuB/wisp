# 68 — Fix the cheap measure-script residuals from 09, 63 and 65

**Repo:** `.`
**Depends on:** none
**Owns:**
- scripts/measure/measure.sh
- scripts/measure/measure.ps1
- .github/workflows/measure.yml
- docs/plan/tasks/68-measure-script-fixes.md (the `## Notes` section only)

**Build:** any cargo command uses
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-68`.

## Goal
Several small faults in the measurement scripts make a run less informative than
it should be: a missing Windows perf counter throws away the whole run, the Linux
`renderer_device` does not say where its name came from, a malformed
`budgets.json` produces a Python traceback, a set `CDPATH` can break path
resolution, the header misstates an exit code, and the measure job can hang for
GitHub's 6-hour default. Fix each without changing what is measured or the budgets.

## Context
- scripts/measure/measure.ps1:54-58 — `Get-PrivateWs` (`-ErrorAction Stop`, throws
  when no row); its callers at :153 and :177; :61-80 `Write-Result`
  (`private_working_set_kib`, `renderer_device`); :46-47 the budgets-file check;
  :75 `ConvertFrom-Json` of the budgets file; :183-191 the catch-all that turns a
  throw into `unmeasured`.
- scripts/measure/measure.sh:11-12 vs :26-27 — the header says "exit non-zero"
  where the exit-code list says 1. :34 and :36 — `cd "$(dirname ..)" && pwd`, which
  prints the directory twice-over when `CDPATH` is set. :53 — `cleanup`, shellcheck
  SC2329 (trap-invoked). :66-118 `write_json`; :100-101 loads the budgets file in
  Python with no error handling. :131-143 `find_device` (app log first, then
  `vulkaninfo`).
- .github/workflows/measure.yml:15-45 — the job; no `timeout-minutes`.
- scripts/measure/budgets.json — read only; its values do not change.
- docs/architecture.md (Budgets section) — the result-JSON keys, for reference only.

## Acceptance
- [ ] measure.ps1: `Get-PrivateWs` returns `$null` (with one `[Console]::Error`
      warning line naming the pid) when the CIM query fails or returns no row,
      instead of throwing; the run then completes with
      `private_working_set_kib.idle`/`after_request` null and the rest measured.
      The budget metrics for Windows (`private_kib.*`) are unaffected.
- [ ] Both scripts write a new key `renderer_device_source`: `"app_log"` or
      `"vulkaninfo"` when `renderer_device` is set, otherwise null. measure.ps1
      writes null. Its header comment documents the key next to `renderer_device`.
- [ ] measure.sh: with `BUDGETS` pointing at a file containing `{not json`, the
      script exits 2 with a single stderr line of the form
      `budgets file is not valid JSON: <path>` and no Python traceback, before
      launching the app or the HTTP server; the same for a top-level value that is
      not an object. measure.ps1 does the same check after :47 (exit 2, one line).
      Verify the sh case locally with any existing file as the binary argument;
      record the command and output in `## Notes`.
- [ ] measure.sh: path resolution is immune to `CDPATH` (for example
      `CDPATH=/tmp scripts/measure/measure.sh <bin> <out>` resolves the same
      absolute paths); `cd` output cannot leak into `bin` or `budgets`.
- [ ] measure.sh header (:11-12) says "exit 1" consistently with :26-27.
- [ ] `shellcheck scripts/measure/measure.sh` exits 0 (SC2329 on `cleanup`
      silenced with a targeted `# shellcheck disable=SC2329` comment saying it is
      trap-invoked).
- [ ] measure.yml: the `measure` job has `timeout-minutes: 45`.
      `continue-on-error` and the matrix are unchanged.
- [ ] A full local run still works:
      `cargo build --release -p wisp-app` then
      `RENDERER=hardware scripts/measure/measure.sh target/wt-68/release/wisp logs/68-measure.json`
      exits 0 and the JSON has `renderer_device_source: null` on macOS. No pwsh is
      installed on this Mac; measure.ps1 is checked by reading the diff here and by
      the PR's `measure.yml` Windows job.
- [ ] Binary size: no Rust change; no impact.
- [ ] Do not open a PR or push; commit on `task/68-measure-script-fixes` only.

## Out of scope
- The macOS x64 runner hang and any change to `tolerate_failure` or the matrix.
- Budget values in `scripts/measure/budgets.json`; build profile, `.cargo/config.toml`.
- Making the vulkaninfo fallback pick GPUI's actual device (only label the source).
- `WISP_TRACE window opened` timing (09) — app code.
- docs/architecture.md and other docs (scribe adds the new key there).

## Notes

- Bad budgets file (sh): `BUDGETS=<file with "{not json"> scripts/measure/measure.sh /bin/ls out.json`
  printed `budgets file is not valid JSON: <path>` and exited 2; same for a file
  containing `[1]`. With `CDPATH=/tmp` the paths still resolved and the run was fine.
- `shellcheck scripts/measure/measure.sh` exits 0.
- Local macOS run: `CDPATH=/tmp RENDERER=hardware scripts/measure/measure.sh target/wt-68/release/wisp logs/68-measure.json`
  exit 0, `renderer_device_source: null`, `within_budget: true`.
- measure.yml run 41651470290 on the branch: Windows, Linux, aarch64 macOS jobs
  succeeded. Windows JSON had `private_working_set_kib` {9204, 13040} and
  `renderer_device_source: null`; Linux had `renderer_device_source: "vulkaninfo"`.
  The Windows CIM-failure path was not triggered (counter present), so it is
  verified by reading only. The x86_64 macOS job hung as known; I cancelled the run.
