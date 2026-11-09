# Plan

What is open, in priority order. Only `scribe` edits this file.

Each item: one line of what, one line of why it matters, and its blocker if it
has one. When an item is in flight, mark it with its task ids. Delete items you
no longer want rather than letting them rot — a plan nobody trusts is worse
than no plan.

---

## Now

<!--
### <title>
<What it is.> <Why it matters now.>
**Blocked by:** <or omit>
**In flight:** 01, 02
-->

### v0.1.0 is tagged
Tagged 2026-11-03 before the Windows hand test; the macOS Apple-silicon candidate was hand-tested and passed. See HISTORY. The Windows build was hand-tested afterwards and passed.

### HTTP client v1
Build the portable GPUI HTTP client in three milestones, in this order. Spec: `docs/plan/specs/2026-10-29-http-client-v1.md`. Product is Wisp; crate naming is settled in `docs/conventions.md#naming`.
- **M0 — Walking skeleton (complete).**
- **M1 — Core and storage (complete):** tasks 10-20 landed. The full model, `resolve`, body store, full event set and cancel, `toml_edit` storage with secrets and watcher, and the fixture integration test all pass without a window. See HISTORY.
- **M2 — UI v1 (complete):** tasks 21-37 landed (layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings, icons, integration and budgets). Window hand check passed on 2026-11-02; see HISTORY. Three defects were found there and all three are fixed and hand-checked in the running app (tasks 38, 39, 40, 41; see HISTORY).
**Budgets (task 35, commit add9842, macOS 27.0.1 arm64, hardware renderer):** binary 18,965,216 B of 20,971,520 (headroom about 2 MB); idle RSS 99,808 KiB (about 97.5 MiB) of 120 MiB; after one request 104,784 KiB (about 102.3 MiB) of 128 MiB. Measured with a 64 KB response; RSS was about 127 MiB with a 20 MB response loaded.
- **M3 — v1 hardening (complete):** tasks 52-61 landed (request sub-tab scrolling, autosend resolve error, Cookies and failed-status fit, tab close button, engine roots and TLS cleanup, storage hardening, history and environment fixes, streamed File and Multipart bodies, integration and budgets); see HISTORY. Task 61 added a 720×480 minimum window and fitted the URL row and status lines to narrow panes; its hand check also closed the task 53 failed-line check.
**Budgets (task 60, commit f116a69, macOS 27.0.1 arm64, hardware renderer):** binary 19,181,072 B of 20,971,520; idle RSS 70,496 KiB (68.8 MiB) of 120 MiB; after one request 75,648 KiB (73.9 MiB) of 128 MiB. RSS fell about 28.4 MiB since task 50 although the binary grew; the macOS version changed between the runs, which likely explains it, so do not read it as a broken measurement.
**Residuals (53):** at 600 px the "Expires / Max-Age" header and typical dates are cut (accepted); below 584 px the rows' vertical scrollbar starts off-screen until you scroll sideways; the Flags width (256 px, `FLAGS_TEXT_PX` 250) was measured against the macOS system font only.
**Residuals (55):** closing a dirty tab had no prompt; fixed by task 83.
**Residuals (54):** a `,` or `)` in a variable name can make the autosend `problems=` list ambiguous (nothing parses past the prefix); if autosend ever runs after an environment is selected, an unresolved `{{x}}` inside a secret's value would print `x`.
**Residuals (57):** a streamed body over 1 MiB is now resent on 307/308 (task 86); PUT/PATCH on 301/302 still shows the 3xx as-is. Special files (/proc entries reporting size 0, FIFOs, /dev/stdin) are sent empty or refused with "not a regular file"; a read-to-EOF fallback was rejected by the user. Bytes bodies still pass user Content-Length/Transfer-Encoding through. No test checks the 64 KiB chunk limit. `block_in_place` runs on every response chunk. The 20 MiB tests compare bytes but do not measure client memory.
**Residuals (56):** the `install_crypto_provider` unit test does not exercise the Err branch.
**Residuals (59):** a true swap (one var moving each way in the same save) cannot be made safe with two writes and is accepted; `gains_moved_name` treats an unparseable other file as empty (harmless while `render` parses first); the files are re-read after `render`; a dangling non-.toml symlink logs a skip warning.
**Residuals (58):** rejecting a symlink diamond takes about 7-10 s before the error; a Windows trailing-dot or trailing-space `.toml` alias passes (`request.rs:221`); the name check at `request.rs:224-226` is unreachable.

