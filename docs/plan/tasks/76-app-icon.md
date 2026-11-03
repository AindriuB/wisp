# 76 — Ship the Wisp app icon on macOS, Windows and Linux

**Repo:** .
**Depends on:** none (based on main 4bf7098; task 75 is in open PR #15)
**Owns:**
- assets/icon/** (the two source SVGs plus every generated file)
- scripts/icon/**
- scripts/release/package.sh
- scripts/release/Info.plist.template
- crates/app/build.rs (new)
- crates/app/Cargo.toml — only a `[build-dependencies]` section and a `build` key, nothing else
- Cargo.lock — only the entries the new build-dependency adds
- THIRD-PARTY-NOTICES — only if `scripts/notices/generate.sh` regenerates it with a diff
- this task file's `## Notes` section

## Goal
Give Wisp its own icon on every platform in the 0.1.0 release. The user
approved (2026-11-03) the orchestrator's cleaned-up version of the user's own
logo. The source of truth is two SVGs in `assets/icon/`:
`wisp.svg` is the full icon. It has a blur glow, sparkles (four 4-point stars
and eight small dots) and the artwork inset to the macOS 824/1024 margin. `wisp-small.svg` has no blur, no sparkles, a thicker stroke, a white
core and a full-bleed rounded square. It is for small sizes. Use both
as they stand: do not redraw them, change their margins or make a third
variant. They are the user's own assets under the project's Apache-2.0 licence.
They need no NOTICE or THIRD-PARTY-NOTICES entry. A one-line provenance comment
in the generator script is enough.

## Decisions already made
- **Which SVG at which size.** The cut is on pixel size. Every raster of
  48 px or less comes from `wisp-small.svg`: 16, 24, 32 and 48, including the
  .icns `icon_16x16@2x` (32 px). Every raster of 64 px or more comes from
  `wisp.svg`: 64, 128, 256, 512 and 1024, including `icon_32x32@2x` (64 px).
- **Padding on Windows and Linux is accepted.** At 64 px and up, Windows and
  Linux use the same `wisp.svg` render as macOS, transparent margin included.
  There is no third full-bleed render. This is the simpler choice. The margin is
  about 10% per side. Windows and Linux icon themes also leave some padding, so
  it looks smaller there, not wrong.
- **Generated files are committed.** The generator only regenerates them. It
  runs on a developer Mac, because `iconutil` exists only on macOS. CI and the
  release workflow use the committed files and need no rasteriser.
  **CI does not check staleness**: this task edits no workflow file, so it
  cannot collide with PR #15. The generator has a `--check` mode for local use,
  and wiring that into CI is a successor task.
- **Rasteriser: the `resvg` CLI, version pinned in the script.** Install it with
  `cargo install resvg --version <pinned> --locked`. It renders feGaussianBlur,
  feMerge and userSpaceOnUse filter regions, and it builds on every OS.
  Nothing on this Mac renders SVG today: there is no rsvg-convert, resvg,
  inkscape or magick, and sips/qlmanage do not render filters reliably. Only
  if resvg proves unable to render `wisp.svg` correctly may the implementer use
  rsvg-convert instead. Record the reason in Notes.
- **The .ico is built from PNG-compressed entries** (16, 24, 32, 48, 64, 128,
  256). Use a small python3 stdlib-only helper in `scripts/icon/`, so there is
  no ImageMagick dependency.
- **Linux gets PNGs only.** The tar.gz carries `icons/wisp-{48,64,128,256,512}.png`.
  A `.desktop` file is out, because it only means something with install paths,
  and installers are out of scope.
- **Windows window icon is free.** `gpui-pre-windows-0.3.10/src/platform.rs:1638-1650`
  (`load_icon`) loads icon resource **ID 1** from the exe for the window
  class. Embedding the .ico as resource ID 1 therefore sets the exe icon and
  the titlebar/taskbar icon together. That is the default for `winresource`'s
  `set_icon`. Confirm the ID.
- **The Linux runtime window icon is out.** `WindowOptions.icon` (gpui-pre
  `src/platform.rs:2702-2704`) is X11-only and needs a decoded `RgbaImage`.
  That means a PNG decoder dependency and an edit to `crates/app/src/main.rs`,
  which this task does not own.

## Context
- `scripts/release/package.sh:31-61` — per-platform staging. The macOS branch
  builds `Wisp.app/Contents` and fills in `Info.plist` from the template. The
  Windows and Linux branches copy the binary into `$dir`.
- `scripts/release/Info.plist.template` — add `CFBundleIconFile` (`wisp`, the
  .icns placed at `Contents/Resources/wisp.icns`).
- `crates/app/Cargo.toml:1-20` — has no build script yet.
- `about.toml:29` — `ignore-build-dependencies = true`, so a build-only crate
  should leave THIRD-PARTY-NOTICES unchanged. Confirm it.
- `docs/conventions.md#dependencies` — size justification, `default-features = false`,
  regenerate notices after any Cargo.lock change, permissive licence only.
- `.github/workflows/release.yml:29-66` — matrix and `package.sh` invocation.
  Do not edit it (task 75 / PR #15 owns that file).
- CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-76

## Acceptance
- [ ] Generated files exist and are committed under `assets/icon/`:
      `wisp.icns`, `wisp.ico` and `png/wisp-{16,24,32,48,64,128,256,512,1024}.png`.
      Each PNG's pixel size matches its name (`sips -g pixelWidth -g pixelHeight`).
      `iconutil -c iconset assets/icon/wisp.icns` round-trips and yields all 10
      iconset entries, 16 to 512@2x.
- [ ] `scripts/icon/generate.sh` regenerates every file above byte-for-byte
      (`git status --porcelain assets/icon` is empty after a run). The script
      maps sizes up to 48 px to `wisp-small.svg` and sizes from 64 px to `wisp.svg`,
      and `generate.sh --check` exits 0 on a clean tree and non-zero after
      deleting one PNG. A Notes line records the rendered 512 px PNG, which the
      implementer has inspected visually, with the blur glow and the sparkles present.
- [ ] `crates/app/build.rs` embeds `assets/icon/wisp.ico` as icon resource ID 1
      only when `CARGO_CFG_TARGET_OS == "windows"`, and emits
      `cargo:rerun-if-changed` for the .ico and build.rs.
      `cargo build -p wisp-app --release` on this Mac succeeds. The macOS
      `wisp` binary size before and after is recorded in Notes; the expected
      delta is 0. `scripts/notices/generate.sh` is run, and Notes says whether
      THIRD-PARTY-NOTICES changed (if it did, the file is committed).
- [ ] Locally, `scripts/release/package.sh macos-arm64 <bin> <out>` yields a
      zip whose `Wisp.app/Contents/Info.plist` has `CFBundleIconFile`=`wisp`
      (`plutil -extract CFBundleIconFile raw`) and contains
      `Contents/Resources/wisp.icns`. `codesign --verify` passes, and
      `osascript -l JavaScript -e` with an NSWorkspace `iconForFile` query (or an
      equivalent) shows a non-generic icon. The user's own visual check is part
      of the release hand test, not this task.
- [ ] Release dry run on the branch:
      `gh workflow run release.yml --ref task/76-app-icon -f draft_release=true`
      is green on all three platforms. Notes gives the run id, the asset sizes
      against the last main run, and evidence from the downloaded artifacts:
      the macOS .app has the plist key and the .icns; `wisp.exe` has an
      RT_GROUP_ICON resource ID 1 (listed with e.g. `llvm-readobj --coff-resources`
      or a Python `pefile` listing run locally); the Linux tar lists `icons/wisp-*.png`.
      The draft release is then deleted (`gh release delete`), and no tag is
      pushed or left behind. The branch `task/76-app-icon` is pushed only so
      the dispatch can run. The implementer opens no PR and merges nothing.

## Out of scope
- Any edit to `.github/workflows/*` (PR #15 owns them). CI staleness checking
  of generated icons is a successor task.
- Redrawing or re-margining either SVG, and any full-bleed large-size variant.
- The Linux runtime window icon (`WindowOptions.icon`), any `crates/app/src/**`
  edit, `.desktop` files, installers, DMG/MSI, and notarisation.
- The in-app SVG icon set (`crates/app/assets/icons/`, task 37) and the
  "19 SVG icons" header in `scripts/notices/generate.sh` (a residual of task 70).

## Doc changes for the scribe at /record
- `docs/architecture.md` Release section: the icon sources (`assets/icon/wisp.svg`,
  `wisp-small.svg`, and the size cut at 48/64 px), the committed generated files,
  how to regenerate (`scripts/icon/generate.sh`, pinned resvg, macOS only),
  where each platform's archive carries the icon, and the fact that Windows
  resource ID 1 also feeds the GPUI window class icon.

## Notes

## Amendment — user decision (2026-11-03)
macOS also ships a tinted (monochrome) appearance. Source: assets/icon/wisp-mono.svg. A classic .icns cannot carry appearance variants, so the macOS bundle additionally gets an asset catalog compiled with `actool` (Xcode; present on the dev Mac and on macos-latest runners) into Contents/Resources/Assets.car: an AppIcon with the default artwork plus a tinted appearance (and dark, if the tool requires one, reusing the default). Info.plist sets CFBundleIconName=AppIcon and keeps CFBundleIconFile=wisp with wisp.icns as the fallback for older macOS. Whether the catalog source (.xcassets or an Icon Composer `.icon`) is committed and compiled at package time, or Assets.car is committed, is the implementer's call — record which and why in Notes. Owns gains assets/icon/** (already owned) and the macOS packaging steps in scripts/release/package.sh (already owned); no workflow edit unless actool cannot run from package.sh on the runner. Acceptance adds: `assetutil --info Contents/Resources/Assets.car` (or equivalent) lists the AppIcon with a tinted appearance in the dry-run .app, and the local .app still passes codesign. The user's visual check of tinted mode is part of the release hand test.

## Amendment — user decision (2026-11-03)
Replaces the first amendment's "explicit tinted asset" approach (actool cannot take wisp-mono.svg; classic appiconset drops tinted variants on this Xcode). User chose the layered Icon Composer route.
- The macOS bundle gets an Icon Composer `.icon` (committed under assets/icon/, compiled by actool at package time into Assets.car with CFBundleIconName=AppIcon), plus wisp.icns as the CFBundleIconFile fallback for older macOS.
- Layers: a background fill matching wisp.svg's dark gradient (the .icon's own fill/gradient if supported, else a full-bleed resvg-rendered PNG), and a foreground layer with the W and sparkles on transparent, full-bleed (no 824/1024 inset, no rounded square; the system masks it). Derived from wisp.svg with the background rect and inset transform removed. This is a permitted new variant: assets/icon/wisp-layer.svg (or PNGs rendered from it by the generator). The system derives dark/tinted/clear appearances.
- wisp-mono.svg stays committed as reference but unused; note in Notes.
- The .icns, Windows .ico and Linux PNGs are unchanged from the original plan.
- Acceptance: assetutil shows AppIcon with the tintable/appearance entries in the dry-run .app; the local .app passes codesign; a rendered preview of the layered icon is saved to the scratchpad for the orchestrator. The user's visual check of default/dark/tinted is part of the release hand test.
