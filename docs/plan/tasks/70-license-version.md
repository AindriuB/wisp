# 70 — License Wisp under Apache-2.0, set version 0.1.0 and generate third-party notices

**Repo:** `.`
**Depends on:** none
**Base:** `main` at `5dc4dd3`
**Build:** `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-70`
**Owns:**
- LICENSE (new)
- NOTICE (new)
- THIRD-PARTY-NOTICES (new, generated)
- about.toml (new; cargo-about config)
- scripts/notices/** (new; the generator script, the cargo-about template, the bundled-assets section)
- Cargo.toml (the `[workspace.package]` table only)
- crates/app/Cargo.toml, crates/core/Cargo.toml, crates/storage/Cargo.toml (the `[package]` table only)
- Cargo.lock (only the three `wisp-*` version lines may change)
- .github/workflows/ci.yml (add one job; existing jobs unchanged)
- docs/plan/tasks/70-license-version.md (the `## Notes` section only)

## Goal
The user chose Apache-2.0 and version 0.1.0 for the first release (decision of
2026-11-03). Every crate inherits both from the workspace. The repo carries
the license text, a NOTICE, and a generated THIRD-PARTY-NOTICES covering every
dependency and bundled asset that ships in a release binary on the three 0.1.0
targets. **Decision (planner):** THIRD-PARTY-NOTICES is committed, and CI
regenerates it on every push and fails if it differs from the committed file.
A committed file can be reviewed when a dependency changes, the release job
(task 72) only has to copy it, and README (task 73) can link to it.

## Context
- Cargo.toml:1-14 — `[workspace.package]` has `edition` and `publish` only. The crates are
  at `version = "0.0.0"` with no `license` (crates/*/Cargo.toml:1-5).
- .github/workflows/ci.yml:14-46 — the `check` job. Actions are pinned by full commit SHA
  with the tag in a trailing comment; follow that for any new action.
- crates/app/assets/icons/NOTICE — the icons copied from gpui-kit-assets 0.8.1
  (Apache-2.0) and the Lucide ISC text. The SVGs are embedded in the binary by
  crates/app/src/assets.rs:10-38, so they are not covered by any crate's license
  entry and must be appended as a separate section.
- gpui-component 0.8.1 also embeds two Lucide SVGs of its own (src/icon.rs:247,
  src/button/button_icon.rs:176). The Lucide ISC section covers them; say so in the
  bundled-assets section.
- Embedded fonts: the `include_bytes!` of IBM Plex Sans and Lilex in gpui-pre 0.3.10
  (src/svg_renderer.rs:361-363) and gpui-pre-wgpu 0.3.10 (src/cosmic_text_system.rs:1233-1260)
  are all inside `#[cfg(test)]`, so no fonts ship. Do not list them.
- License facts from `cargo metadata --locked` at 5dc4dd3: our three crates are the only
  packages with no license. Notable ones: `webpki-root-certs` 1.0.11 is
  `CDLA-Permissive-2.0` (it is the bundled root-certificate data). `dwrote` 0.11.5
  (Windows) and `option-ext` 0.2.0 are `MPL-2.0`. `cbindgen` 0.28.0 is MPL-2.0 but
  build-only. Expressions that need an accepted choice include `Apache-2.0 OR GPL-2.0-only`
  (self_cell), `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (r-efi), `Apache-2.0 OR BSL-1.0`
  (ryu), `Apache-2.0 AND ISC` (ring), `CC0-1.0` (notify, tiny-keccak) and `Unicode-3.0`.
- The three 0.1.0 targets: `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`,
  `x86_64-unknown-linux-gnu`. Intel macOS is dropped (user decision 2026-11-03).

## Acceptance
- [ ] `Cargo.toml` `[workspace.package]` has `version = "0.1.0"` and `license = "Apache-2.0"`.
      Each of the three crate manifests has `version.workspace = true` and
      `license.workspace = true` and no literal `version` or `license`.
      `cargo metadata --no-deps --format-version 1 --locked` reports version `0.1.0` and
      license `Apache-2.0` for `wisp-app`, `wisp-core` and `wisp-storage`.
- [ ] `git diff 5dc4dd3 -- Cargo.lock` changes only the three `version = "0.0.0"` lines of
      the `wisp-*` packages to `"0.1.0"`.
- [ ] `LICENSE` is the Apache License 2.0 text: `curl -s https://www.apache.org/licenses/LICENSE-2.0.txt | diff - LICENSE`
      prints nothing.
- [ ] `NOTICE` starts with the two lines `Wisp` and `Copyright 2026 Andrew Bannister`,
      and says that third-party notices are in `THIRD-PARTY-NOTICES`.
- [ ] `scripts/notices/generate.sh` (bash, `set -euo pipefail`) writes `THIRD-PARTY-NOTICES`
      from `Cargo.lock` with cargo-about at a pinned version named in the script, using
      `about.toml` and a template under `scripts/notices/`. Running it twice in a row
      leaves `git status --porcelain` empty. `about.toml` sets the three targets above,
      ignores build and dev dependencies and our own unpublished crates, and does not
      query clearlydefined (so the output depends only on `Cargo.lock` and the crate
      sources).