**Budgets (task 61, macOS arm64, hardware renderer):** binary 19,181,072 B (+0); idle RSS 98.7 MiB of 120 MiB; after one request 103.6 MiB of 128 MiB. RSS is about 30 MiB above task 60's figures; the change cannot explain it, so it is probably run-to-run noise. Re-measure main on the same machine to confirm the baseline.
**Residuals (61):** at 720 px wide the sidebar can still take 480 px and panes can be dragged to 100 px (clipping covers it); the minimum size was checked on macOS arm64 only. Do not re-add `overflow_hidden` to the `resizable_panel()` calls: it clips the splitter's 9 px grab band to 5 px.

**Residuals (72):** the Windows archive ships LICENSE, NOTICE and THIRD-PARTY-NOTICES with CRLF endings (Windows checkout); a `.gitattributes` `eol=lf` would fix it. Re-running a failed tag-push run creates a second draft for the same tag; delete the stale one before publishing. The tag-push path has not been exercised. The packaged `Wisp.app` launched with a window but nobody has looked at it; user hand test pending at release.

**Residuals (70):** the Bundled assets header in `scripts/notices/generate.sh:37` hard-codes "19 SVG icons" and the gpui-kit-assets and gpui-component versions, so it can go stale without the CI `notices` job noticing; `about.toml` accepts OpenSSL and BSL-1.0 though nothing uses them (OpenSSL contradicts the rustls-only rule in `docs/conventions.md#dependencies`).

**Residuals (67, optional P3):** `safe_sidecar` (`ops.rs:416-420`) still compares with `to_lowercase` rather than `fold_case`; it errs toward keeping a sidecar, so no data is lost. Task 67's head has not run on Linux; its own PR CI is the first run.

**Residuals (66):** the "Request failed" fallback heading is written in three places (`mod.rs:50`, `mod.rs:167`, `body.rs:727-729`); a shared helper would keep them in step. The macOS arm64 hand check of the error panel passed on 2026-11-03 (with task 69's sub-tab scroll check); only the Windows and Linux hand checks remain for the user.

**Residuals (62):** (P3) `clean()` (`ops.rs:83-87`) rejects a leading dot or `environments` with `to_lowercase`, while the new check in `clean_node` (`ops.rs:172-177`) uses `fold_case`. Paths through `clean()` only (move destinations, create targets) still use plain lowercasing; on Linux `move_node(r, "a.toml", "environment\u{17f}")` gives Io NotFound while `delete_node` of the same name gives Invalid. Fix: switch `clean()` to `fold_case` and drop the duplicate block; candidate for the next polish wave. The comment at `ops.rs:172` reads as if that block were the Linux fix, but the `collection.toml`/`folder.toml` check was. `delete_refuses_wide_case_fold_alias` (`ops.rs:1397-1400`) could now assert Invalid unconditionally. No test covers a non-existent reserved source spelling for rename or move (only delete).

### v0.1.1 wave
Patch release after v0.1.0. Done: tasks 86 (streamed File and Multipart bodies resent on 307/308), 84 (secrets redacted from Debug output in wisp-core), 85 (storage rejects Windows-unsafe and aliasing names), 83 (ask before discarding unsaved changes on quit and close), 88 (narrow-window layout limits and Cookies scrollbar) and 87 (Linux window icon, app id and desktop launcher; Linux hand checks still pending) and 89 (workspace version 0.1.1); see HISTORY. Remaining before tagging: a release-candidate dry run, the user's hand tests (the checklist in task 83's HISTORY entry), then tag and publish with the user's approval.

### Storage-safety follow-ups (from task 85, not v0.1.1)
- The Windows test `sidecar_rejects_8dot3_short_name_of_sibling_toml` fails rather than skips on a volume with 8.3 names off; it should return early when `B~1.TOM` does not exist.
- No test fails if the symlink-to-`.toml` check in `sidecar_path` is disabled; Unix is covered by the dev/ino check.
- The reserved-name rule also rejects display names like "CON.txt" whose file slug would be harmless (decided).
- A folder containing `con.toml` can still be moved, while moving `con.toml` itself is refused.

