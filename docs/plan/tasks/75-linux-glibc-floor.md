# 75 — Pin Linux builds to ubuntu-24.04 and guard the glibc floor

**Repo:** .
**Depends on:** none
**Owns:**
- .github/workflows/release.yml
- .github/workflows/measure.yml
- .github/workflows/ci.yml
- scripts/release/** (new glibc guard script; `package.sh` and `Info.plist.template` untouched)
- scripts/measure/** (only if the guard is wired into `measure.yml` through a measure script; `budgets.json` untouched)

## Goal
GitHub's `ubuntu-latest` is now Ubuntu 26.04, so the linux-x64 binary from
release dry run 41690366878 needs GLIBC_2.43 (`asinf`, `acosf`, `coshf`,
`sinhf`, `atan2f`, `acoshf` per `objdump -T`) and 0.1.0 would only run on very
new distros. User decision (2026-11-03): the Linux floor is Ubuntu 24.04 or
newer, glibc 2.39. Build and measure every Linux binary on `ubuntu-24.04`, and
make the release fail if the binary ever needs a glibc symbol version above
2.39 again.

## Context
- .github/workflows/release.yml:33-37 — Linux matrix row (`os: ubuntu-latest`); :38-52 — apt list (zed `script/linux` at rev 08cbb88a…, `libstdc++-14-dev` noted as "the Ubuntu 24.04 extra"); :63-67 — Build then Package, where the guard goes between them; :76 — `release` job also on `ubuntu-latest` (does not build a binary)
- .github/workflows/measure.yml:31-34 — Linux matrix row; :38-58 — apt list, Xvfb/mesa/vulkan installs (`xvfb mesa-vulkan-drivers imagemagick`, then best-effort `libegl1 libgl1 libgl1-mesa-dri libegl-mesa0 vulkan-tools`); :71-77 — `xvfb-run` measurement
- .github/workflows/ci.yml:18-19 — `check` matrix `[ubuntu-latest, macos-latest]`; :50-51 — `notices` job (runs `cargo-about` over metadata, builds no app binary)
- scripts/release/package.sh — style to follow for a new bash script (`set -euo pipefail`, usage line, explicit exit codes)
- scripts/measure/budgets.json:12-16 — Linux budgets: binary 29,360,128 B, idle and after-request anon RSS 122,880 / 131,072 KiB
- docs/plan/HISTORY.md, entry `## 2026-11-03 — Release workflow builds packaged archives into a draft GitHub Release` — states the floor as "Ubuntu 24.04 or newer" on ubuntu-latest
- docs/conventions.md — commit format and script style

## Acceptance
- [ ] In `release.yml`, `measure.yml` and `ci.yml`'s `check` job the Linux runner is the literal `ubuntu-24.04`; `rg -n 'ubuntu-latest' .github/workflows/` returns only jobs that build no Linux app binary (release.yml `release`, ci.yml `notices` may stay), and a comment on each pinned row says why (glibc floor 2.39). The Linux build steps (apt list, toolchain, `cargo build --release --locked -p wisp-app --target x86_64-unknown-linux-gnu`) are identical in `release.yml` and `measure.yml`.
- [ ] New `scripts/release/check-glibc.sh <binary> [max, default 2.39]` extracts every `GLIBC_x.y[.z]` version the binary requires (`objdump -T` or `readelf --version-info`), compares numerically (so 2.4 < 2.39 < 2.43), prints the highest one, exits 0 when it is ≤ max and non-zero naming the offending version (and symbols) when it is above. `release.yml`'s Linux job runs it after Build and before Package; `measure.yml`'s Linux job runs it too unless that is not cheap, in which case Notes say why.
- [ ] Dry run `gh workflow run release.yml --ref task/75-linux-glibc-floor -f draft_release=true` succeeds with the guard step passing; `objdump -T` on the downloaded linux-x64 binary shows max GLIBC ≤ 2.39; the `dryrun-<run_id>` draft release is then deleted (`gh release delete dryrun-<run_id> --yes --cleanup-tag`). No `v*` tag is pushed and nothing is published. No PR is opened.
- [ ] `gh workflow run measure.yml --ref task/75-linux-glibc-floor` succeeds with all three targets measured and within `budgets.json`, and the Linux job uploads a screenshot showing the rendered window (not black/blank). `ci.yml` is green on the branch (both `check` rows and `notices`). If any apt package is missing or renamed on 24.04 the list is corrected and the change is noted.
- [ ] Task Notes record: dry-run, measure and ci run ids; max GLIBC before (2.43, run 41690366878) and after; Linux `binary_bytes`, idle and after-request RSS before (latest main `measure.yml` run, id cited) and after; a negative guard check — `scripts/release/check-glibc.sh <linux binary> 2.30` exits non-zero and prints the offending version — with its output.

## Out of scope
- README floor wording — task 73 owns README; the orchestrator updates it separately.
- Docs for the scribe at /record, not for the implementer: correct docs/architecture.md:263 (Release section, "built on ubuntu-latest, so its glibc floor is Ubuntu 24.04 or newer") to the measured floor and the pinned `ubuntu-24.04` runner and why; amend the task-72 HISTORY entry's "Linux built on ubuntu-latest so the glibc floor is Ubuntu 24.04 or newer" wording; note in the HISTORY entry that `libstdc++-14-dev` matches the 24.04 runner again.
- Lowering the floor below glibc 2.39 (older runner, container build, zig/cargo-zigbuild, musl) — not requested.
- `scripts/measure/budgets.json`, `scripts/release/package.sh` and any crate source — even if the Linux size or RSS shifts, report it, do not retune budgets.
- macOS and Windows runner choices (`macos-latest`, `windows-latest`).
- Pushing a tag, publishing a release, or opening a PR.

## Notes
- Runs on task/75-linux-glibc-floor: release dry run 41697405027 (draft dryrun-41697405027, deleted with its tag), measure 41697410452, ci 41697404106 (all success).
- Max GLIBC: before 2.43 (run 41690366878), after 2.39 (guard step in release and measure passed).
- Linux measure before (main code, run 41686669384, task/74 at c05f6ae, ubuntu-latest): binary_bytes 23,359,144; rss_anon idle 53,376 KiB, after-request 45,096 KiB. After: binary_bytes 23,358,512 (-632 B, unchanged); rss_anon idle 61,132 KiB (+7,756 KiB, +14.5%), after-request 51,256 KiB (+6,160 KiB, +13.7%) (budgets 29,360,128 / 122,880 / 131,072); macOS and Windows also within budget. Screenshot shows the rendered window.
- Guard runs in measure.yml too (a few ms, reads the already-built binary).
- Negative check: `check-glibc.sh wisp 2.30` exits 1, prints "highest GLIBC required: 2.39 (max allowed: 2.30)", "FAIL: ... needs GLIBC_2.39 ...", offending symbols incl. GLIBC_2.39 pidfd_spawnp, pidfd_getpid. `2.4` also fails (numeric compare); default 2.39 exits 0.
- No apt package changes were needed on 24.04.

## Review round 1 — findings

Reviewed HEAD: 3f66af1.

- [P2] Notes, "Linux measure before" — cites run 40964848611 (2026-10-29, pre-task-64, 35,133,064 B, RSS null), which is not the latest measurement of main's code. Use run 41686669384 (task/74 at c05f6ae, merged as 4bf7098, ubuntu-latest): binary_bytes 23,359,144; rss_anon_kib idle 53,376 / after-request 45,096. State the deltas against the after-figures: size -632 B (unchanged); RssAnon idle +7,756 KiB (+14.5%), after-request +6,160 KiB (+13.7%) — all within budget.
- [P3] scripts/release/check-glibc.sh:29-34 — the inner `echo "  $v $sym" >&2` bypasses the `| sort -u >&2` pipe, so offending symbols print unsorted with duplicates. Drop the inner `>&2`.
- [P3] scripts/release/check-glibc.sh:15 — if objdump fails (non-ELF/corrupt input) the script exits 1, the "above max" code, instead of the documented 2. Check objdump's status and exit 2 with a message. Input: `check-glibc.sh README.md`.
- [NOTE, optional] scripts/release/check-glibc.sh:14 — the comment "Undefined (required) only" doesn't match the code (no `*UND*` filter); fix the comment.
