# Release runbook

<!-- Read by /ship. Keep it under 30 lines: commands and checks, no prose. -->

## Version bump
- Files: `Cargo.toml` `[workspace.package] version` and `Cargo.lock` (`cargo update -w`) — set to `<version>`; README.md asset and folder names `Wisp-<old>-*` → `Wisp-<version>-*`; release notes in `docs/plan/release-notes-<version>.md` (scribe, agreed with the user).
- Commit: `release: bump version to <version>` on a `release/<version>` branch; PR to `main`.
- Wait: CI `ci.yml` green on the PR, then merge (user's yes).

## Tag
- `git tag -a v<version> -m "<version>"` on the merge commit, then `git push origin v<version>`.
- `release.yml` checks the tag matches `Cargo.toml` and creates a **draft** release `v<version>` with three `Wisp-<version>-*` archives and `SHA256SUMS`.

## Publish targets
| Target | Triggered by | Workflow | Secret it needs |
|---|---|---|---|
| GitHub release (draft) | tag push `v*` | `release.yml` | macOS: Developer ID + notary secrets; Windows: SignPath (optional, else unsigned) |
| GitHub release (public) | user publishes the draft: `gh release edit v<version> --draft=false --notes-file docs/plan/release-notes-<version>.md` | — | none |
| winget manifest | release published | `winget.yml` | none (opens no PR; the winget-pkgs PR is the user's) |

## Verify the published state
- `gh release view v<version> --json isDraft,assets` → `isDraft: false`, three `Wisp-<version>-*` assets plus `SHA256SUMS`.
- Download the macOS zip: `shasum -a 256 -c --ignore-missing SHA256SUMS`, `spctl -a -vv Wisp.app` → Notarized Developer ID.
- `winget.yml` run for the release is green and its manifest artifact names `<version>`.

## If a step fails
- Do not delete or move a pushed tag. Fix forward with `<version+patch>`.
- Delete any `dryrun-*` draft releases after the real release is public.