### Follow-ups from task 86 (not for 0.1.1)
- An API key sent in a custom header (for example `X-Api-Key`) still crosses origins on a redirect; only the headers reqwest strips are removed on a streamed resend.
- The streamed resend checks only each file's length, not its contents, so a file edited to the same size between sends is resent with the new bytes.
- `EngineError` messages include the request URL. Fix with `reqwest::Error::without_url()` so a URL carrying a secret does not reach the UI.

### Pane-scrollbar defects (tasks 46-49, fixed and hand-checked 2026-11-02)
All four defects found hand-checking 43-45 are fixed (see HISTORY). Combined binary 19,014,912 B (+0 B).
**Residuals:** many inherited headers can squeeze the request-header table (the inherited list keeps natural height per spec). (Path, Auth, Settings, Multipart and File sub-tabs now scroll; task 52.) Do not add `.flex()` to the `div().w_full()` wrapper around KvTable virtual rows (46).
**Header limit (47, resolved by 50):** the limit is now 1,000 headers; see the Task 50 residuals below.

### Reqwest 0.13 follow-ups (task 50)
Optional, none blocking. Closed by 51: the runtime TLS test (`crates/core/tests/tls.rs`) and the `webpki-root-certs` `default-features = false` consistency fix. The `engine.rs:87`, `install_crypto_provider` and `https://localhost` residuals were closed by task 56 (see HISTORY).
Budgets after 50: binary 19,114,864 B of 20,971,520; idle RSS 97.2 MiB; after one request 102.3 MiB.

### Open budget and measurement questions (from M0)
**Per-platform budgets (done, task 65):** set by the user on 2026-11-03 and enforced by `measure.yml` through `scripts/measure/budgets.json` (table in `docs/architecture.md#budgets`). Both Linux backends (x11 and wayland) are kept. Intel macOS was dropped from 0.1.0 on 2026-11-03 (task 71).
**Residuals (71):** if the screenshot times out or `import` fails silently, the log line is a bare `screenshot: ` with no reason (`measure.sh:236`); `dpkg -S` diversion lines would become a bogus package entry in `packages.txt` (none today); the `unowned:` branch of the packages step has never executed in CI.
**Residuals (65):** the Windows over-budget path (`within: false`, exit 3) has never executed; the logic reads correct and the first real over-budget run is its test.
**Residuals (64):** Linux idle RssAnon is about 4.8 MB higher with the link flags (58,168 vs 53,332 KiB for panic/opt-level alone) while after-request is lower; likely noise, recheck when it next shows in a run. `.cargo/config.toml` target rustflags are replaced by any `RUSTFLAGS` env and need lld (GNU ld rejects `--icf=all`). The responsiveness hand check was done on macOS arm64 only (same speed); Windows and Linux builds were not hand-checked. `panic = "abort"` means any panic ends the process.
**09 review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s final catch block (`:184-192`) prints only the message, not the stderr tail nor whether stderr was empty.
**Residuals (68):** `measure.ps1` has never run locally (no pwsh); its CIM-failure path (`Get-PrivateWs` returning null) and its bad-budgets and one-element-array checks have not executed at runtime, only been read. The Windows CI job covers the normal path.
**Residuals (73):** macOS portable mode is unsupported by decision, but `settings.rs:70-75` still honours a `portable` marker inside `Wisp.app/Contents/MacOS` (which edits the signed bundle), so the code is out of step with the README. The README's claim that Finder, not `unzip`, sets quarantine is unverified until the user's macOS hand test. Its platform-testing statements describe the state at release and must be confirmed before tagging. It needs a line for the app icon when task 76 lands and must drop any console or runtime caveats when task 77 lands; none are present today.
**Residuals (77):** in a Windows release launched from Explorer all stderr is lost (settings warnings, window-open failures, panic messages under `panic = "abort"`); a crash/error log file or attaching to the parent console would be a future feature. A local `cargo build --release` on Windows without `--target` applies `crt-static` to build scripts and proc-macros too (works on MSVC, noted only).
**Residuals (75):** `scripts/release/check-glibc.sh` discards objdump's own error text on failure, and its header's exit-2 description is narrower than its use. Linux RssAnon rose about 14% on ubuntu-24.04 (idle 61,132 vs 53,376 KiB; after request 51,256 vs 45,096) with no explanation; likely different mesa and libraries, still within budget.
**Residuals (76):** `generate.sh --check` does not flag stale extra files; shellcheck SC2209 false positive at `scripts/icon/generate.sh:18` (`mode=write`) makes shellcheck exit 1; `package.sh:58` `assetutil | grep -q` could SIGPIPE under pipefail on a very large catalog (task 78 will harden it); the macOS runner depends on the unpinned default Xcode having Icon Composer support; Linux window icon and `.desktop` file done by task 87; the user's visual check of default, dark and tinted is part of the release hand test. The README gains an icon line only if it discusses appearance (it does not).

