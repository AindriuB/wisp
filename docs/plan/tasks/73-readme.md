# 73 — Write the repository README for 0.1.0

**Repo:** `.`
**Executed by:** `scribe` (README is a doc; CLAUDE.md rule 6). It still goes through
`/review` and `/verify` and gets its own PR.
**Depends on:** 71, 72
**Base:** `main` at `5dc4dd3`; before starting, merge `origin/main` containing tasks 70, 71
and 72 into this branch.
**Owns:**
- README.md (new)
- docs/plan/tasks/73-readme.md (the `## Notes` section only)

## Goal
The repository has no README. Before 0.1.0 is published it needs one that tells
someone who has never seen Wisp what it is, how to install it on each platform,
how to open an unsigned build, what Linux needs at runtime, how well each
platform has been tested, and its license.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:1-40 — what Wisp is (portable,
  lightweight HTTP client, collections as TOML files in a folder); :200-205 the
  "Not in v1" list.
- docs/architecture.md#budgets — binary size and memory figures, if cited.
- docs/plan/tasks/72-release-packages.md (retired by the scribe at /record; read it with `git log --all -- docs/plan/tasks/72-release-packages.md` and `git show`) — the asset contract: archive
  names, folder layout, `SHA256SUMS`, bundle id.
- docs/plan/tasks/71-measure-screenshot.md `## Notes` (retired the same way; read from git history) — the Linux
  package list derived from what the running binary loaded, and the screenshot run id.
- THIRD-PARTY-NOTICES, LICENSE, NOTICE (on main after 70).

## Acceptance
- [ ] `README.md` exists at the repo root and opens with one paragraph saying what Wisp
      is, in the terms of the spec.
- [ ] An Install section with one subsection per platform (macOS Apple silicon, Windows
      x64, Linux x64) names the exact asset from task 72's contract, the commands to
      verify it against `SHA256SUMS` (`shasum -a 256 -c` on macOS, `Get-FileHash` on
      Windows, `sha256sum -c` on Linux), and where to put the extracted files.
- [ ] The macOS subsection states that Apple silicon is required and Intel Macs are not
      supported, and how to open the unsigned app: System Settings → Privacy & Security →
      "Open Anyway" after the first blocked launch (on macOS 15 and later Control-click →
      Open no longer bypasses Gatekeeper), or `xattr -d com.apple.quarantine Wisp.app`.
      The Windows subsection gives SmartScreen's "More info" → "Run anyway".
- [ ] The Linux subsection lists the runtime packages, with Debian/Ubuntu package names,
      taken from task 71's package list (Vulkan loader and a Vulkan driver, X11/xcb,
      Wayland, xkbcommon, fontconfig/freetype, and any other library that list shows).
      Every package named in the README appears in that list, or the README marks it as a
      driver the list cannot show. It states the minimum glibc of the CI-built binary,
      as read from the binary's symbol versions (`objdump -T`) in a release artefact, and Notes
      records that output.
- [ ] A Platform testing section says: macOS Apple silicon and Windows x64 were hand-tested
      by the maintainer before release; Linux x64 is tested in CI only (build, tests,
      measured run under Xvfb with software Vulkan, and a screenshot of the window
      inspected), with no hand test.
- [ ] A License section says Wisp is Apache-2.0 and links `LICENSE`, `NOTICE` and
      `THIRD-PARTY-NOTICES`.
- [ ] Screenshots are optional; any image the README links exists in the repo at a path
      in this task's Owns (so none, unless Owns is amended first).
- [ ] The prose follows `docs/conventions.md#documentation`, and every link in the README
      resolves to a file on this branch.

## Out of scope
- Any file other than README.md, including `docs/` updates (those are done at /record of
  tasks 70-72).
- Build-from-source instructions beyond one line pointing at `rust-toolchain.toml` and
  `cargo build --release -p wisp-app`.
- Release notes for the GitHub Release (the orchestrator writes them at publication).

## Notes

## Amendment — user decisions (2026-11-03)

Confirmed by the user: bundle id `io.github.aindriub.wisp`; ad-hoc signing (`codesign --sign -`) of the .app is acceptable within "unsigned, no Developer ID"; Linux binary built on ubuntu-latest, README states the glibc floor (Ubuntu 24.04 or newer). Gatekeeper instructions use System Settings → Privacy & Security → Open Anyway plus `xattr -d com.apple.quarantine`, not Control-click → Open.

