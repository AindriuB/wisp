# 79 — Quit properly on every platform and stop the macOS Dock-Quit crash

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/main.rs
- crates/app/src/actions.rs
- crates/app/src/ui/menu.rs (new, optional; plus its one `mod menu;` line in crates/app/src/ui/mod.rs)
- scripts/measure/quit-check.sh (new)
- docs/plan/tasks/79-macos-quit.md (the `## Notes` section only)

Branch `task/79-macos-quit`, worktree `.worktrees/wisp/79-macos-quit`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-79`. Commit on the
branch; do not push and do not open a PR.

## Goal
In the signed 0.1.0 build on a Touch Bar Mac (macOS 27.0.1, MacBookPro17,1),
closing the window left the process running with no window, and Dock → Quit
then crashed with an uncaught Objective-C exception (SIGABRT). Wisp needs a
Quit command (menu and keyboard) on every platform, needs to exit when its
last window closes, and must not crash when it quits.

## Context
- crates/app/src/main.rs:47-81. It opens one window and calls `cx.activate(true)`. It has no `cx.set_menus`, no Quit action and no close or last-window handling.
- crates/app/src/actions.rs:5-23 (`actions!`), :27-52 (`KEYS`, one array per OS), :94-103 (`palette_hint`), :106-119 (`bind_keys`, which looks keys up by index), :153-162 (the test that checks the index-to-name mapping). Add Quit the same way. This is task 74's single-source-of-truth pattern.
- GPUI APIs (gpui-pre 0.3.10, in `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.10/src/`):
  - `App::quit` app.rs:1296
  - `on_app_quit` :2851
  - `on_window_closed` :2886
  - `set_menus` :2923
  - `Window::on_window_should_close` window.rs:6914 (this callback runs inside AppKit's `windowShouldClose:`, before any teardown)
  - `MenuItem::action` / `os_submenu` / `os_action`: platform/app_menu.rs:118-140. `OsAction` is at :311.
- `MacPlatform::quit` (gpui-pre-macos-0.3.10/src/platform.rs:575-593) sends `[NSApp terminate:]` asynchronously on the main queue. `applicationWillTerminate:` (:1380) runs GPUI's quit callbacks, and then AppKit exits the process with no further display cycle. GPUI does not implement `applicationShouldTerminateAfterLastWindowClosed:`.

### Crash cause (planner's analysis, from source; not yet confirmed on hardware)
- Every window is set up through `MacWindow::a11y_init` (gpui-pre-macos window.rs:2340-2357). Window::new calls it unless the app was built `new_inaccessible` (gpui-pre window.rs:1678-1698). It creates `accesskit_macos::SubclassingAdapter::for_window`, and that **isa-swizzles the window's content view** with `object_setClass` (accesskit_macos-0.26.3/src/subclass.rs:203).
- When a window closes, `MacWindow::drop` (window.rs:1356-1390) drops the adapter. `SubclassingAdapter::drop` (subclass.rs:259-263) then calls `object_setClass(view, prev_class)` to put back the class it saved.
- AppKit's Touch Bar finder KVO-observes the objects in the responder chain, and the content view is one of them. KVO also works by isa-swizzling: the class becomes `NSKVONotifying_…`. When the adapter restores the saved class, it overwrites KVO's class. On the next display-cycle flush, `-[_NSTouchBarFinderObservation invalidate]` calls `removeObserver:forKeyPath:` on an object that no longer has KVO's isa. That throws "not registered as an observer", and the uncaught exception calls abort().
- This matches the report. Red-button close → the window and adapter drop while the app keeps running → a later flush (here the one triggered by Dock Quit) throws.
- It also explains why quitting with the window open (AppleScript, SIGTERM) did not crash: the window is torn down inside `applicationWillTerminate:`, and the process exits before another flush runs.
- The real bug is in accesskit_macos and GPUI: restoring the class through `object_setClass` is not KVO-safe. Wisp can only work around it.

### Fix to apply (smallest one, inside our code, no new dependency)
- Never tear down a window while the app keeps running. Register `window.on_window_should_close` on the main window. Its handler calls `cx.quit()` and returns `false`, so the red button, Cmd-W-to-window (if any) and the Windows/Linux close button all go through `cx.quit()` → `terminate:`. The window is then dropped inside `applicationWillTerminate:` and no flush follows.
- Also register `cx.on_window_closed`, which calls `cx.quit()` when `cx.windows()` is empty. It is a backstop for closes that bypass should_close.
- Recommendation: quit on last window close on macOS too. Wisp is a single-window utility with no document model and no way to reopen a window from the Dock (`should_handle_reopen` has no callback wired). A windowless process is useless, and that state is exactly what triggers the crash.
- Do not add `objc2` or `cocoa` to crates/app/Cargo.toml unless the hand test still crashes after the fix above.
  - First fallback: `Application::new_inaccessible`, if `gpui_platform` exposes it. It drops AccessKit on macOS, so record the cost.
  - If you need a Cargo.toml change, stop and report: it is not in Owns.
- `NSApplication.isAutomaticCustomizeTouchBarMenuItemEnabled` or a nil `touchBar` would not help. The finder's KVO on the responder chain happens regardless of them, so they would not stop it.

## Acceptance
- [ ] `actions.rs` has a `Quit` action with `("cmd-q", "quit")` in the macOS `KEYS` and `("ctrl-q", "quit")` in the other `KEYS`. `bind_keys` binds it by index. A global `cx.on_action` for `Quit` calls `cx.quit()`. `bind_keys_indices_match_names` includes `"quit"`. A new test asserts `format_hint(<quit key>, Mac) == "⌘Q"` and `format_hint("ctrl-q", Linux) == "Ctrl+Q"`.
- [ ] On macOS, `cx.set_menus` installs an app menu titled "Wisp". It contains at least "About Wisp" (only if trivial in 0.3.10; otherwise state that in Notes), "Hide Wisp" (only if 0.3.10 has an `OsAction` or selector for it), a separator and "Quit Wisp" bound to `Quit`. The menu shows ⌘Q because it is derived from the keymap.
- [ ] Closing the main window (should_close → `cx.quit()`; `on_window_closed` with no windows left → `cx.quit()`) ends the process on every platform. Add an env hook `WISP_CLOSE_WINDOW_AFTER_MS=<n>` that runs the same close handler after n ms. With `WISP_CLOSE_WINDOW_AFTER_MS=1500 target/wt-79/release/wisp`, the process exits with status 0 within 5 s.
- [ ] `scripts/measure/quit-check.sh <Wisp.app>` (macOS only, `set -euo pipefail`) does two runs. (a) Launch the app, wait for the process, run `osascript -e 'tell application id "io.github.aindriub.wisp" to quit'`, and assert the process is gone within 5 s. (b) Run the binary with the close hook and assert exit 0. In both runs, assert that no new `wisp*.ips` appeared in `~/Library/Logs/DiagnosticReports`. The script passes locally on a bundle from `scripts/release/package.sh macos-arm64`. `cargo test -p wisp-app`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` all pass.
- [ ] `## Notes` records three things. (1) The crash cause above, confirmed or corrected after reading the code. Say whether Zed or accesskit upstream changed `SubclassingAdapter::drop` or the Touch Bar handling, or "not traced" if you cannot tell offline. (2) Why macOS quits on last window close. (3) Manual steps for the user on a Touch Bar Mac, with expected results:
  - launch → red button → process exits, and `pgrep -x wisp` is empty
  - relaunch → Cmd-Q → exits
  - relaunch → Dock right-click → Quit → exits
  - after all three, no new `wisp*.ips` in `~/Library/Logs/DiagnosticReports`

