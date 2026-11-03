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
