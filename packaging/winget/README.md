# winget manifest

The manifest for the Windows zip is generated from a published release's own
assets (version, URL, SHA256) by `scripts/release/winget-manifest.sh`, using the
templates in `templates/`. `.github/workflows/winget.yml` runs it and validates
the result. Nothing here submits anything to winget-pkgs.

## Values you confirm

Both are defined once, at the top of `scripts/release/winget-manifest.sh`:

- `PACKAGE_IDENTIFIER` (default `AindriuB.Wisp`)
- `PUBLISHER` (default `AindriuB`)

These are placeholders until you confirm them. Once a version is in
winget-pkgs the identifier cannot change.

## After each release

1. The workflow runs on `release: published`. To run it by hand:
   `gh workflow run winget.yml -f version=<x.y.z>`.
2. Download the `winget-manifest` artifact and copy its
   `a/AindriuB/Wisp/<version>/` directory into `packaging/winget/manifests/a/AindriuB/Wisp/`.
   Open a PR here with it. The pull_request run re-validates the committed files.
3. Open the winget-pkgs PR yourself: fork `microsoft/winget-pkgs`, copy the
   version directory to `manifests/a/AindriuB/Wisp/<version>/`, and open the PR.
   Sign the Microsoft CLA once, on your first PR.

## What users get

The zip holds `wisp.exe` and `wisp-cli.exe` inside `Wisp-<version>-windows-x64/`. winget installs it
as a portable command, so `winget install AindriuB.Wisp` gives the commands `wisp` and `wisp-cli`.