## Out of scope
- Patching or vendoring gpui-pre-macos or accesskit_macos, or filing upstream issues. Record the findings in Notes only.
- Any change to Cargo.toml or Cargo.lock (stop and report if one is needed).
- Windows/Linux crash logging, signing or notarisation (task 78), release.yml, scripts/release/** and other release workflow changes.
- Multi-window support, a Dock menu, reopen-from-Dock, Edit/View/Window menus beyond the app menu.
- Changing the existing keys or the Cmd-W (CloseTab) behaviour.

## Notes

### Crash cause
Not reproduced here (no Touch Bar hardware); the analysis above was read against
the source and stands as written: `SubclassingAdapter::drop` restores the saved
class with `object_setClass`, which clobbers AppKit's KVO isa-swizzle on the
content view. Whether Zed or accesskit upstream changed `SubclassingAdapter::drop`
or the Touch Bar handling: not traced (offline). Workaround only: no window is
ever dropped while the app keeps running.

### Why macOS quits on last window close
Wisp is single-window with no document model and no reopen-from-Dock handler, so
a windowless process is useless, and that state is what exposes the crash. Both
`on_window_should_close` (returns false after `cx.quit()`) and an
`on_window_closed` backstop quit the app, so the window drops inside
`applicationWillTerminate:`.

### Implementation notes
- App menu "Wisp" holds Services, a separator and "Quit Wisp" (Cmd-Q from the keymap). "About Wisp" and "Hide Wisp" are omitted: GPUI 0.3.10 has no `OsAction`/selector for them and About needs a panel.
- `WISP_CLOSE_WINDOW_AFTER_MS` calls the same `quit_app` the close handlers use.
- Accepted limit: that hook calls `quit_app` directly, not through `on_window_should_close`, so check (b) does not prove the real close-button path (a real-close hook would need objc or `remove_window`, the crashing teardown). Manual step 1 on the Touch Bar Mac covers it.
- `scripts/measure/quit-check.sh` reads the bundle id from Info.plist. It passed on a `package.sh macos-arm64` bundle re-id'd to `io.github.aindriub.wisp.test79` and ad-hoc signed (so a real running Wisp is untouched). It only signals pids it started.
- The script cannot prove the Touch Bar crash is gone (needs that hardware); it checks quit paths and absence of new `.ips`.

### Manual steps (Touch Bar Mac)
1. Launch, click the red button: process exits; `pgrep -x wisp` is empty.
2. Relaunch, Cmd-Q: exits.
3. Relaunch, Dock right-click, Quit: exits, no crash dialog.
4. Afterwards no new `wisp*.ips` in `~/Library/Logs/DiagnosticReports`.

## Review round 1 — findings

Reviewed HEAD: 19c8d00.

- [P2] scripts/measure/quit-check.sh:46 — run (a) sends the AppleScript quit to whatever app has the bundle id, so with the user's own Wisp open (same id as a package.sh build) it can quit the user's copy and lose drafts. Before launching, abort with exit 2 and a clear message if `application id "$id" is running` is already true.
- [STYLE] crates/app/src/actions.rs:148 — rename `quit_hints` to a sentence (docs/conventions.md:104), e.g. `quit_hint_reads_cmd_q_on_mac_and_ctrl_q_elsewhere`.
- [P3] scripts/measure/quit-check.sh:48 — run (a) discards the child's exit status; capture it and fail on a non-zero/signal exit, and wait for ReportCrash longer than a fixed `sleep 2` (poll up to ~10 s for a new .ips).
- [Accepted limit, orchestrator decision] main.rs:112-123 — the WISP_CLOSE_WINDOW_AFTER_MS hook calls quit_app directly rather than through on_window_should_close, so check (b) doesn't prove the close button path; a real-close hook would need objc or remove_window (the crashing teardown). Accepted: the user's hand test on the Touch Bar Mac covers the real close button. Record in Notes.