**Residuals (78):** the saved keychain search list is not restored if it was empty (harmless on a fresh runner); the `notarytool` timeout JSON shape is unverified; Windows code signing (SmartScreen) is not done, candidate follow-up Azure Trusted Signing; a macOS portable-mode marker inside the bundle would break the notarised signature (see Residuals (73)).
**Residuals (74):** the non-macOS platform modifier is labelled "Win" where Linux calls it Super (no current binding uses it); only enter, escape and tab get display names, other named keys are uppercased.
**Residuals (63):** `measure.sh:131-133` vulkaninfo fallback may name a different device than GPUI picked, and the JSON does not say which source produced `renderer_device`. `measure.ps1` was not parse-checked locally (no pwsh); only CI covers it.
**06 (resolved):** the method and environment selects now show chevrons (task 37, hand-checked 2026-11-02). UI automation needs the window frontmost.

### Carry-forward: crates/core
**Debug redaction follow-ups (84):** multipart text values (`PreparedPartValue::Text`) still print in Debug; URL path and userinfo secrets are not redacted in `PreparedRequest` Debug (only the query is); `EngineError` messages include the URL; `ResponseCookie` derives Debug and holds cookie values; a proxy password with an unencoded `/`, `?` or `#` still prints in `ClientSettings` Debug (reqwest rejects such proxies anyway). None are in 0.1.1.
**resolve nits (11):** a verbose-mode regex `(?x)...#c` can be misreported as `InvalidPattern`; problem dedupe keys on the Debug string; a test helper has a leftover `eprintln`; a scheme-less URL's `://` search can land in the query (the engine rejects such URLs anyway).
**Template helpers (22):** no resolve-level test pins `apply_path_params` to `path_range_plain` (an unclosed `{{` in the authority). `query_start_is_linear` asserts wall-clock < 100 ms and could flake on loaded CI. `expand` allocates all var refs before the budget check (linear, not unbounded). `lookup_var("")` is Unresolved while `resolve` reports Syntax for `{{ }}`. `query_start` can return a `?` inside a fragment.
**Types test (10, optional P3):** the File/Multipart unsupported test checks only the error kind, not the message.
**Body store (12):** the uid-ownership check on the spill directory has no test (it needs root). Three body tests spill into the real `<temp_dir>/wisp-<uid>` rather than a test base directory. The non-unix spill-directory code is not compiled or tested on this Mac.
**Client cache (16):** `ClientCache` builds a client while holding a std `Mutex`. It is tested only through `ClientCache::get`, not `Engine::submit`.
**Engine tests and limits (16):** the multipart test leaves a temp file behind if an assertion fails. The app's `BODY_LIMIT` is now 64 KiB (first 64 KiB read from `bodies()`).