### Notes (implementer, 2026-11-03)

Merged origin/main, task/71 and task/72 into this branch; the 71/72 merge conflicted only in HISTORY.md, HISTORY-INDEX.md and the spec's platform note, resolved by keeping both sides.

Linux package list is from measure run 41686028996 (`measure-x86_64-unknown-linux-gnu.packages.txt`). The README names only packages that appear in it (libvulkan1, libxcb1, libx11-6, libx11-xcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libgcc-s1, libc6) plus mesa-vulkan-drivers, which is in the list as the driver. fontconfig and freetype do not appear in the list, so they are not named. The binary's NEEDED entries are only libxcb, libxkbcommon, libxkbcommon-x11, libgcc_s, libm and libc; Wayland and X11 libraries come in through the graphics stack.

glibc floor, read from the release artefact `Wisp-0.1.0-linux-x64.tar.gz` of dry-run 41690366878 with `objdump -T wisp`: highest versions are GLIBC_2.43 (six symbols: asinf, acosf, coshf, sinhf, atan2f, acoshf), GLIBC_2.39 (pidfd_spawnp, pidfd_getpid), GLIBC_2.35, GLIBC_2.34 (15). So the floor is glibc 2.43, not Ubuntu 24.04 (glibc 2.39) as the amendment assumed; ubuntu-latest is now 26.04. The README says glibc 2.43 or newer, Ubuntu 26.04. Not verified: that Ubuntu 26.04 ships 2.43 (inferred from the libgallium 26.0.8-1ubuntu0.3 in the library list).

Not confirmed: the Windows hand-test and macOS hand-test statements are written as the state at release, per the orchestrator. The tag-push release path and the assets' presence on a published release have not run, so the Install steps assume the archives sit beside `SHA256SUMS` after download. Windows checksum check is a manual compare (Get-FileHash plus Select-String), since PowerShell has no `-c` equivalent.

### Correction (2026-11-03)

Task 75 pinned Linux builds to ubuntu-24.04 and added `scripts/release/check-glibc.sh`, so the glibc floor is 2.39, not 2.43 (source: docs/architecture.md, Release section on main). The README now says "glibc 2.39 or newer (Ubuntu 24.04 or newer, or an equally recent distribution)"; the package list is unchanged. The README sentence about the missing app icon was removed, since task 76 is still in progress. The branch was merged with origin/main, keeping main's HISTORY.md, HISTORY-INDEX.md, PLAN.md, architecture.md and spec, so the branch's own content is README.md and this task file.

## Review round 1 — findings

Reviewed HEAD: 54ce05b.

- [P2] README.md:65 — "Wisp writes nothing outside the collection folder and `settings.toml`" is false: response bodies over the memory limit spill to a temp file under the OS temp dir (crates/core/src/body.rs:13, :86-133; docs/architecture.md:163 calls it "the one exception"), Unix startup creates and deletes a `.wisp-probe-*` file there (body.rs:75), and Save writes wherever the user picks. Reword to match.
- [P3] README.md:17-24 — Terminal `unzip` doesn't carry the quarantine flag (the zip is built with `ditto --noqtn`), so the block and the xattr step only apply when the zip is opened in Finder; and Finder quarantines every file in the bundle. Say the block happens when extracted with Finder, and use `xattr -dr com.apple.quarantine Wisp.app` (orchestrator decision: `-dr` supersedes the amendment's `-d`).
- [P3] README.md:65 — portable mode: user decision 2026-11-03, portable mode is Windows and Linux only for 0.1.0; macOS always uses ~/Library/Application Support/wisp. Say so.
- [P3] README.md:57 — "the X11/xcb and xkbcommon libraries it links directly": the binary's NEEDED entries are libxcb, libxkbcommon, libxkbcommon-x11, libgcc_s, libm, libc; libx11-6/libx11-xcb1 come via the graphics stack. Reword; also replace the awkward "A driver is something the list … can only show indirectly" with a plain statement that the Vulkan driver depends on your GPU (mesa-vulkan-drivers for AMD/Intel/software, vendor driver for NVIDIA).
- [NOTE] README.md:5 — say "Developer ID signing" rather than "code signing" (the app is ad-hoc signed). README.md:48 — make clear whether "it" is the folder or the binary.
- Out of this task (orchestrator): wisp.exe opens a console window and needs the VC++ runtime — fixed by a separate task; leave the README's Windows wording as is unless it claims the opposite.
