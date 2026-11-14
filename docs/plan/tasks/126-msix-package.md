# 126 — Build an MSIX package for the Microsoft Store

**Repo:** .
**Depends on:** 124 (release.yml edits are serial)
**Owns:**
- .github/workflows/release.yml (the `build` job's Windows steps only; added after 124's steps)
- packaging/msix/** (new: manifest template, packing script, STORE-SUBMISSION.md)
- scripts/icon/generate.sh (insertions only: the MSIX logo renders)
- assets/icon/msix/** (new: generated logos)

## Goal
Release runs should also produce an MSIX of the Windows build, as a separate
workflow artifact the user can upload to Partner Center. Store identity values
come from repository variables. Where a value is absent, the run uses a marked
placeholder and still produces the package. The task also ships a Store
submission checklist. It sits under `packaging/msix/`, not `docs/`, so the
implementer can own it. The scribe links it from `docs/architecture.md#release`
at /record.

## Context
- .github/workflows/release.yml as left by 124: the Windows exe, signed or
  not, sits at `target/x86_64-pc-windows-msvc/release/wisp.exe` before Package.
- .github/workflows/release.yml:136-140 — the `release` job downloads `pkg-*`
  and expects exactly 3 archives. The MSIX artifact must not match `pkg-*`.
- scripts/icon/generate.sh:8,37-38 — the size cut (at most 48 px from
  `wisp-small.svg`, the rest from `wisp.svg`) and the render helper.
- scripts/release/package.sh:82 — the zip layout. The MSIX carries the same
  `wisp.exe` and the three notice files.
- Full-trust desktop app: `Windows.FullTrustApplication`, `rescap:runFullTrust`,
  `internetClient`, `TargetDeviceFamily Windows.Desktop`.

## Store identity (repository variables; the user sets them from Partner Center)
| Variable | Manifest field | Placeholder when unset |
|---|---|---|
| `MSIX_IDENTITY_NAME` | `Identity/@Name` | `Wisp.Placeholder` |
| `MSIX_PUBLISHER` | `Identity/@Publisher` (`CN=...`) | `CN=WispPlaceholder` |
| `MSIX_PUBLISHER_DISPLAY_NAME` | `PublisherDisplayName` | `Wisp placeholder publisher` |

## Acceptance
- [ ] AC1 `packaging/msix/AppxManifest.xml.template` declares one full-trust
  application with `Executable="wisp.exe"`, the capabilities `runFullTrust`
  and `internetClient` only, `Windows.Desktop` with a stated MinVersion, the
  three identity fields as `@...@` tokens, and the version as `@VERSION@`.
- [ ] AC2 `scripts/icon/generate.sh` also renders the logos the manifest names
  (44x44, 50x50 and 150x150 at minimum) into `assets/icon/msix/`, following
  its size cut. The committed PNGs equal its output, so `generate.sh --check`
  passes. Existing outputs are byte-identical.
- [ ] AC3 `packaging/msix/pack.ps1` stages `wisp.exe`, the notice files and the
  logos, fills the template, and maps `<v>` to `<v>.0` (a four-part version
  ending in 0). It finds `makeappx.exe` in the runner's Windows SDK and packs
  `Wisp-<v>-windows-x64.msix`. It fails if makeappx reports any error.
- [ ] AC4 release.yml runs `pack.ps1` on the Windows leg after 124's signing
  and verify steps, using the same exe Package zips. It uploads the MSIX as an
  artifact named `msix-windows-x64`, and does not attach it to the release or
  to SHA256SUMS. For each unset variable it emits
  `::notice::MSIX uses placeholder <field>; not Store-uploadable`.
- [ ] AC5 124's steps, the macOS and Linux steps, and the `release` job are
  unchanged. The 15,500,000 B exe gate from 124 still runs. This task does not
  change the exe.
- [ ] AC6 `packaging/msix/STORE-SUBMISSION.md` is a checklist for the user. It
  covers the steps that are the user's alone: creating the Partner Center
  account, reserving the name, and setting the three variables from Partner
  Center's Product identity page. It covers what the Store needs that the
  repo does not hold: a privacy policy URL (Wisp makes network requests), age
  rating, screenshots and the `runFullTrust` justification. It also covers
  uploading the `msix-windows-x64` artifact, running WACK locally if the user
  wants to, and that the Store re-signs the package. It notes that a Store
  install keeps its `%LOCALAPPDATA%` data apart from a zip install's. Agents
  create no accounts and enter no credentials.
- [ ] AC7 A `workflow_dispatch` dry run on this branch succeeds. It uploads
  `msix-windows-x64`, the log shows the AC4 placeholder notices, and the
  `release` job still finds exactly 3 archives.

## Checks
- `lint` command — AC4, AC5
  run: `actionlint .github/workflows/release.yml`
- `icons` command — AC2
  run: `scripts/icon/generate.sh --check`
- `manifest-and-scope` read — AC1, AC3, AC4, AC5
  how: the diff of release.yml adds Windows-only steps after 124's verify step
  and nothing else. The template has exactly the listed capabilities and tokens.
- `checklist` read — AC6
  how: every item in AC6 appears in the file, and placeholders point at the
  variables table.
- `dry-run` command — AC4, AC7
  run: `gh workflow run release.yml --ref task/126-msix-package -f draft_release=false`,
  then `gh run watch <id> --interval 60 --exit-status` and
  `gh run view <id> --json jobs` plus `gh api repos/AindriuB/wisp/actions/runs/<id>/artifacts` listing `msix-windows-x64`.

## Out of scope
- Submitting to the Store, or any Partner Center call or API.
- Signing the MSIX, `resources.pri` and scale-qualified logo variants,
  `AppExecutionAlias`, and auto-update.
- Changing the zip or the exe.
- Anything under `docs/` (scribe).
