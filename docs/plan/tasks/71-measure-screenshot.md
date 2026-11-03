# 71 — Drop Intel macOS from measurement; add a Linux window screenshot and runtime-library list

**Repo:** `.`
**Depends on:** none
**Base:** `main` at `5dc4dd3`
**Build:** `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-71` (local runs only; CI uses its own)
**Owns:**
- .github/workflows/measure.yml
- scripts/measure/budgets.json
- scripts/measure/measure.sh
- docs/plan/tasks/71-measure-screenshot.md (the `## Notes` section only)

## Goal
Intel macOS is not a 0.1.0 target (user decision 2026-11-03: macOS 27 is
Apple-silicon-only and the x64 CI hang was never fixed), so measurement stops
building and budgeting it. The user cannot hand-test Linux, so the Linux
measure job must also save a screenshot of the app window after the request
completes, and a list of the shared libraries the running app loaded, both
uploaded with the job's artefacts. README (task 73) takes its Linux
runtime-library list from that output.

## Context
- .github/workflows/measure.yml:17-43 — the matrix. The `x86_64-apple-darwin` row
  (:27-31) is the only one with `tolerate_failure: true`; `continue-on-error` reads it (:43).
- .github/workflows/measure.yml:46-55 — the Linux apt list; :80-85 the Linux measure step
  (`xvfb-run -a`, which defaults to a 640x480x8 screen, smaller than the app's 720x480
  minimum window, crates/app/src/main.rs:41,89); :99-107 the upload step.
- scripts/measure/budgets.json:7-11 — the `x86_64-apple-darwin` entry.
- scripts/measure/measure.sh:190-224 — run 2 (after one request). :217-221 is the
  after-request sample, then the app is killed. The screenshot and library list are
  taken there, after the RSS/PSS/RssAnon sample and before `kill`, so they cannot
  change the measured figures.
- Last Linux figures: `measure.yml` run 41636483942 on `4ba28c1`, binary 23,353,896 B,
  RssAnon 58,168 / 44,972 KiB (docs/architecture.md:188-195).
- docs/plan/tasks/65 (retired, in git history) set the `gh workflow run measure.yml --ref <branch>`
  pattern used below.

## Acceptance
- [ ] `rg -n 'x86_64-apple-darwin|macos-26-intel' .github/workflows/measure.yml scripts/measure/budgets.json`
      finds nothing. The other three `budgets.json` entries are unchanged (`jq` comparison
      with 5dc4dd3 of `del(."x86_64-apple-darwin")` is equal).
- [ ] With every remaining row untolerated, `measure.yml` no longer has the
      `tolerate_failure` matrix key or the `continue-on-error` line. The build step
      `cargo build --release --locked -p wisp-app --target ${{ matrix.target }}` is
      unchanged, and no step sets `RUSTFLAGS` or `CARGO_PROFILE_*`.
- [ ] `measure.sh` takes an optional `SCREENSHOT=<path.png>` environment variable. When it
      is set, the OS is Linux and `DISPLAY` is set, the script captures the X root window
      to that path after the after-request sample and before killing the app. If the
      capture fails it prints one `screenshot: <reason>` line on stderr and the result
      JSON, status and exit code are what they would have been without `SCREENSHOT`.
      When `SCREENSHOT` is unset the script's behaviour is unchanged.
- [ ] On Linux, at the same point, `measure.sh` writes `<logbase>.libs.txt`: the sorted,
      unique absolute paths of the shared objects in `/proc/<pid>/maps` of the running app.
      This file is written whether or not `SCREENSHOT` is set.
- [ ] The Linux measure step runs under an Xvfb screen of at least 1280x800 at 24-bit depth,
      passes `SCREENSHOT=measure-${{ matrix.target }}.png`, and the apt step installs the
      capture tool. A following Linux step writes
      `measure-${{ matrix.target }}.packages.txt`: for each path in the libs file, the
      Debian package that owns it (`dpkg -S`), deduplicated, plus the binary's
      `readelf -d` NEEDED entries. The upload step includes the PNG and both `.txt` files.
- [ ] `gh workflow run measure.yml --ref task/71-measure-screenshot` has run, and Notes
      records its run id and head commit. The run concludes `success` with three jobs
      (arm64, Windows, Linux) each `success`, `status: "measured"` and
      `within_budget: true`. Notes records the Linux binary size and RssAnon idle /
      after-request next to the figures above, so a change from the larger Xvfb screen
      is visible.
- [ ] The Linux artefact contains a PNG at least 1280 px wide. The implementer has viewed
      it, and Notes says what it shows (whether the window, the URL and a completed
      response status are visible and legible). The PNG itself is not committed.
