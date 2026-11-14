# 124 — Sign the Windows exe with Authenticode through SignPath

**Repo:** .
**Depends on:** none
**Owns:**
- .github/workflows/release.yml (the `build` job's Windows steps; release.yml edits are serial, and 126 follows this task)
- scripts/release/sign-windows/** (new: verify script, SignPath artifact-configuration reference, setup notes)

## Goal
The release build should Authenticode-sign `wisp.exe` through SignPath
Foundation (free signing for open-source projects) before it is zipped, and
verify the signature afterwards. Signing turns on only when the SignPath
settings are present. Without them the run prints a `::notice::` and ships
unsigned, as today. The provider-specific call sits in one step, so another
provider can replace it without touching the steps around it. No self-signed
certificate is used.

## Context
- .github/workflows/release.yml:72-97 — "Select macOS signing mode": the pattern
  to copy (all present, none present, some present).
- .github/workflows/release.yml:105-114 — Package, then the always-run cleanup step.
- scripts/release/package.sh:79-82 — the Windows packaging copies `$bin` as-is, so
  the exe at `target/x86_64-pc-windows-msvc/release/wisp.exe` must already be
  signed when Package runs.
- docs/architecture.md#macos-signing-and-notarisation — the fallback rules.
- SignPath GitHub integration: `signpath/github-action-submit-signing-request`
  takes the `artifact-id` of an `actions/upload-artifact` upload and returns the
  signed artifact. SignPath Foundation needs GitHub-hosted runners (origin
  verification). Pin the action by commit SHA, as every action in this repo is.

## Settings (names only; the user creates every value)
| Name | Kind | Holds |
|---|---|---|
| `SIGNPATH_API_TOKEN` | secret | CI user API token |
| `SIGNPATH_ORGANIZATION_ID` | variable | organisation id |
| `SIGNPATH_PROJECT_SLUG` | variable | project slug |
| `SIGNPATH_SIGNING_POLICY_SLUG` | variable | e.g. `release-signing` |
| `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG` | variable | artifact configuration slug |

## Acceptance
- [ ] AC1 A "Select Windows signing mode" step (Windows only, after Build) sets
  `WISP_WINDOWS_SIGN` as follows:
  | Settings present | Result |
  |---|---|
  | all five | `signpath` |
  | none | `unsigned`, plus `::notice::Windows exe is unsigned because the SignPath settings are absent` |
  | some | `::error::` naming the missing ones (names only); the step fails |

  This deliberately differs from macOS: a tag push with no settings at all
  still ships unsigned, so SignPath approval timing cannot block a tag.
- [ ] AC2 Signing lives in one step, "Sign Windows exe (SignPath)", gated on
  `env.WISP_WINDOWS_SIGN == 'signpath'`. The steps before it upload the
  unsigned exe as an artifact. The steps after it put the signed exe back at
  the path Package reads. Every SignPath-specific input appears only in that
  step and in the upload step feeding it.
- [ ] AC3 A "Verify Windows signature" step runs whenever the mode is
  `signpath`. It calls `scripts/release/sign-windows/verify.ps1` and fails
  unless all of these hold: `Get-AuthenticodeSignature` reports `Valid`;
  `signtool verify /pa` passes; the signature carries an RFC 3161 timestamp;
  and the signer subject is printed (subject only, no certificate dump).
- [ ] AC4 The Windows build job fails if the `wisp.exe` that Package zips is
  over 15,500,000 B. It prints the byte size in every mode.
- [ ] AC5 Secrets never reach a log or the repo: the token appears only as
  `${{ secrets.SIGNPATH_API_TOKEN }}` in step inputs or env; no step uses
  `set -x`, `Set-PSDebug`, or echoes a secret; nothing is written under the
  checkout except the signed exe.
- [ ] AC6 macOS and Linux steps, and the `release` job, are unchanged.
- [ ] AC7 `scripts/release/sign-windows/README.md` lists the settings table
  above, the SignPath artifact configuration (kept beside it as a reference
  XML file), how to swap the provider (replace the one step from AC2), and
  the fact that a release-signing request waits for manual approval in
  SignPath. The approval wait has a bounded timeout (at most 60 minutes).
- [ ] AC8 A `workflow_dispatch` dry run with no SignPath settings succeeds,
  shows the AC1 notice, and its Windows zip holds an unsigned `wisp.exe`
  whose size AC4 printed.

## Checks
- `lint` command — AC1, AC2, AC6
  run: `actionlint .github/workflows/release.yml`
- `structure` read — AC1, AC2, AC3, AC5, AC6
  how: read the diff. There is one provider step; the mode logic matches the
  table; the macOS, Linux and `release` job hunks are empty; and no secret
  is interpolated into a `run:` body.
- `notes` read — AC7
  how: the README matches the settings table and names the swap point.
- `dry-run` command — AC4, AC8
  run: `gh workflow run release.yml --ref task/124-windows-authenticode-signing -f draft_release=false`,
  then `gh run watch <id> --interval 60 --exit-status`. The Windows log shows
  the notice and a size of at most 15,500,000.
- `signed-run` read — AC3 (deferred)
  how: after the user adds the settings, a dispatch run shows the verify step
  passing. Until then AC3 is proved by reading the code only, and the PR says so.

## Out of scope
- README or docs wording such as SignPath Foundation's code-signing-policy
  statement (scribe at /record).
- Signing the MSIX (the Store signs it; task 126) or anything on macOS.
- Requiring signing on tag pushes. Once SignPath is live, that is a one-line
  follow-up.
- A self-signed or test certificate.