- [ ] `THIRD-PARTY-NOTICES` lists, for every package in the shipped dependency graph of
      the three targets, its name, version and license, with each license's full text
      included once. These checks pass:
      `rg -c 'webpki-root-certs' THIRD-PARTY-NOTICES` ≥ 1 and the CDLA-Permissive-2.0
      text is present; `dwrote` and `option-ext` appear under MPL-2.0 with the MPL-2.0 text;
      `rg -n '\bcbindgen\b|\bwiremock\b|\brcgen\b|\btempfile\b' THIRD-PARTY-NOTICES` finds
      nothing; `wisp-app`, `wisp-core` and `wisp-storage` do not appear as dependencies.
- [ ] `THIRD-PARTY-NOTICES` ends with a "Bundled assets" section, produced by the script,
      that reproduces `crates/app/assets/icons/NOTICE` verbatim, names gpui-kit-assets 0.8.1
      (Apache-2.0) as the source of the 19 embedded icons, and notes that gpui-component's
      own two embedded Lucide icons fall under the same ISC text.
- [ ] `.github/workflows/ci.yml` has a new `notices` job on `ubuntu-latest` that installs
      the same pinned cargo-about version, runs `scripts/notices/generate.sh`, and fails
      with `git diff --exit-code THIRD-PARTY-NOTICES`. The `check` job is byte-identical to
      5dc4dd3. Notes records a CI run on this branch in which `notices` succeeded, and the
      output of a local negative check: deleting one line of `THIRD-PARTY-NOTICES` and
      running the same diff command exits 1 (then restore it).
- [ ] `cargo build --locked -p wisp-app` and `cargo test --workspace --locked` pass with
      `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-70`.
- [ ] Every accepted license in `about.toml` is permissive or weak-copyleft (MPL-2.0 only);
      when an expression offers a choice, the generated file names the permissive option.
      If cargo-about rejects any package, Notes lists it with its license instead of the
      implementer widening `accepted` to a copyleft license.

## Out of scope
- Packaging LICENSE, NOTICE and THIRD-PARTY-NOTICES into release archives (task 72).
- README and any change under `docs/` (task 73 and the scribe). For /record the scribe
  should: add to `docs/conventions.md#dependencies` that a `Cargo.lock` change must be
  followed by `scripts/notices/generate.sh` and a commit of `THIRD-PARTY-NOTICES`
  (CI's `notices` job enforces it), and note the license and version in
  `docs/architecture.md`.
- Source-file license headers, SPDX tags in source files, `repository`/`authors`
  manifest fields, cargo-deny.
- Any dependency change, including removing a dependency to avoid a license.

## Notes

- cargo-about 0.9.3 needs `--features cli` to install the binary; the script and CI job use it.
- `about.toml` accepted list is permissive plus MPL-2.0. `0BSD` was added (enum-iterator, enum-iterator-derive); it is permissive. Nothing was rejected otherwise.
- Negative check: after deleting line 5 of `THIRD-PARTY-NOTICES`, `git diff --exit-code THIRD-PARTY-NOTICES` exited 1; restored with `git checkout`, then it exited 0.
- Generating twice leaves `git status --porcelain` empty (outputs compared with cmp).
- DEVIATION, tempfile: the grep for `tempfile` matches line 6696 (`tempfile 3.27.0`). With the three targets configured together, cargo-about includes `uds_windows` (a Windows-only dep of `zbus`, which only Linux reaches) so `tempfile` appears. Run with one `--target` at a time cargo-about matches `cargo tree` exactly and omits `tempfile`, but then `dwrote` is also absent: `dwrote` is not in the normal graph of any of the three targets (cargo tree confirms). So the task's checks "dwrote appears" and "tempfile absent" cannot both hold; the committed file is the 3-target superset (safe for notices; over-lists tempfile, uds_windows, dwrote and ~20 Windows-only/other-OS crates). A per-target union would be exact but needs a JSON merge step; left for the orchestrator to decide.
- CI: run 41684364745 failed `notices` (offline cargo metadata lacked sources; fixed by `cargo fetch --locked` in generate.sh). Run 41684893111 on this branch: `notices` succeeded.

## Amendment — user decision (2026-11-03)

THIRD-PARTY-NOTICES is generated once over all three release targets together (aarch64-apple-darwin, x86_64-pc-windows-msvc, x86_64-unknown-linux-gnu) and may over-list: it can include crates that no single target ships (e.g. `uds_windows`, `tempfile`, `dwrote`). Over-listing is accepted; under-listing is not. The acceptance check that `tempfile` is absent is withdrawn. MPL-2.0 crates that appear (e.g. `dwrote`, `option-ext`) must be listed with their licence text.
