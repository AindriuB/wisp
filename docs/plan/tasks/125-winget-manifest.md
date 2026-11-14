# 125 — Generate and validate a winget manifest from release assets

**Repo:** .
**Depends on:** none
**Owns:**
- .github/workflows/winget.yml (new)
- scripts/release/winget-manifest.sh (new)
- packaging/winget/** (new: templates, README, committed manifests)

## Goal
Write a winget manifest for the Windows release zip and validate it, built from
a published release's own assets (version, download URL, SHA256). It runs in
its own workflow, outside release.yml, for two reasons. A winget manifest
should point at published assets, not a draft. And it keeps release.yml edits
serial (124, then 126). The task prepares the manifest only. Submitting it to
microsoft/winget-pkgs is the user's own outward-facing PR.

## Context
- .github/workflows/release.yml:136-145 — asset naming (`Wisp-<v>-windows-x64.zip`)
  and SHA256SUMS, the inputs to the manifest.
- scripts/release/package.sh:79-82 — the zip holds `Wisp-<v>-windows-x64/wisp.exe`
  plus three notice files.
- winget-pkgs layout: `manifests/<letter>/<Publisher>/<Name>/<version>/` with
  the version, installer and `locale.en-US` files. For a zip that holds a
  portable exe, use `InstallerType: zip`, `NestedInstallerType: portable`, a
  `NestedInstallerFiles` entry, and `PortableCommandAlias: wisp`.
- v0.1.0 is published (PLAN.md "v0.1.0"). Its real assets are the test input.

## Acceptance
- [ ] AC1 `scripts/release/winget-manifest.sh <version> <SHA256SUMS> <outdir>`
  writes the three manifest files under
  `<outdir>/manifests/<letter>/<Publisher>/<Name>/<version>/`. It takes the
  zip's SHA256 from SHA256SUMS (uppercased, as winget expects). The URL is
  `https://github.com/AindriuB/wisp/releases/download/v<version>/Wisp-<version>-windows-x64.zip`.
  The script exits non-zero if the zip's line is missing or the version is
  malformed.
- [ ] AC2 The templates live in `packaging/winget/templates/`. They pin one
  manifest schema version and set `Architecture: x64`,
  `License: Apache-2.0`, `PackageName: Wisp` and `Publisher` and
  `PackageIdentifier` from one `@PACKAGE_IDENTIFIER@` and `@PUBLISHER@`
  pair, defined once in the script. Their defaults are `AindriuB.Wisp` and
  `AindriuB`, and the README marks both as values the user confirms.
- [ ] AC3 `.github/workflows/winget.yml` runs on `release: published` and on
  `workflow_dispatch` with a `version` input. It downloads SHA256SUMS from that
  release with `gh release download`, runs the script, and validates the result
  on `windows-latest`. It uses `winget validate --manifest <dir>` when `winget`
  is on PATH. Otherwise it checks each file against the pinned JSON schemas,
  using a validator pinned by version. A `::notice::` names which check ran. It
  uploads the manifest directory as an artifact. Permissions are
  `contents: read`. The workflow uses no secrets beyond `github.token`, and it
  opens no PR and pushes nothing.
- [ ] AC4 The manifest for v0.1.0 is generated from v0.1.0's published
  SHA256SUMS and committed under `packaging/winget/manifests/...`. A
  dispatch run of `winget.yml` with `version=0.1.0` validates it, and the
  workflow's artifact is byte-identical to the committed files. That run can
  only happen after merge, because GitHub dispatches only workflows that are
  already on the default branch. Before merge, AC7's pull_request run is the
  proof.
- [ ] AC5 `packaging/winget/README.md` gives the user's steps after a release:
  run or await the workflow, commit the new version's manifest by PR here, then
  open the winget-pkgs PR themselves (fork, copy the directory, sign the CLA
  once). It also notes that winget installs the zip's exe as a portable command,
  `wisp`.
- [ ] AC6 Nothing outside `Owns` changes.
- [ ] AC7 `winget.yml` also runs on `pull_request` when files under its own
  paths change (`packaging/winget/**`, the script, the workflow). That run
  validates the committed manifests on `windows-latest` with the same AC3
  logic and downloads nothing. It is green on this task's PR.

## Checks
- `script` command — AC1, AC4
  run: `gh release download v0.1.0 -p SHA256SUMS -D "$TMPDIR/w" && scripts/release/winget-manifest.sh 0.1.0 "$TMPDIR/w/SHA256SUMS" "$TMPDIR/w/out" && diff -r "$TMPDIR/w/out/manifests" packaging/winget/manifests`
- `script-errors` command — AC1
  run: `! scripts/release/winget-manifest.sh 0.1.0 /dev/null "$TMPDIR/w/bad" && ! scripts/release/winget-manifest.sh v0.1 "$TMPDIR/w/SHA256SUMS" "$TMPDIR/w/bad"`
- `lint` command — AC3
  run: `actionlint .github/workflows/winget.yml && shellcheck scripts/release/winget-manifest.sh`
- `validate-pr` command — AC3, AC7
  run: `gh run list --branch task/125-winget-manifest --workflow winget.yml --limit 1 --json databaseId`,
  then `gh run watch <id> --interval 60 --exit-status`
- `validate-dispatch` command — AC4 (after merge, by the orchestrator)
  run: `gh workflow run winget.yml --ref main -f version=0.1.0`, then `gh run watch <id> --interval 60 --exit-status`
- `templates-readme` read — AC2, AC5
  how: the identifiers are defined once, and the README names them as the user's to confirm.
- `scope` command — AC6
  run: `git diff --exit-code main...HEAD -- . ':!.github/workflows/winget.yml' ':!scripts/release/winget-manifest.sh' ':!packaging/winget'`

## Out of scope
- Opening any PR against microsoft/winget-pkgs, or using wingetcreate or komac
  with a token.
- release.yml (owned by 124 and 126).
- MSIX or Store manifests (task 126).
