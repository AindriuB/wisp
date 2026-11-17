# Windows Authenticode signing (SignPath)

The release `build` job signs `wisp.exe` (and `wisp-cli.exe`, see below) through SignPath Foundation before
Package zips it, then runs `verify.ps1`. Signing turns on only when all five
settings exist. With none, the run prints a `::notice::` and ships unsigned.
With some, the run fails and names the missing ones.

## Settings (the repository owner creates every value)

| Name | Kind | Holds |
|---|---|---|
| `SIGNPATH_API_TOKEN` | secret | CI user API token |
| `SIGNPATH_ORGANIZATION_ID` | variable | organisation id |
| `SIGNPATH_PROJECT_SLUG` | variable | project slug |
| `SIGNPATH_SIGNING_POLICY_SLUG` | variable | e.g. `release-signing` |
| `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` | variable | artifact configuration slug |

## Signing wisp-cli.exe

The artifact configuration below lists both `wisp.exe` and `wisp-cli.exe`. The
owner must apply it in SignPath, then set the repository variable
`SIGNPATH_SIGNS_CLI=true`. Until then the workflow uploads only `wisp.exe`
(a configuration listing one file keeps working), ships `wisp-cli.exe` unsigned,
and adds "wisp-cli.exe in this release is not code-signed." to the draft release
notes. The release never fails because the CLI is unsigned. Once the variable is
`true`, both exes are signed, copied back and checked with `verify.ps1`.

## Artifact configuration

`artifact-configuration.xml` beside this file is the reference. Create it in
SignPath under the slug above. The signed artifact is a zip with `wisp.exe` at
its root (plus `wisp-cli.exe` when `SIGNPATH_SIGNS_CLI` is `true`).

## Manual approval

A release-signing request waits for manual approval in SignPath. The workflow
waits at most 60 minutes (`wait-for-completion-timeout-in-seconds: 3600`) and
then fails the job. SignPath Foundation needs GitHub-hosted runners.

## Swapping the provider

Replace the one step "Sign Windows exe (SignPath)" in
`.github/workflows/release.yml` (and the upload step feeding it). The new step
must leave the signed exe in `${{ runner.temp }}/signed-exe/wisp.exe`. The
mode, copy-back, verify and size steps stay as they are.
