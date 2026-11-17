# Microsoft Store submission checklist

The release workflow builds an unsigned MSIX as the workflow artifact
`msix-windows-x64` (not attached to the GitHub release, not in `SHA256SUMS`).
This list is for the repository owner. Agents create no accounts and enter no
credentials.

## Identity variables

Set these as repository variables (Settings, Secrets and variables, Actions,
Variables). Take the values from Partner Center, Product management, Product
identity. A run with any of them unset still produces a package, but it carries
a placeholder and the log shows
`::notice::MSIX uses placeholder <field>; not Store-uploadable`.

| Variable | Manifest field | Partner Center field | Placeholder when unset |
|---|---|---|---|
| `MSIX_IDENTITY_NAME` | `Identity/@Name` | Package/Identity/Name | `Wisp.Placeholder` |
| `MSIX_PUBLISHER` | `Identity/@Publisher` | Package/Identity/Publisher (`CN=...`) | `CN=WispPlaceholder` |
| `MSIX_PUBLISHER_DISPLAY_NAME` | `PublisherDisplayName` | Package/Properties/PublisherDisplayName | `Wisp placeholder publisher` |

## Your steps

- [ ] Create a Partner Center developer account (individual or company; a
      one-time registration fee applies).
- [ ] Reserve the app name in Partner Center.
- [ ] Set the three variables above from the Product identity page.
- [ ] Run the Release workflow (tag push, or dispatch) and confirm the log has
      no `MSIX uses placeholder` notices.
- [ ] Download the `msix-windows-x64` artifact.
- [ ] Optional: run the Windows App Certification Kit (WACK) locally on the
      MSIX before upload. Unsigned packages need to be installed or signed with
      a test certificate for some WACK tests.
- [ ] Upload the `.msix` in a new submission. The Store re-signs the package
      with its own certificate, so the repository signs nothing here.

## What the Store needs that the repo does not hold

- [ ] A privacy policy URL. Wisp makes network requests (it is an HTTP client),
      so the listing must have one.
- [ ] Age rating questionnaire.
- [ ] Screenshots (at least one) and the listing text.
- [ ] The `runFullTrust` justification. The manifest declares the restricted
      capability `runFullTrust` because Wisp is a full-trust desktop app
      (GPUI, direct file and network access). Partner Center asks why; say that
      it is a native desktop application that is not a UWP app.
- [ ] Capabilities declared: `runFullTrust` and `internetClient` only.

## Things to know

- The package version is the `wisp-app` version plus a fourth part `0`
  (`0.1.1` becomes `0.1.1.0`). The Store reserves the last part.
- Minimum OS is Windows 10 1809 (`10.0.17763.0`).
- A Store install runs in a package container. Its `%LOCALAPPDATA%` data is
  kept apart from a zip install's, so collections and settings from the zip
  version are not seen by the Store version, and the reverse.
- `wisp-cli` is not in the MSIX. A packaged console exe needs an app execution
  alias and goes through Store review; the CLI ships in the zip, tarball and
  macOS bundle only. `pack.ps1` packages `wisp.exe` alone.
