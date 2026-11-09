# 83 — Ask before quitting, closing the window or closing a tab discards unsaved changes; add Hide to the macOS menu

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/main.rs
- crates/app/src/ui/root.rs
- crates/app/src/workspace.rs
- crates/app/src/actions.rs
- docs/plan/tasks/83-confirm-discard.md (the `## Notes` section only)

Branch `task/83-confirm-discard`, worktree `.worktrees/wisp/83-confirm-discard`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-83`. Commit on the
branch; do not push.

## Goal
Cmd-Q/Ctrl-Q, the window close button and closing a dirty tab currently drop
unsaved drafts without asking (PLAN.md, Residuals (55) and Residuals (79)).
Every path Wisp can intercept must ask first, using the same "Discard unsaved
changes?" prompt that opening a collection already uses. The paths GPUI 0.3.10
cannot intercept are listed in Notes so the scribe can document the gap. On
macOS the app menu also gets Hide Wisp (⌘H), Hide Others (⌥⌘H) and Show All
(see Residuals (79)).

## Context
- crates/app/src/ui/root.rs:142-190 — `on_open_collection`: the prompt pattern to reuse (`window.prompt(PromptLevel::Warning, "Discard unsaved changes?", …, &["Discard", "Cancel"])`, answer `Ok(0)` = Discard).
- crates/app/src/ui/root.rs:300-315 (tab × button), :420-426 (`CloseTab` action; ⌘W, the palette's "Close tab" and the banner's close button all dispatch it).
- crates/app/src/workspace.rs:91-93, :210-212 (`open_needs_confirm`), :387 (`close_tab(ix)`).
- crates/app/src/main.rs:47-64 (`quit_app`, `install_menus`), :72 (global `Quit` handler), :98-101 (`on_window_should_close`), :112-123 (`WISP_CLOSE_WINDOW_AFTER_MS` hook).
- crates/app/src/actions.rs:5-23, :27-55, :108-123, :170-182 — how actions, `KEYS` and `bind_keys_indices_match_names` fit together.
- GPUI 0.3.10 (`~/.cargo/registry/src/index.crates.io-*/`):
  - `Window::prompt` gpui-pre-0.3.10/src/window.rs:6711, `has_active_prompt` :6769, `on_window_should_close` :6914 (wired on macOS, Windows, X11 and Wayland).
  - `App::hide` / `hide_other_apps` / `unhide_other_apps` app.rs:1606-1618.
  - **No termination veto:** the macOS app delegate implements only `applicationWillTerminate:` (gpui-pre-macos-0.3.10/src/platform.rs:91, :1380), with no `applicationShouldTerminate:`. The internal `on_quit` callback (app.rs:1152) is not public. Windows answers `WM_QUERYENDSESSION` with 1 unconditionally (gpui-pre-windows-0.3.10/src/platform.rs:1203).

## Decisions
- Every quit request goes through one function. It is called by the `Quit` action (⌘Q/Ctrl-Q and the menu's "Quit Wisp") and by `on_window_should_close`:

  | dirty tabs | prompt already up | result |
  |---|---|---|
  | none | any | `quit_app` |
  | some | no | prompt "Discard unsaved changes?" with the detail "Quitting closes all tabs." and the buttons [Discard, Cancel]. Discard → `quit_app`; Cancel or dismiss → nothing. |
  | some | yes | nothing (no second prompt) |

- Closing tab T, from any of the paths at root.rs:300-315 and :420-426: if T is clean, close it as now. If T is dirty, prompt "Discard unsaved changes?" with the detail `"<title>" has unsaved changes.` and the buttons [Discard, Cancel]. On Discard, close T, looked up by entity after the answer and not by the index captured before it; if T is gone by then, do nothing. With a prompt already up, do nothing.
- `on_window_should_close` still returns `false` (task 79: no window is torn down while the app runs).
- The `WISP_CLOSE_WINDOW_AFTER_MS` hook keeps calling `quit_app` directly (the measurement scripts rely on it).
- Do not add objc/cocoa code to intercept `terminate:`. Dock Quit and the other uninterceptable paths are a documented gap, not part of this task.

## Acceptance
- [ ] AC1 A pure helper decides the quit result from (any dirty, prompt active), following the first table above. Tests named `quit_step_*` cover all three rows.
- [ ] AC2 A pure helper decides the tab-close result from (tab dirty, prompt active). Tests named `close_step_*` cover clean, dirty and dirty-with-a-prompt-up.
- [ ] AC3 The `Quit` action and `on_window_should_close` both call the single quit function from the Decisions. No other caller of `quit_app` remains apart from the hook and the `on_window_closed` backstop.
- [ ] AC4 Every tab-close path at root.rs (the × button and the `CloseTab` handler) goes through the tab-close prompt. Discard closes the tab by entity identity after the answer.
- [ ] AC5 macOS only: the "Wisp" menu holds Services, a separator, "Hide Wisp" (`cmd-h`, `App::hide`), "Hide Others" (`alt-cmd-h`, `App::hide_other_apps`), "Show All" (`App::unhide_other_apps`, no key), a separator and "Quit Wisp". The new actions are bound only on macOS. A macOS-only test `hide_bindings_parse_on_mac` asserts both keystrokes parse, and `bind_keys_indices_match_names` still passes.
- [ ] AC6 `scripts/measure/quit-check.sh` passes on a `scripts/release/package.sh macos-arm64` bundle whose Info.plist `CFBundleIdentifier` is changed to `io.github.aindriub.wisp.test83` and which is re-signed ad hoc. With no dirty tabs, quit is not blocked. The script signals only pids it started; never use pkill or killall.
- [ ] AC7 `## Notes` lists, per platform, which quit and close paths ask and which cannot: Dock Quit, AppleScript quit, logout, restart and shutdown on macOS; `WM_QUERYENDSESSION`/logoff on Windows; SIGTERM and session end on Linux. Each entry cites the GPUI line that makes it uninterceptable. Notes also give the user's hand-check steps with expected results: ⌘Q with a dirty tab → Cancel keeps the app; ⌘Q → Discard quits; the red button and ⌘W on a dirty tab, the same; ⌘H hides.
- [ ] AC8 `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Checks
- `quit-close-unit` command — AC1, AC2, AC5. cargo test does not write JUnit XML here, so the command asserts the pass count itself (≥ 8 on macOS: 3 quit, 3 close, 1 hide, plus the existing index test).
  run: `cargo test -p wisp-app -- quit_step_ close_step_ hide_bindings_parse_on_mac bind_keys_indices_match_names 2>&1 | grep -E '^test result: ok\. ([8-9]|[1-9][0-9]+) passed'`
- `app-tests` command — no regressions
  run: `cargo test -p wisp-app`
- `quit-check` command — AC6
  run: `scripts/measure/quit-check.sh <path to the re-id'd Wisp.app>`
- `lint` command — AC8
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `routing` read — AC3, AC4
  how: in the diff, every `quit_app` call site and every tab-close call site in root.rs goes through the new helpers. Discard looks the tab up by entity.
- `notes` read — AC7
  how: Notes has the per-platform table with GPUI line citations, and the hand-check steps.

## Out of scope
- Intercepting `terminate:` or `WM_QUERYENDSESSION` by any means (objc swizzle, a new dependency, patching GPUI).
- Auto-saving drafts, or a "Save" button in the prompt.
- The prompt `open_collection` already shows (root.rs:142-190) stays as it is.
- Layout changes in root.rs (task 88) and the Linux window icon in main.rs (task 87).
- docs/architecture.md and the README: the scribe records the gap.

## Notes
