# 87 — Linux window icon, app id and a .desktop launcher in the release tar

**Repo:** .
**Depends on:** 83 (both edit crates/app/src/main.rs)
**Owns:**
- crates/app/src/main.rs
- crates/app/Cargo.toml
- Cargo.toml (one `[workspace.dependencies]` line for `image` only)
- Cargo.lock
- THIRD-PARTY-NOTICES (regenerated only)
- scripts/release/package.sh (the `linux-x64` arm only)
- assets/linux/io.github.aindriub.wisp.desktop (new)
- docs/plan/tasks/87-linux-desktop.md (the `## Notes` section only)

Branch `task/87-linux-desktop`, worktree `.worktrees/wisp/87-linux-desktop`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-87`. Start only after
83 is merged, then `git fetch origin && git rebase origin/main`. Commit on the
branch; do not push.

## Goal
On Linux, Wisp's window has no icon, and the tar ships PNGs but no launcher
(PLAN.md, **Residuals (76)**: "Linux window icon and `.desktop` file not done").
GPUI 0.3.10 sets a window icon only on X11. Wayland compositors take the icon
from the `.desktop` entry whose name matches the window's app id. So Wisp sets
both: the X11 icon, and an app id that the shipped `.desktop` file matches.

## Context
- crates/app/src/main.rs:37-45 — `window_options()` and its test at the end of the file.
- gpui-pre-0.3.10/src/platform.rs:2631-2644 — `WindowOptions.app_id` and `icon: Option<Arc<image::RgbaImage>>` ("X11 only"). gpui-pre-linux-0.3.10/src/linux/x11/window.rs:803 sets `_NET_WM_ICON`, and :1597 sets WM_CLASS from the app id. wayland/window.rs:595 sets the xdg app id. gpui depends on `image` 0.25 with `png` already enabled, so adding it to wisp-app should not add a new package to the lock.
- scripts/release/package.sh:75-81 — the `linux-x64` arm, which today copies `icons/wisp-{48..512}.png`.
- assets/icon/png/ — the source PNGs (wisp-256.png is 32,967 B).
- scripts/release/Info.plist.template:9-10 — the macOS bundle id `io.github.aindriub.wisp`. The Linux app id reuses it.
- docs/conventions.md#dependencies — `default-features = false`; regenerate notices after any `Cargo.lock` change.

## Contract
- On Linux, `window_options()` sets `app_id = Some("io.github.aindriub.wisp")` and `icon` = the 256 px PNG, embedded with `include_bytes!` and decoded once at startup. If decoding fails, it logs one stderr line and sets no icon; it never panics. On macOS and Windows both fields stay `None`.
- `assets/linux/io.github.aindriub.wisp.desktop` holds exactly these keys: `[Desktop Entry]`, `Type=Application`, `Name=Wisp`, `Comment=Lightweight HTTP client`, `Exec=wisp %f`, `Icon=io.github.aindriub.wisp`, `Terminal=false`, `Categories=Development;Network;`, `StartupWMClass=io.github.aindriub.wisp`.
- The linux tar keeps its current `icons/` directory. It adds `share/applications/io.github.aindriub.wisp.desktop` and `share/icons/hicolor/<N>x<N>/apps/io.github.aindriub.wisp.png` for N in 48 64 128 256 512, so that copying `share/` into `~/.local/share` installs the launcher.

## Acceptance
- [ ] AC1 `image` is added as a workspace dependency (`version = "0.25"`, `default-features = false`, `features = ["png"]`) and used by wisp-app. `cargo tree -p wisp-app -e normal -i image` shows a single `image` version. `scripts/notices/generate.sh --check` exits 0 (regenerate and commit first if it does not).
- [ ] AC2 A test `window_icon_decodes_to_a_256_square` decodes the embedded PNG and asserts it is 256×256. It runs on every platform.
- [ ] AC3 A test `linux_window_options_carry_app_id_and_icon`, under `#[cfg(target_os = "linux")]`, asserts that `app_id` and `icon` are set. A test `non_linux_window_options_have_no_icon`, under `#[cfg(not(target_os = "linux"))]`, asserts that both are `None`. `window_has_a_720_by_480_minimum` still passes.
- [ ] AC4 The `.desktop` file has exactly the keys in the Contract.
- [ ] AC5 `scripts/release/package.sh linux-x64 <any existing file> <outdir>` writes a tar that lists the `share/…` paths in the Contract and the unchanged `icons/` files.
- [ ] AC6 macOS binary size: a release build of wisp-app grows by no more than 4,096 B over the 83 merge commit on the same machine. Notes records both sizes.
- [ ] AC7 `## Notes` states that the window icon is X11-only in GPUI 0.3.10, and that on Wayland the icon appears only once the `.desktop` file is installed. It gives the user's Linux hand-check steps: X11 window icon; after `cp -r share/* ~/.local/share/`, the launcher icon on Wayland.
- [ ] AC8 `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean. The PR's Linux CI job (build, tests and clippy on Linux) is the first compile of the Linux-only code; the record step waits for it.

## Checks
- `icon-unit` command — AC2, AC3. cargo test does not write JUnit XML here, so the command asserts the pass count itself (≥ 3 on macOS: decode, non-linux, minimum size).
  run: `cargo test -p wisp-app -- window_icon_decodes non_linux_window_options window_has_a_720 2>&1 | grep -E '^test result: ok\. ([3-9]|[1-9][0-9]+) passed'`
- `notices` command — AC1
  run: `scripts/notices/generate.sh --check && cargo tree -p wisp-app -e normal -i image`
- `tar-layout` command — AC5
  run: `scripts/release/package.sh linux-x64 Cargo.toml "$TMPDIR/wisp87" && tar tzf "$TMPDIR"/wisp87/Wisp-*-linux-x64.tar.gz | grep -c 'share/icons/hicolor/.*/apps/io.github.aindriub.wisp.png' | grep -qx 5 && tar tzf "$TMPDIR"/wisp87/Wisp-*-linux-x64.tar.gz | grep -q 'share/applications/io.github.aindriub.wisp.desktop$'`
- `lint` command — AC8
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `desktop-and-size` read — AC4, AC6, AC7
  how: the `.desktop` file matches the Contract key for key, and Notes has the two binary sizes, the X11-only statement and the hand-check steps.

## Out of scope
- An install script, AppImage, Flatpak or `.deb`.
- Changing the existing `icons/` layout, or the macOS and Windows packaging arms.
- The README's Linux install instructions (the scribe adds them at record).
- Quit, menu and prompt code in main.rs (task 83).

## Notes
