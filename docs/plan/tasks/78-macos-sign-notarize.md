# 78 — Sign the macOS release with Developer ID and notarise it

**Repo:** `.`
**Depends on:** 76
**Owns:**
- .github/workflows/release.yml (the `build` job's macOS-only steps, `if: runner.os == 'macOS'`, and the `env:` they need; nothing in the Windows/Linux steps or the `release` job)
- scripts/release/package.sh (the `macos-arm64)` case only, plus any argument or env handling it needs)
- scripts/release/sign-macos.sh (new)
- scripts/release/macos-keychain.sh (new, optional; the keychain setup and cleanup may live in sign-macos.sh instead)
- scripts/release/wisp.entitlements (new, only if a launch test proves the hardened runtime needs it)
- docs/plan/tasks/78-macos-sign-notarize.md (the `## Notes` section only)

CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-78

## Goal
The macOS release `Wisp.app` for 0.1.0 is signed with the user's Developer ID
Application certificate (team `UB89GDN343`), uses the hardened runtime, is
notarised by Apple and has the ticket stapled. A user who downloads it gets
no Gatekeeper block. Without the signing secrets (forks, or before the user
adds them) the job keeps today's ad-hoc signing and says so. A `v*` tag push
without them fails.

**Scope change (user decision, 2026-11-03), for the scribe:** this replaces
the spec's "code signing/notarization: Not in v1"
(`docs/plan/specs/2026-10-29-http-client-v1.md:206`) and task 72's ad-hoc
signing as the release path. Ad-hoc signing stays only as the fallback.

## Context
- **Start by merging task 76.** Run `git merge origin/task/76-app-icon`, or merge `main` if 76 has landed. Task 76 rewrites the `macos-arm64)` case of `scripts/release/package.sh`: `wisp.icns` and `Assets.car` go into `Contents/Resources`, and `Info.plist` gains icon keys. Sign the bundle in the form 76 builds it. Do not reorder or drop 76's steps.
- scripts/release/package.sh:32-49 (on main) — the macOS case. `codesign --force --sign - "$dir/Wisp.app"` is at :46 and the `ditto` zip of the `$name` folder (LICENSE, NOTICE, THIRD-PARTY-NOTICES and Wisp.app) is at :47-48. The archive's name and layout must not change, because the `release` job's checks (release.yml:96-102) depend on them.
- .github/workflows/release.yml:20-77 — the `build` matrix job. The existing macOS steps use `if: runner.os == 'macOS'` (:50-55). The Package step is at :68-70. Triggers: `push: tags: ['v*']` and `workflow_dispatch` with `draft_release` (:3-11).
- task 77 owns `crates/app/src/main.rs` and `.cargo/config.toml` only, so it does not overlap with this task.
- `docs/conventions.md` — commit format `78: <imperative summary>`.

### Required design
1. **Mode selection** (a workflow step before Package, macOS only). Map every secret to `env:`, because `secrets.*` cannot be tested in a step's `if:`, and test for non-empty values in shell. The mode is:
   - all five secrets present → `developer-id`;
   - none present → on `push` of a `v*` tag, **fail**, naming the missing secrets (names only). On `workflow_dispatch`, set `adhoc` and print `::notice::` saying the release is ad-hoc signed and not notarised because the Developer ID secrets are absent;
   - some present → fail on every trigger and list the missing names (names only).
   Pass the mode to package.sh as an env value such as `WISP_MACOS_SIGN=adhoc|developer-id`. With the env value unset (a local run), package.sh keeps ad-hoc signing.
2. **Keychain** (developer-id only). Decode the .p12 into `$RUNNER_TEMP`. Create a temporary keychain with a random password generated in the step (`::add-mask::` it). Then unlock it, `set-keychain-settings -lut 21600`, `security import … -T /usr/bin/codesign`, `security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k …`, and add it to the user search list. Delete the decoded .p12 as soon as it has been imported. Select the signing identity by **SHA-1 hash** from `security find-identity -v -p codesigning <keychain>`, keeping only lines that contain `Developer ID Application` and `(UB89GDN343)`. Fail if exactly one does not match. Never hard-code the certificate's common name.
3. **Signing, inside out, no `--deep`.** First `codesign --force --options runtime --timestamp --sign <hash> [--entitlements …] Contents/MacOS/wisp`, then the same for `Wisp.app`. Use `--keychain <temp keychain>` if needed. Start with no entitlements file. Add `scripts/release/wisp.entitlements` only if the signed app fails to launch, or fails to render or connect, without it. Record in Notes which key was needed and the evidence for it.
4. **Notarise.** `ditto -c -k --keepParent Wisp.app <tmp>/Wisp-notarize.zip`. Then `xcrun notarytool submit <zip> --key <p8 file> --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID" --wait --output-format json`, and take the submission id and status from the JSON. If the status is not `Accepted`, run `xcrun notarytool log <id> …` to print Apple's log, then exit non-zero. On `Accepted`, run `xcrun stapler staple Wisp.app` and `xcrun stapler validate Wisp.app`.
5. **Verify before packaging.** Run `codesign --verify --strict --deep --verbose=2 Wisp.app` and `spctl --assess --type execute -vv Wisp.app`. Developer-id mode requires `accepted` and `source=Notarized Developer ID`. In adhoc mode run only the codesign verify, because spctl is expected to reject. Then build the release zip from the **stapled** app with the existing `ditto … --keepParent "$name"` line.
6. **Cleanup.** A final macOS step with `if: always()` deletes the temporary keychain, restores the default search list, and removes the decoded `.p12`/`.p8` files and the notarisation zip. It must succeed when nothing was created (adhoc mode, or an early failure).
7. **No leaks.** No `set -x` or `bash -x` anywhere near secret handling. Write the .p8 and .p12 with `umask 077` into `$RUNNER_TEMP`. Never `echo` or `cat` a secret or a decoded file. Mask the keychain password and anything else derived from a secret. Secret values reach `security`/`notarytool` only through env or files, never as literals in the YAML.

## Acceptance
- [ ] `shellcheck scripts/release/*.sh` exits 0, with no new `# shellcheck disable` other than one for a documented false positive.
- [ ] **Before secrets exist:** `gh workflow run release.yml --ref task/78-macos-sign-notarize` (no draft) succeeds. The macOS job log shows the ad-hoc `::notice::`, and the run's artefacts include all three archives and SHA256SUMS. Record the run id in Notes.
- [ ] Reading the diff shows: a tag push without secrets reaches the fail branch, a partial set fails on every trigger, and the cleanup step has `if: always()` and tolerates nothing having been created.
- [ ] **After the user has added the secrets** (stop and report that you need them; do not ask for the values, create placeholders, or set them yourself), `gh workflow run release.yml --ref task/78-macos-sign-notarize -f draft_release=true` succeeds. Download the macOS artefact on this Mac into a new empty directory and extract it with `ditto -x -k`. On the extracted `Wisp.app`: `codesign --verify --strict --deep --verbose=2` passes; `spctl --assess --type execute -vv` prints `accepted` and `source=Notarized Developer ID`; `xcrun stapler validate` passes; `codesign -dvv` shows `TeamIdentifier=UB89GDN343` and `flags=0x10000(runtime)`. Delete the `dryrun-<run_id>` draft afterwards with `gh release delete dryrun-<run_id> --yes --cleanup-tag`. Record the run id and command output (no secret material) in Notes.
- [ ] The signed, downloaded `Wisp.app` launches on this Mac. Either System Events reports a `Wisp` process with at least one window, or a `WISP_AUTOSEND` run completes a request. `Contents/Resources/Assets.car` and `wisp.icns` are present, and `xcrun assetutil --info …/Assets.car` lists `AppIcon`.
- [ ] No secret value appears in the diff, the commits, the task file or any run log. `git log -p` on the branch contains no base64 blob or PEM header, and the run logs show `***` wherever a masked value would appear.
- [ ] Commits use the `78: <summary>` format. Nothing is pushed as a tag, no release is published, and no PR is opened. The branch may be pushed so that the dispatch runs work.

## User setup
The user does this once. Never generate, request, print or store the values.

1. In the Apple Developer portal (team `UB89GDN343`), create a **Developer ID Application** certificate if you do not already have one. In Keychain Access, export it **with its private key** as a `.p12` file and protect it with a password.
2. In App Store Connect → Users and Access → Integrations → App Store Connect API, create a **Team key** with the *Developer* role. Download the `.p8` file (you can download it only once). Note its **Key ID** and the **Issuer ID** shown on that page.
3. Add the five repository secrets (run from the repo; the paths are placeholders):
   ```sh
   base64 -i /path/to/DeveloperID.p12 | gh secret set APPLE_DEVELOPER_ID_P12
   gh secret set APPLE_DEVELOPER_ID_P12_PASSWORD        # paste the .p12 password at the prompt
   gh secret set APPLE_API_KEY_ID --body '<KEY_ID>'
   gh secret set APPLE_API_ISSUER_ID --body '<ISSUER_UUID>'
   base64 -i /path/to/AuthKey_<KEY_ID>.p8 | gh secret set APPLE_API_KEY_P8
   ```
4. Delete any temporary copies of the `.p12` and `.p8` files you no longer need, and confirm that `gh secret list` shows the five names.

`APPLE_TEAM_ID` is not a secret. The workflow sets it as the plain env value `UB89GDN343`.

## Out of scope
- README macOS install section, for the scribe at /record. For signed releases, drop the Gatekeeper "Open Anyway" and `xattr -dr` steps. Keep a short note that builds from source, or ad-hoc fallback builds, still need them.
- `docs/architecture.md` Release section (around :263), for the scribe. Replace the ad-hoc sentence with a description of Developer ID signing, the hardened runtime, notarisation and stapling, the five secret names, and the fallback rule (tag push fails, dispatch falls back).
- Spec, for the scribe. Add a dated note (2026-11-03) at `docs/plan/specs/2026-10-29-http-client-v1.md:206` saying signing and notarisation moved into v1 by user decision.
- PLAN.md residual (73), for the scribe. Signing makes it more urgent: the `portable` marker inside `Wisp.app/Contents/MacOS` (`settings.rs:70-75`) would now break a notarised signature. Do not change `settings.rs` here.
- Windows signing (Authenticode) and any change to the Windows or Linux steps. Task 77 owns the Windows binary changes.
- The icon assets and 76's actool/icns steps, except merging them in.
- The `release` job, `measure.yml` and `ci.yml`.
- Creating Apple certificates or keys, or setting secrets: the user does that (see User setup).

## Notes
- Ad-hoc fallback dispatch (no secrets): run 41715118789 succeeded; macOS log shows the ad-hoc `::notice::`; artefacts pkg-macos-arm64, pkg-windows-x64, pkg-linux-x64 and wisp-0.1.0-release (with SHA256SUMS).
- Signed dry run (`-f draft_release=true`): run 41718679603 succeeded. Notarisation submission e2cb1752-fef5-444a-9f6b-78fcecb81275 status Accepted; stapled. Downloaded app: `codesign --verify --strict --deep` valid; `spctl` accepted, `source=Notarized Developer ID`, `origin=Developer ID Application: Andrew Bannister (UB89GDN343)`; `stapler validate` worked; `TeamIdentifier=UB89GDN343`, `flags=0x10000(runtime)`. Draft dryrun-41718679603 deleted. (An earlier draft, run 41716718217, was ad-hoc; deleted.)
- Launch: System Events reported a `wisp` process with 1 window. No entitlements file was needed. Resources contain Assets.car (lists AppIcon) and wisp.icns.
- The notarisation step took ~19 minutes of the macOS job in this run.
- Review round 1 signed dry run: run 41726270801 succeeded (all jobs). Downloaded macOS app: `codesign --verify --strict --deep` valid; `spctl` accepted, `source=Notarized Developer ID`; `stapler validate` worked. Draft dryrun-41726270801 deleted. (Two earlier round-1 runs were cancelled after a bad intermediate push; no artefacts.)

## Review round 1 — findings

Reviewed HEAD: 422baaa.

- [P3] .github/workflows/release.yml:105-108 — APPLE_API_KEY_ID / APPLE_API_ISSUER_ID / APPLE_API_KEY_P8 are set on the shared Package step, so the Windows and Linux runners get them too. Gate each to macOS, e.g. `${{ runner.os == 'macOS' && secrets.APPLE_API_KEY_P8 || '' }}`.
- [P3] scripts/release/package.sh:59 — `xcrun assetutil --info` now runs bare under `set -e`; if it fails, actool.log and the "AppIcon missing" message are no longer printed. Keep writing to a file, but handle assetutil's failure the same way as a missing AppIcon (print actool.log and the message, then exit 1).
- [P3] scripts/release/sign-macos.sh:44 — `notarytool submit --wait` has no `--timeout`; add one (e.g. 90m) so a stuck Apple queue fails the job with a clear message instead of running to the 6 h job limit; on timeout print the submission id and how to check it later (`notarytool info`/`log`).
- [NOTE, optional] scripts/release/macos-keychain.sh:67 — cleanup removes `$tmp/Wisp-notarize.zip` but the zip lives at `$tmp/wisp-notary/Wisp-notarize.zip`; drop the dead path.
