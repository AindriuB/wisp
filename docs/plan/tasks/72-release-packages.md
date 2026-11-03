# 72 — Make release.yml produce a GitHub Release with packaged archives and checksums

**Repo:** `.`
**Depends on:** 70
**Base:** `main` at `5dc4dd3`; before starting, merge `origin/main` containing task 70 into
this branch (it supplies LICENSE, NOTICE, THIRD-PARTY-NOTICES and version 0.1.0).
**Build:** `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-72` (local runs only; CI uses its own)
**Owns:**
- .github/workflows/release.yml
- scripts/release/** (new: packaging script(s) and the Info.plist template)
- docs/plan/tasks/72-release-packages.md (the `## Notes` section only)

## Goal
Today `release.yml` builds four targets and uploads bare binaries as workflow
artefacts. For 0.1.0 it builds the three release targets (Intel macOS is
dropped, user decision 2026-11-03) exactly as `measure.yml` builds them,
packages each into a user-facing archive carrying LICENSE, NOTICE and
THIRD-PARTY-NOTICES, writes SHA-256 checksums, and attaches them to a GitHub
Release. **Decision (planner):** a `v*` tag push creates a **draft** release;
the orchestrator publishes it after checking it (`gh release edit <tag> --draft=false`).
A draft means a half-uploaded or wrong release is never public. A
`workflow_dispatch` run builds the same assets without a tag, so the whole
path can be tested without publishing anything.

## Context
- .github/workflows/release.yml:1-67 — current workflow; the `x86_64-apple-darwin` row is :24-28.
- .github/workflows/measure.yml:45-79 — the steps to mirror up to and including
  `Build`: checkout, Linux apt list, Metal toolchain, `rustup show active-toolchain ||
  rustup toolchain install`, `rustup target add`, and
  `cargo build --release --locked -p wisp-app --target ${{ matrix.target }}`.
  The release profile (Cargo.toml:9-17) and `.cargo/config.toml` Linux link flags apply
  only if no `RUSTFLAGS` is set (docs/architecture.md:195).
- crates/app/Cargo.toml:7-9 — the binary is `wisp` (`wisp.exe` on Windows).
- No app icon asset exists in the repo; deriving one is out of scope for 0.1.0, so the
  bundle has no `CFBundleIconFile` (macOS shows the generic icon).
- `gh` is preinstalled on GitHub-hosted runners; use `GH_TOKEN: ${{ github.token }}`.

### Asset contract (task 73's README depends on these names)
`<v>` is the `wisp-app` version from `cargo metadata` (0.1.0), never hard-coded.

| Asset | Contents (one top-level folder `Wisp-<v>-<platform>/`) |
|---|---|
| `Wisp-<v>-macos-arm64.zip` | `Wisp.app/`, `LICENSE`, `NOTICE`, `THIRD-PARTY-NOTICES` |
| `Wisp-<v>-windows-x64.zip` | `wisp.exe`, `LICENSE`, `NOTICE`, `THIRD-PARTY-NOTICES` |
| `Wisp-<v>-linux-x64.tar.gz` | `wisp` (mode 0755), `LICENSE`, `NOTICE`, `THIRD-PARTY-NOTICES` |
| `SHA256SUMS` | one `<hex>  <asset>` line per archive, `sha256sum -c` format |

`Wisp.app/Contents/MacOS/wisp` is the binary; `Contents/Info.plist` has
`CFBundleName` and `CFBundleDisplayName` `Wisp`, `CFBundleIdentifier`
`io.github.aindriub.wisp`, `CFBundleExecutable` `wisp`, `CFBundlePackageType` `APPL`,
`CFBundleShortVersionString` and `CFBundleVersion` `<v>`, `LSMinimumSystemVersion` equal to
the binary's own minimum OS as read from its load commands (not a guessed constant),
`NSHighResolutionCapable` true and `LSApplicationCategoryType`
`public.app-category.developer-tools`. The bundle is ad-hoc signed
(`codesign --force --sign - Wisp.app`) after assembly, so the downloaded bundle is not
reported as damaged; this is not Developer ID signing or notarisation. The macOS zip is
made with `ditto -c -k --keepParent`.

## Acceptance
- [ ] `rg -n 'x86_64-apple-darwin|macos-26-intel' .github/workflows/release.yml` finds nothing;
      the matrix is exactly the three targets above on the same runners as `measure.yml`.
- [ ] For each target, the release build steps from checkout through `Build` are identical
      to `measure.yml`'s, except that `release.yml` need not install `xvfb`,
      `mesa-vulkan-drivers` or the graphics diagnostics. The `Build` command is byte-identical, and
      `rg -n 'RUSTFLAGS|CARGO_PROFILE|CARGO_BUILD|CARGO_ENCODED' .github/workflows/release.yml`
      finds nothing.
- [ ] Triggers are `push: tags: ['v*']` and `workflow_dispatch` with a boolean input
      `draft_release` (default false). Workflow-level permissions stay `contents: read`; only
      the job that creates the release has `contents: write`.
- [ ] Each build job packages its archive per the asset contract and uploads it as a
      workflow artefact. A final job on `ubuntu-latest` downloads the three, writes
      `SHA256SUMS`, and uploads all four as one artefact named `wisp-<v>-release`.
- [ ] On a tag push the final job fails before creating anything unless the tag equals
      `v<v>`, then runs `gh release create <tag> --draft --verify-tag --title "Wisp <v>"`
      with the four assets. On `workflow_dispatch` with `draft_release: true` it creates a
      draft release named `dryrun-<run_id>` targeting `${{ github.sha }}` with the same four
      assets. With `draft_release: false` no release is created.
- [ ] `gh workflow run release.yml --ref task/72-release-packages -f draft_release=true` has
      run, and Notes records its run id, head commit and conclusion `success`. From that run:
      the draft `dryrun-<run_id>` existed with exactly the four assets (Notes records
      `gh release view dryrun-<run_id> --json assets,isDraft`), and it was then removed with
      `gh release delete dryrun-<run_id> --yes --cleanup-tag`.
- [ ] On the downloaded `wisp-0.1.0-release` artefact, Notes records the output of:
      `shasum -a 256 -c SHA256SUMS` (all OK); the listing of each archive (`unzip -l`,
      `tar -tzvf`) matching the contract, including mode `-rwxr-xr-x` for `wisp` in the
      tar; `file` on the three binaries (Mach-O arm64, PE32+ x86-64, ELF x86-64);
      `plutil -lint` and `plutil -extract CFBundleShortVersionString raw` (`0.1.0`) on the
      Info.plist; `codesign --verify --deep --strict Wisp.app` (no output, exit 0).
- [ ] On this Mac, the unzipped `Wisp.app/Contents/MacOS/wisp` run with
      `WISP_AUTOSEND_URL=https://example.com` prints a `WISP_EVENT completed` line, and
      `open Wisp.app` shows the Wisp window (Notes says so). This is the binary the user
      will hand-test.
- [ ] Notes records the macOS and Linux binary sizes from the run next to `measure.yml`'s
      most recent figures for the same commit range (arm64 10,420,080 B, Linux
      23,353,896 B at `4ba28c1`), as evidence that the build is the same.
- [ ] The tag-push path is not run. Notes names the step lines that a reviewer reads to
      check the tag/version guard and the `--draft --verify-tag` call.

## Out of scope
- Pushing a tag, publishing a release, or leaving any draft behind (the orchestrator
  tags and publishes after the user's hand tests).
- Code signing with an identity, notarisation, dmg/msi/deb/AppImage installers,
  auto-update, Intel macOS, aarch64 Linux.
- An app icon (none exists); `CFBundleIconFile` stays absent.
- `measure.yml`, `budgets.json` and `measure.sh` (task 71); `ci.yml`, LICENSE/NOTICE/
  THIRD-PARTY-NOTICES and manifests (task 70); README (task 73).
- Release notes text beyond the title (`gh release create` may use `--notes ""` or
  `--generate-notes`; the orchestrator edits notes before publishing).
- Docs. For /record the scribe should add a Release section to `docs/architecture.md`
  describing the triggers, the draft-on-tag rule, the dispatch dry run, the asset contract
  and the bundle id, and update the CI line of the spec
  (docs/plan/specs/2026-10-29-http-client-v1.md:214) with a dated note.

## Notes

- Dry run: run 41686673200, head commit b3bacbff45b20389d5af4765967f816fe32bb91f, conclusion success (all 4 jobs). Artefact `wisp-0.1.0-release`.
- `gh release view dryrun-41686673200 --json assets,isDraft`: isDraft true; assets SHA256SUMS (280 B), Wisp-0.1.0-linux-x64.tar.gz (9,813,028), Wisp-0.1.0-macos-arm64.zip (5,481,043), Wisp-0.1.0-windows-x64.zip (5,843,025). Then `gh release delete dryrun-41686673200 --yes --cleanup-tag`: release gone (`gh release list` empty); the cleanup-tag call printed HTTP 422 "Reference does not exist" because a draft has no tag; no dryrun tag on origin.
- `shasum -a 256 -c SHA256SUMS`: all three OK. Listings matched the contract (one top-level folder; mac: Wisp.app/Contents/{MacOS/wisp,Info.plist,_CodeSignature}, LICENSE, NOTICE, THIRD-PARTY-NOTICES; windows: wisp.exe + 3 notices; tar: `-rwxr-xr-x` wisp + 3 notices). No `._` AppleDouble entries (ditto uses --norsrc --noextattr --noqtn --noacl).
- `file`: Mach-O 64-bit executable arm64; PE32+ executable (console) x86-64; ELF 64-bit LSB pie executable x86-64 (stripped). `plutil -lint` OK; CFBundleShortVersionString 0.1.0; LSMinimumSystemVersion 11.0 (read from load commands); `codesign --verify --deep --strict` exit 0, no output.
- Local run of unzipped Wisp.app binary with WISP_AUTOSEND_URL=https://example.com: `WISP_EVENT completed status=200 size=577`. `open Wisp.app`: process "wisp" frontmost with 1 window (checked via System Events; no screenshot available, so window appearance itself not eyeballed).
- Sizes (this run vs measure.yml at 4ba28c1): macOS arm64 10,377,632 B vs 10,420,080; Linux 23,358,504 B vs 23,353,896. Within 0.5%, and the commit ranges differ (tasks 70 version/licence work in between).
- Windows zip's LICENSE is 11,560 B and NOTICE 237 B vs 11,358/230 on mac/Linux: the Windows checkout converted LF to CRLF. Not fixed (adjacent; consider `.gitattributes eol=lf` in task 70's area).
- Tag-push path not run. For review read release.yml: line 106 `Check tag matches version (tag push)` (guard) and lines 112-115 `Create draft release (tag push)` with `gh release create ... --draft --verify-tag`; dry run at 117-120. Version comes from the archive names (from `cargo metadata` in package.sh), checked to agree across all three.

## Amendment — user decisions (2026-11-03)

Confirmed by the user: bundle id `io.github.aindriub.wisp`; ad-hoc signing (`codesign --sign -`) of the .app is acceptable within "unsigned, no Developer ID"; Linux binary built on ubuntu-latest, README states the glibc floor (Ubuntu 24.04 or newer). Gatekeeper instructions use System Settings → Privacy & Security → Open Anyway plus `xattr -d com.apple.quarantine`, not Control-click → Open.
