# 156 — Ship `wisp-cli` in every release archive and raise the GUI gate (3B)

**Repo:** .
**Depends on:** none
**Owns:**
- .github/workflows/release.yml
- .github/workflows/winget.yml (PR path: render and validate the templates)
- scripts/release/package.sh
- scripts/release/sign-macos.sh
- scripts/release/sign-windows/** (artifact-configuration.xml, README.md, verify.ps1 only if it needs a path argument change)
- packaging/winget/templates/installer.yaml.template
- packaging/winget/README.md
- packaging/msix/STORE-SUBMISSION.md
- scripts/measure/cli-budgets.json

## Goal
Every release archive carries `wisp-cli` beside the GUI: in the Windows zip,
the Linux tarball, and inside the signed and stapled macOS bundle. The release
job enforces the CLI budget and, per the user's decision of 2026-11-17, a
Windows GUI gate of 16,000,000 B. Windows CLI signing switches on only once the
owner has updated SignPath; until then the CLI ships unsigned with a notice.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Release packaging" (:286-296), "Binary budget" (:282-284); Amendment 1 "Tag rule"; Amendment 2 (task 153) for the gate.
- release.yml: matrix :20-35 (`binary`), Build :63-64, glibc :65-67, signing mode :68-90, SignPath upload/sign/copy-back/verify :92-123, Windows size check :124-135 (the `15500000` at :130-131), Package :187-193, checksums :220-229 (`sha256sum Wisp-*`, three archives), draft releases :242-251 (`--notes ""`).
- package.sh: arguments `<platform> <bin> <out>`; macOS bundle :34-66 (calls sign-macos.sh :66); Windows zip :72-73; Linux tar :76-88.
- sign-macos.sh: `bin="$app/Contents/MacOS/wisp"` :12; ad-hoc :16-17; Developer ID loop `for target in "$bin" "$app"` :36-41.
- sign-windows/artifact-configuration.xml:4-9 (one `pe-file` for wisp.exe); README.md:3-34.
- measure.yml already builds both packages and checks `cli-budgets.json` (:83-107); it is not changed here.
- CLI sizes at the wave-2 head (`scripts/measure/results/wave-2-size.json`): macOS 1,415,600 B, Windows 1,631,232 B, Linux 1,966,744 B, against 4,194,304 B per target. The CLI roughly tripled since wave 1 (macOS 552,528 B) and nobody has investigated why; this task keeps the 4 MiB limits, does not investigate, and says so in the PR.
- winget.yml: on `pull_request` it validates only the committed 0.1.0 manifests (:54-56), so a template change is not validated today.
- Worktree: `./.claude/scripts/wt-new.sh . 156 package-cli-and-raise-gate`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-156`. Run every command in the foreground; never start a background command; never use `pkill` or `killall`. Do not dispatch `release.yml`; task 158 does the dry run.

## Decision (planner, inside the spec)
Windows CLI signing is gated by a repository variable `SIGNPATH_SIGNS_CLI`. When it is `true` and the SignPath mode is active, the unsigned artifact holds both exes and both are signed and verified. Otherwise the artifact is exactly today's (wisp.exe only), so a SignPath configuration that lists one file keeps working, and the CLI ships unsigned. The owner sets the variable after updating the configuration in SignPath.

## Acceptance
- [ ] AC1 The Build step builds `-p wisp-app -p wisp-cli` in one `cargo build` invocation; the matrix names the CLI binary per target.
- [ ] AC2 On Linux, `check-glibc.sh` runs on `wisp-cli` with the same 2.39 floor.
- [ ] AC3 The Windows GUI size check fails above 16,000,000 B (was 15,500,000) and its error message names 16000000.
- [ ] AC4 A CLI size step on every target reads its limit from `scripts/measure/cli-budgets.json`; it fails when the CLI is over the limit and also when the limit is missing or not a positive integer. `cli-budgets.json` keeps 4,194,304 per target.
- [ ] AC5 `package.sh` takes the CLI as an optional fourth argument: Windows zip has `wisp-cli.exe` beside `wisp.exe`; Linux tar has `wisp-cli` mode 0755 owner 0/0 beside `wisp`; macOS puts it at `Wisp.app/Contents/MacOS/wisp-cli`. Without the argument the archives are as today. The Package step passes it.
- [ ] AC6 `sign-macos.sh` signs the CLI inside out before the main binary and the app (Developer ID: runtime and timestamp; ad-hoc: the same deep verify passes), when `Contents/MacOS/wisp-cli` exists.
- [ ] AC7 Windows signing follows the Decision: with `SIGNPATH_SIGNS_CLI == 'true'` both exes are uploaded, copied back and checked with `verify.ps1`; otherwise only `wisp.exe`. The release job never fails because the CLI is unsigned.
- [ ] AC8 Both draft-release commands pass notes that contain "wisp-cli.exe in this release is not code-signed" exactly when AC7's CLI signing did not run, and no such sentence otherwise.
- [ ] AC9 `artifact-configuration.xml` has a second `pe-file` for `wisp-cli.exe`; the sign-windows README says the owner must apply it in SignPath and then set `SIGNPATH_SIGNS_CLI=true`, and what happens before that.
- [ ] AC10 The winget installer template has a second `NestedInstallerFiles` entry `Wisp-@VERSION@-windows-x64\wisp-cli.exe` with `PortableCommandAlias: wisp-cli`. winget.yml's PR path also renders the templates with `winget-manifest.sh` and a fixture SHA256SUMS and validates the result. If validation rejects two portable files, drop the entry and say so in the winget README and the PR.
- [ ] AC11 STORE-SUBMISSION.md states that `wisp-cli` is not in the MSIX and why (a packaged console exe needs an app execution alias and Store review); `packaging/msix/pack.ps1` is unchanged.
- [ ] AC12 `SHA256SUMS` still covers exactly the three `Wisp-*` archives; no new release asset. No file outside `Owns` changes.

## Checks
- `shell-syntax` command — AC5, AC6
  run: `bash -n scripts/release/package.sh && bash -n scripts/release/sign-macos.sh`
- `package-local` command — AC5 (macOS archive, ad-hoc)
  run: `cargo build --release --locked -p wisp-app -p wisp-cli && scripts/release/package.sh macos-arm64 "$CARGO_TARGET_DIR/release/wisp" "$CARGO_TARGET_DIR/dist" "$CARGO_TARGET_DIR/release/wisp-cli" && unzip -l "$CARGO_TARGET_DIR"/dist/Wisp-*-macos-arm64.zip | grep -E 'Contents/MacOS/wisp-cli$'`
- `budgets-json` command — AC4
  run: `jq -e '.limits | to_entries | length == 3 and all(.value == 4194304)' scripts/measure/cli-budgets.json`
- `scope` command — AC11, AC12
  run: `git diff --exit-code main...HEAD -- packaging/msix/pack.ps1 scripts/measure/budgets.json .github/workflows/measure.yml crates`
- `winget-pr` command — AC10
  run: `gh pr checks <pr> --required=false | grep -E '^manifest\s+pass'` (the winget.yml PR run on this branch)
- `workflow` read — AC1-AC4, AC7, AC8, AC9, AC12
  how: the reviewer reads the release.yml diff: one build invocation, glibc on both binaries, 16000000, the CLI step's integer check, the variable gate on every SignPath step, the notes condition, the checksum glob unchanged.

## Out of scope
- The release dry run, tagging and hand tests (task 158).
- Investigating the CLI's size growth; changing measure.yml (its non-integer-limit Low stays a PLAN follow-up).
- Updating the SignPath configuration itself (owner only) and opening the winget-pkgs PR.
- docs/architecture.md and README (task 157); spec and roadmap (task 153).

## Review round 1 — findings

Reviewed HEAD 141362c66710a60e2113b8e8ce785770bbda8200.

- [P2] .github/workflows/release.yml:165-169 (Verify Windows signature) — a failed `wisp.exe` verification is hidden when the CLI verification that runs after it passes. `verify.ps1` exits with `exit 1`, which only leaves the script and sets `$LASTEXITCODE=1`; the step's pwsh script then runs the second call, whose `signtool verify` resets `$LASTEXITCODE` to 0, and GitHub's pwsh wrapper ends the step with `exit $LASTEXITCODE`, so the step passes with an unverified `wisp.exe`. Input: `SIGNPATH_SIGNS_CLI=true`, SignPath returns a `wisp.exe` that fails verification (no RFC 3161 timestamp, or a status other than Valid) next to a good `wisp-cli.exe`. Fix shape: after the first call add `if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`, or split the check into two steps. (AC7 requires both exes checked with `verify.ps1`.)

## External review — findings

Key sha256:68751d8b14d4 · reviewed head 02d6f4d19ab4708650841d18255005aeeb67dc57 (PR #114, record/156; task code at f562f44) · round 2

## AindriuB/wisp#114 · Changes suggested · head 02d6f4d19ab4
0 P1 · 1 P2 · 1 P3
| ID | Sev | Where | Issue | Failing input | Fix |
|---|---|---|---|---|---|
| 114-1 | P2 | `scripts/release/sign-windows/artifact-configuration.xml:12` | The reference configuration now requires a wisp-cli.exe entry, but the workflow uploads wisp.exe alone whenever SIGNPATH_SIGNS_CLI is not true. In SignPath, each listed pe-file must match exactly one file by default. The README says to apply this configuration first and set the variable afterwards, so between those two steps every signed release fails. | The owner applies the new artifact-configuration.xml in SignPath and has not yet set SIGNPATH_SIGNS_CLI=true. A tag push uploads a zip with only wisp.exe. The SignPath request then fails because wisp-cli.exe is missing, and the Windows build fails. This contradicts the README line "a configuration listing one file keeps working". | Add min-matches="0" to the wisp-cli.exe pe-file, or tell the owner to set the variable and the configuration together. Also fix the README wording. |
| 114-2 | P3 | `.github/workflows/release.yml:165` | The CLI's signing state is uploaded from the Windows job only. The release job always downloads meta-windows-cli-signing and reads it to write notes.md. Nothing in the workflow checks that the file holds "signed" or "unsigned". | A Windows run that writes an empty or unexpected state file produces notes.md with the "not code-signed" line even when the CLI is signed, because every value other than "signed" falls through to that line. | Fail the notes step when the state is neither "signed" nor "unsigned". |
Not assessed: SignPath's real matching behaviour and the winget-pr CI result were not checked against a live run. The release workflow was not dispatched, by rule.

## Wave verified — c611780a4e2ecc3aa7d80c9190dc15215c35b352

## Outcome
Merged via record/156 at f562f44 (stacked on record/154). Every archive now carries `wisp-cli`; the Windows GUI gate is 16,000,000 B; the CLI size step reads `cli-budgets.json`; Windows CLI signing is behind `SIGNPATH_SIGNS_CLI`. Review round 1 fixed a P2 at release.yml:167 (a failed `wisp.exe` verification was hidden by the passing CLI check). Tester: 1,343 passed, 0 failed, no flakes. The `winget-pr` check (AC10) runs only on the PR's CI. The release was not dispatched; task 158 does the dry run.

Review round 2 (PR #114 external review, 114-1) made the `wisp-cli.exe` entry in the SignPath configuration optional (`min-matches="0"`).
