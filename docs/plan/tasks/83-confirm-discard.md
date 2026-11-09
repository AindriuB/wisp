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
GPUI paths are in `gpui-pre*-0.3.10/src` under the cargo registry.

Paths that ask (all platforms): Cmd-Q / Ctrl-Q, the macOS "Quit Wisp" menu
item, the window close button (`on_window_should_close`), the × button on a
tab, Cmd-W / Ctrl-W, the palette's "Close tab" and the banner's close button.

Paths that cannot ask, so a dirty draft is dropped:

| Platform | Path | Why it cannot be intercepted |
|---|---|---|
| macOS | Dock "Quit", AppleScript `quit`, logout, restart, shutdown | The app delegate implements only `applicationWillTerminate:` (gpui-pre-macos-0.3.10/src/platform.rs:91, :1380) and no `applicationShouldTerminate:`; `terminate:` cannot be vetoed. The internal `on_quit` (gpui-pre-0.3.10/src/app.rs:1152) is not public. |
| Windows | `WM_QUERYENDSESSION` (logoff, restart, shutdown) | Answered with 1 unconditionally (gpui-pre-windows-0.3.10/src/platform.rs:1203); `WM_ENDSESSION` then runs `handle_end_session` (:1204). |
| Linux | SIGTERM, session end | GPUI installs no signal handler (no SIGTERM/signal handling under gpui-pre-linux-0.3.10/src; the only "signal" hits are calloop and frame-callback code), so the default action ends the process. |

Hand-check (macOS, release bundle or `cargo run`):
1. Edit a request so its tab shows the dirty dot. Press Cmd-Q. Expect the "Discard unsaved changes?" prompt ("Quitting closes all tabs."). Cancel: the app stays and the draft is intact.
2. Cmd-Q again, Discard: the app quits.
3. With a dirty tab, click the red window button. Same prompt; Cancel keeps the app, Discard quits.
4. With a dirty tab, press Cmd-W (or click its ×). Prompt says `"<title>" has unsaved changes.`; Cancel keeps the tab, Discard closes only that tab. A clean tab closes with no prompt.
5. Press Cmd-H: the app hides. The Wisp menu shows Hide Wisp, Hide Others (Opt-Cmd-H) and Show All.

## Review round 1 — findings

Reviewed HEAD: 2d92b6a98b85ac65b605a640716eb15c043be619

- [P1] crates/app/src/main.rs:164-169 — The global `Quit` handler does nothing in ordinary use. Keystrokes and the menu (`App::dispatch_action`, gpui-pre-0.3.10/src/app.rs:2963-2973) reach global listeners inside the active window's update (window.rs:6619-6635), while that window is taken out of `cx.windows` (app.rs:3281). The nested `handle.update` therefore returns `Err("window not found")` (app.rs:2336-2350), and `let _ =` discards it. Input: window focused, cmd-q / ctrl-q or menu "Quit Wisp", dirty or not → app stays open, no prompt. `quit-check.sh` misses it (it quits via AppleScript `terminate:` and `WISP_CLOSE_WINDOW_AFTER_MS`, never the `Quit` action).
  route: implementer
  fix: e.g. `cx.defer(move |cx| handle.update(...))`, or handle `Quit` on `WorkspaceView` via `cx.listener` so the window is passed in. The global handler must still quit when no window is open. Add a test or check that exercises the `Quit` action with a window active.
- [STYLE] crates/app/src/workspace.rs:774-792 — the new `close_step_*` tests and `use super::{CloseStep, close_step};` sit before the module's `use super::*;`; the import is redundant. Open `mod tests` with `use super::*;` then the tests, as elsewhere in the crate.
  route: implementer

## Residual (accepted, follow-up)

- [P3] ui/root.rs:193 and ui/sidebar.rs:678 — the open-collection and delete-from-disk prompts don't set `prompt_up`, so on macOS a quit during one of those native sheets (with a dirty tab) queues a second "Discard unsaved changes?" sheet behind it. No data loss. Fixing it touches the out-of-scope open-collection prompt and sidebar.rs (outside Owns), so it is a follow-up, not this task.