### Carry-forward: crates/app
**Hand check (69):** the macOS arm64 check of the `.id("…-editor")` removal passed on 2026-11-03 (long Path, Auth and Settings sub-tabs scroll, checked with task 66's build); only the Windows and Linux hand checks remain. Possible flake, unconfirmed: wisp-core `a_20_mib_file_body_streams_with_content_length` failed once in a heavily parallel full-workspace run and passed alone; watch for it.
**Residuals (79):** the Touch Bar Dock-Quit crash root cause is upstream (`accesskit_macos` subclass restore versus KVO) and not reported upstream yet. Cmd-Q and window close now ask before discarding drafts (task 83); Dock Quit and system shutdown still do not (documented in `docs/architecture.md#app-lifecycle`). The `WISP_CLOSE_WINDOW_AFTER_MS` hook calls `quit_app` directly, so `quit-check.sh` does not exercise the close button (hand-tested). No About menu item (GPUI 0.3.10 `OsAction` lacks it). Hide, Hide Others and Show All were added by task 83 on macOS. shellcheck SC2010 (`ls | grep`) in `quit-check.sh`.
**Escape (21):** Escape is bound with context None to `CancelRequest`. Hand-checked in task 41: Escape inside the focused URL input cancels a send (status "Cancelled").
**measure.sh (21):** the `WISP_EVENT` line was not separately confirmed in the output; status "measured" implies it.
**KvTable (29):** hand-checked in task 35 (typing in Params updates rows and the URL; Tab goes name, value, next name). Accepted residual: above the 50-row virtualisation threshold Tab cannot reach rows outside the viewport, because unpainted rows have no focus handle; scroll-to-focus is deferred. The table only virtualises if its container gives it a definite height (task 31). Optional: `locate` in `crates/app/src/ui/kv.rs` is O(n) per keystroke.
**Folder order (39, optional):** a list naming a request in both forms (`["a","a.toml"]`) has only its first match edited by rename, move and delete. `a.toml` is ambiguous when both `a.toml` and `a.toml.toml` exist. Hand-checked in task 41: rename and delete in the running app update `folder.toml` and the tree follows its order.
**Storage ops tests (23, optional):** `case_alias_sidecar_counts_as_shared` only exercises identity on delete and is vacuous on non-case-folding file systems.
**Palette and highlight (25, optional P3):** `palette.rs:136` lowercases only the first char of a label, so chars with a multi-char lowercase (such as "İ") never match. Write the hover " (uses a secret)" suffix format into the spec for task 31.
**Formatting (26/36, optional P3):** `sniff_kind` strips the BOM after the UTF-8 check, so a sniff window cut inside a BOM reads as Text (not a real-world issue).
**Settings (27):** ToggleTheme and ToggleLayout (task 28) surface `settings::save` errors to the user, because save now returns Err on an unparsable or unreadable file instead of overwriting it.
**Draft and URL sync (24):** accepted residual: `Request.vars` has no `DirtySections` flag (vars change only through external edits in v1), so a draft can be dirty with no section highlighted. `merge_parsed_params` aligns old and parsed params with a 3-level order-preserving LCS (name+value, name, same slot); it is O(n*m) per keystroke, fine at real param counts (about 8 MB at 1000 params). Names containing `=` or `&`, and empty names, do not round-trip (documented on `display_url`). Optional cosmetic: group the test helpers `pairs` and `kv` at the top of the `url_sync` tests.
**Workspace shell (28):**
- *Measurement:* superseded by the task 35 figures under M2 (the shell measured about 96 MiB idle and binary 15.3 MB when it landed).
- *Hand-checked in task 35:* ⌘O from A to B (with dirty tabs: Discard, Open panel) closes A's tabs and resets the environment; ToggleTheme and ToggleLayout from the palette write only `theme` and `split` to settings.toml; theme="system" follows macOS Light/Dark live. The dev build wrote `~/Library/Application Support/wisp/settings.toml` during the check.
- *Mislabelled autosend error (fixed in task 54):* the failure line now reads `WISP_EVENT failed kind=Resolve problems=<names>`; see HISTORY.
- *Gaps:* (tab close button landed in task 55.) Routing, history and watcher wiring have no end-to-end tests because gpui test support is not enabled; the open-collection test covers only the `open_needs_confirm` helper.
- *Optional P3:* `bind_keys_indices_match_names` does not prove `key(i)` binds the right action.
**Sidebar (30):**
- *Hand-checked in task 35:* env switching, rename (file and tab renamed) and delete (prompt names the item; file and sidecar removed). Tree order was wrong and rename/delete left stale `order` names; fixed in task 39 at the storage layer, running-app check passed in task 41. Create and move were not separately reported.
- *Optional:* a node created inside a collapsed folder is selected but hidden, because the parent is not expanded. An environment literally named "No environment" clashes with the `NO_ENV` entry. The delete-prune `collapsed.retain` has no unit test (correct by inspection).
**URL bar and sub-tabs (31):**
- *Hand-checked in task 35:* URL and Params live sync, highlight, hover tooltip ("baseUrl = … · collection"), status colouring. Params checkbox resync while the URL is focused fixed in task 40, hand-checked in task 41 (typing `?a=1&b=2` with no caret jump; untick and re-tick while focused resync). Not reported: secret `••••` hover, Cancel while sending.
- *Decision on resync:* the "only on DraftReplaced" rule holds for Headers and Settings. Params, the URL input (when unfocused) and Path also resync on `DraftEdited`, behind equality guards so an edit never echoes back.
- *Optional:* a non-numeric or zero timeout is silently ignored in Settings (`settings.rs:73-75`). The URL bar also refreshes on `CollectionChanged` (harmless). A URL that does not round-trip through `apply_url_text` (an enabled param with an empty name) can still produce one extra edit.
**Auth and body editors (32):**
- *Hand-checked in task 35:* JSON sidecar edit and ⌘S writes the sidecar only; Bearer, Inherit, Bearer restores the token (masked, reveal toggle); "Inherits Bearer from collection" line; Format. Multipart Text plus File part send hand-checked in task 41 (file part sha256 matched).
- *Lesson for later UI work:* gpui-component 0.8.1 `InputState::set_value` emits no Change event, so any programmatic set must update the model or draft by hand (the cause of the round-1 P1/P2).
- *Optional:* `path.exists()` stats the filesystem on every render of the File body; a hand-typed relative File path is checked against the process cwd rather than the collection dir, so "File not found" can be wrong; a Choose… pick into a row deleted while the dialog is open is silently dropped.
**Palette and environment switcher (34):**
- *Hand-checked in task 35:* all passed (`gu`, `theme`, `layout`, ⌘E with dev pre-selected, Escape, shortcuts afterwards). Focus loss after other unmounts fixed in task 38, hand-checked in task 41.
- *Lesson:* in gpui-pre 0.3.10 key bindings dispatch before key-down listeners, and a context-less binding (such as escape to CancelRequest) beats component-context ones. Overlays must handle such actions with `on_action` on their own div, and an overlay that takes focus must restore it on dismiss.
- *Optional:* with more than 50 environments the active one past the limit is not pre-selected. ⌘K or ⌘E while the palette is open reloads it in place and clears the query rather than toggling it closed.
**Response viewer (33):**
- *Hand-checked in task 35:* Pretty keeps key order; a 20.3 MB response shows Raw with Pretty disabled; two Set-Cookie rows; header cell click and ⌘C copy. Hand-checked in task 41: Raw prefetch while scrolling, over-limit Pretty tooltip, Save output sha256. The 4 MB Raw cap line was verified in task 42 (see below).
- *Raw scrollbar and jump keys (42, hand-checked 2026-11-02):* Raw and Pretty show a draggable vertical scrollbar; End/⌘↓/Ctrl+↓ load to the 4 MB cap and jump to the bottom, Home/⌘↑/Ctrl+↑ jump to the top (any modifier works with End/Home; keys are handled on the BodyPane focus handle, and with the URL input focused End moves its text cursor). The 4 MB cap line ("Showing the first 4 MB — Save to read the rest") is verified on a 250,000-line body.
- *Optional P3 (42):* the `jump_still_valid` asserts sit inside the key test and could be split; when the 4 MB boundary falls mid-line the cap line follows a partial final line. Budgets measured: binary 18,998,304 B (+33,088), idle RSS 100,064 KiB, after request 104,512 KiB.
- *Scrollbars (43-45, hand-checked 2026-11-02):* sidebar, response Headers and Cookies and KvTable now have draggable scrollbars. Budgets: combined binary 19,014,912 B. See the pane-scrollbar defects below.
- *Decision:* headers are per-cell selectable divs with ⌘C copy, not the gpui-component `Table`. There is no partial-text selection and no keyboard cell navigation.
- *Optional:* header selection is cleared on every `ResponseChanged`, which sub-tab and Pretty/Raw switches also emit (consider a separate view-change notify). `failed_create_keeps_existing_target` silently skips its assertions when run as root. `File::create` truncates before writing, so a failed Replace leaves no original (not an atomic replace).
**Icon assets (37):**
- *Measurement:* binary 18,981,728 B (+16.5 KB over main), idle RSS about 97.3 MiB, after-request about 101.9 MiB.
- *Facts:* 19 icons embedded from gpui-kit-assets 0.8.1 (Apache-2.0, NOTICE kept) in `crates/app/assets/icons/`. If dialogs or notifications are used, add CircleCheck, CircleX and Info. The walk test fails naming any `IconName` used in app sources that is not embedded.
- *Hand-checked in task 35:* select chevrons, checkbox ticks, eye, × and search icons render. Tip: launching the bare binary may not expose a window to computer use until `WISP_AUTOSEND_URL` is set; wrap it in a scratch `.app` bundle with a bundle id.
- *Optional:* `main.rs`'s run line was split into `let app = ...with_assets(...); app.run(...)` to avoid a rustfmt reflow.

### Carry-forward: crates/storage
**Codec (13):** `write_vars` never reorders existing keys. Flipping an explicit `enabled = true` to false may drop a trailing comment on that line. New entries in multi-line inline arrays get `toml_edit` default spacing. The Windows readonly-target save path in `atomic_write` is untested. `write_auth` keeps its signature, so callers skip the call when the value equals that file's default and the file had no `[auth]`; use `atomic_write_private` for files that hold secrets.
**Watcher (14):** nested directory renames within the same 100 ms window may emit one spurious `Changed` for a directory-derived path (accepted); the M2 reload must re-check the disk and tolerate `Changed` for non-file paths. On Linux notify follows symlinks into existing linked directories by default; consider `follow_symlinks: false`. The event-time `p.is_dir()` follows symlinks while `skip_directory` does not (harmless).
**History (15):** Concurrent writers could race rotation of `history.jsonl`; v1 has one writer.
**Request files (17):** Method parsers unified in task 58 and `.toml` sidecar names are rejected. Task 85 closed sidecars aliasing sibling request files, trailing dot/space names, reserved device names and Unix hard links; hard links are not detected on Windows (std has no stable identity API). Windows `::$DATA` aliases are not covered. The drive-prefix check rejects any name with `:` as its second byte. Sidecar symlinks have a check-then-use race. `bare_relative_request_path_allows_inside_symlink` uses the process-global `set_current_dir`; safe today because all other tests use absolute paths, but a future relative-path test could flake.
**Collection tree (18, walk fixed in 58):** the cycle rule is now ancestor-only with a 10,000-folder whole-walk budget; directory symlinks pointing outside the root are followed; a broken `folder.toml` fails the whole open; `scope_for` with a non-canonical absolute root returns no folders (callers pass relative paths).
**Environments (19):** names with spaces are now rejected, so an existing environment with a space in its name will not load; on case-insensitive filesystems `Dev` and `dev` map to the same file. A symlinked `environments/dev.toml` loads but is not listed, because `file_type` does not follow links; one `file_type` error aborts the whole listing. The plain file is written before the secrets file, so a save that half-fails can lose a var that is moving between them.
**Integration test (20, accepted):** the JSON body fixture has no comment because JSON cannot hold one.

## Next

### Follow-ups from task 83 (not v0.1.1)
- No automated test sends the real `Quit` action with a window active. It needs gpui `test-support` in `crates/app/Cargo.toml`, or `quit-check.sh` sending the action instead of AppleScript `terminate:`. The review P1 (the handler did nothing while a window was active) got past the tests for this reason.
- The open-collection prompt (`ui/root.rs:193`) and the delete-from-disk prompt (`ui/sidebar.rs:678`) do not set `prompt_up`, so a quit during one of them with a dirty tab queues a second "Discard unsaved changes?" sheet on macOS. No data is lost.

### Follow-ups from task 88 (not v0.1.1)
- The Cookies column header now scrolls up with the rows instead of staying pinned (accepted by the user, 2026-11-09). Fix: a separate header x-scroller that follows the rows' horizontal offset.
- When the Cookies table is tall, sideways scrolling works only with the pointer over the rows.

## Someday