- [ ] Notes contains the package list from `measure-x86_64-unknown-linux-gnu.packages.txt`
      (package names only, no paths beyond what identifies Vulkan, X11, Wayland,
      xkbcommon and fontconfig libraries), for task 73.

## Out of scope
- `release.yml` (task 72), `ci.yml` (task 70), README (task 73).
- `scripts/measure/results/x86_64-apple-darwin.json`: it is a historical record; leave it.
- `measure.ps1`, macOS or Windows screenshots, and a Wayland-backend screenshot.
- Changing the autosend payload, the settle time or the budgets.
- Docs. For /record the scribe should, in `docs/architecture.md`: change v1 platforms
  (:23) to macOS Apple silicon, Windows x64, Linux x64; remove the `x86_64-apple-darwin`
  row from the Set budgets table (:207) and the x64 sentences of :216-222 and :230-231,
  saying it was dropped for 0.1.0 and why; keep the historical x64 rows in the measured
  tables (:173, :182, :193) marked as dropped; describe the screenshot and libs artefacts.
  In `docs/plan/PLAN.md`: delete the "x86_64-apple-darwin RSS" item and the x64 parts of
  the Residuals (63) and (64) lines and the "Only macOS x64 is tolerated" sentence. In the
  spec (docs/plan/specs/2026-10-29-http-client-v1.md:22 and :214) add a dated note that
  Intel macOS was dropped from 0.1.0 on 2026-11-03.

## Notes

- Run: `measure.yml` run 41684148256, head commit 3f7e00fe30a84aff0ee9da595702e98894ab5545
  (task/71-measure-screenshot). Concluded `success`; arm64, Windows and Linux jobs each
  `success`, `status: measured`, `within_budget: true`.
- Linux (Xvfb 1280x800x24): binary 23,357,608 B (was 23,353,896); RssAnon idle 57,832 /
  after-request 45,064 KiB (was 58,168 / 44,972). No meaningful change from the larger screen.
- Screenshot: 1280x800 PNG. Shows the full app window: tab "Untitled", GET, the URL
  `http://127.0.0.1:45953/file.bin`, and in the response pane a green "200 OK · 1 ms · 64.0 KB"
  with "Binary body, 65536 bytes". All legible. Not committed.
- Capture tool: ImageMagick `import -window root` (apt `imagemagick`).
- Packages for task 73 (dpkg -S of loaded shared objects; arch suffix :amd64 omitted):
  libbsd0, libc6, libdisplay-info3, libdrm-amdgpu1, libdrm-intel1, libdrm2, libedit2,
  libegl-mesa0, libegl1, libelf1t64, libexpat1, libffi8, libgbm1, libgcc-s1, libglvnd0,
  libllvm21, libmd0, libpciaccess0, libsensors5, libstdc++6, libtinfo6, libvulkan1,
  libwayland-client0, libx11-6, libx11-xcb1, libxau6, libxcb-dri3-0, libxcb-present0,
  libxcb-randr0, libxcb-shm0, libxcb-sync1, libxcb-xfixes0, libxcb-xkb1, libxcb1, libxdmcp6,
  libxkbcommon-x11-0, libxkbcommon0, libxml2-16, libxshmfence1, libzstd1, mesa-libgallium,
  mesa-vulkan-drivers, zlib1g.
  Binary NEEDED: libxcb.so.1, libxkbcommon.so.0, libxkbcommon-x11.so.0, libgcc_s.so.1,
  libm.so.6, libc.so.6. (Mesa/llvm/drm entries come from the software Vulkan driver in CI;
  fontconfig is not loaded at runtime, and libwayland-client0 is loaded though only X11 was used.)

- Round 1 confirmation: `measure.yml` run 41686028996 on the round 1 fix commit; all three jobs `success`. Packages file now strips `:amd64`, splits multi-owner lines, writes `unowned: <path>` lines and is non-fatal; screenshot wrapped in `timeout 20`.

## Review round 1 — findings

Reviewed HEAD: 8746f4e.

- [P3] .github/workflows/measure.yml:85 — the runtime-packages pipeline runs under `bash -eo pipefail`; if any loaded `.so` isn't owned by dpkg, `dpkg -S` exits 1, `xargs` exits 123 and the Linux job fails although measurement passed; where it doesn't fail, `2>/dev/null` silently drops the unowned path. Make the step non-fatal (`|| true`) and write unowned paths as `unowned: <path>` lines.
- [NOTE, optional] same line — strip the `:amd64` suffix and split multi-owner lines so task 73 can read the file directly.
- [NOTE, optional] scripts/measure/measure.sh:236 — `import -window root` has no timeout; wrap it (e.g. `timeout 20` on the Linux runner) if cheap.
