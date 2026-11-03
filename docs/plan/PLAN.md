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

### HTTP client v1
Build the portable GPUI HTTP client in three milestones, in this order. Spec: `docs/plan/specs/2026-10-29-http-client-v1.md`. Product is Wisp; crate naming is settled in `docs/conventions.md#naming`.
- **M0 — Walking skeleton (complete).**
- **M1 — Core and storage (complete):** tasks 10-20 landed. The full model, `resolve`, body store, full event set and cancel, `toml_edit` storage with secrets and watcher, and the fixture integration test all pass without a window. See HISTORY.
- **M2 — UI v1 (complete):** tasks 21-37 landed (layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings, icons, integration and budgets). Window hand check passed on 2026-11-02; see HISTORY. Three defects were found there and all three are fixed and hand-checked in the running app (tasks 38, 39, 40, 41; see HISTORY).
**Budgets (task 35, commit add9842, macOS 27.0.1 arm64, hardware renderer):** binary 18,965,216 B of 20,971,520 (headroom about 2 MB); idle RSS 99,808 KiB (about 97.5 MiB) of 120 MiB; after one request 104,784 KiB (about 102.3 MiB) of 128 MiB. Measured with a 64 KB response; RSS was about 127 MiB with a 20 MB response loaded.
- **M3 — v1 hardening (in progress):** tasks 52-60. Landed: 52 (request sub-tab scrolling), 54 (autosend resolve error), 53 (Cookies and failed-status fit), 55 (tab close button), 56 (engine roots, provider log, TLS URLs), 58 (storage method parser, folder walk budget, sidecar rejection), 59 (history previews, linked environments, safe save order); see HISTORY. Open: 57, 60. Task 60 carries the integration check and budgets (including the deferred `measure.sh` run for 54 and the hand check of 52's scrolling), including the deferred hand check of 55 (three tabs, close active, close while sending, URL focus kept) and of 53 (Cookies table and failed-status line at 600 px and 200 px; watch for a stray 1-2 px sideways scroll at exactly 600 px; measure the real binary size, since 53's branch predated 52/54).
**Residuals (53):** at 600 px the "Expires / Max-Age" header and typical dates are cut (accepted); below 584 px the rows' vertical scrollbar starts off-screen until you scroll sideways; the Flags width (256 px, `FLAGS_TEXT_PX` 250) was measured against the macOS system font only.
**Residuals (55):** closing a dirty tab has no prompt (same as ⌘W, out of scope); the mouse-down wrapper at `root.rs:301` needs a one-line comment so it is not removed as redundant; `gpui::MouseButton::Left` is written as a full path.
**Residual (52):** the `.id("…-editor")` calls in `path.rs`, `auth.rs` and `settings.rs` are overwritten by `Scrollable` and do nothing.
**Residuals (54):** a `,` or `)` in a variable name can make the autosend `problems=` list ambiguous (nothing parses past the prefix); if autosend ever runs after an environment is selected, an unresolved `{{x}}` inside a secret's value would print `x`.
**Residuals (56):** the `install_crypto_provider` unit test does not exercise the Err branch.
**Residuals (59):** a true swap (one var moving each way in the same save) cannot be made safe with two writes and is accepted; `gains_moved_name` treats an unparseable other file as empty (harmless while `render` parses first); the files are re-read after `render`; a dangling non-.toml symlink logs a skip warning.
**Residuals (58):** rejecting a symlink diamond takes about 7-10 s before the error; a Windows trailing-dot or trailing-space `.toml` alias passes (`request.rs:221`); the name check at `request.rs:224-226` is unreachable.

### Pane-scrollbar defects (tasks 46-49, fixed and hand-checked 2026-11-02)
All four defects found hand-checking 43-45 are fixed (see HISTORY). Combined binary 19,014,912 B (+0 B).
**Residuals:** many inherited headers can squeeze the request-header table (the inherited list keeps natural height per spec). (Path, Auth, Settings, Multipart and File sub-tabs now scroll; task 52.) Do not add `.flex()` to the `div().w_full()` wrapper around KvTable virtual rows (46).
**Header limit (47, resolved by 50):** the limit is now 1,000 headers; see the Task 50 residuals below.

### Reqwest 0.13 follow-ups (task 50)
Optional, none blocking. Closed by 51: the runtime TLS test (`crates/core/tests/tls.rs`) and the `webpki-root-certs` `default-features = false` consistency fix. The `engine.rs:87`, `install_crypto_provider` and `https://localhost` residuals were closed by task 56 (see HISTORY).
Budgets after 50: binary 19,114,864 B of 20,971,520; idle RSS 97.2 MiB; after one request 102.3 MiB.

### Open budget and measurement questions (from M0)
**Per-platform budgets:** the user has not set budgets for Windows, Linux or macOS x64. Windows (24.9 MB) and Linux (35.1 MB) release binaries exceed the 20 MB figure, and Linux software-rendered RSS (about 171 MB idle) exceeds 120 MB. CI renderers are software or unknown and not comparable with the hardware figure; the set budgets still apply to macOS arm64 hardware only. Per-platform budgets are the user's call.
**x86_64-apple-darwin RSS:** unmeasured on CI because the main thread blocks in the headless runner's icon service (see HISTORY). Binary size only.
**09 review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s catch path does not print the stderr tail to the job log nor say whether stderr was empty; a set `CDPATH` could break `measure.sh`'s path resolution.
**06 (resolved):** the method and environment selects now show chevrons (task 37, hand-checked 2026-11-02). UI automation needs the window frontmost.

### Carry-forward: crates/core
**Secrets in Debug (10):** `Request`, `FolderDefaults` and `CollectionDefaults` (through `Auth`: Basic.password, Bearer.token, ApiKey.value) and `ClientSettings.proxy` (the URL may contain user:pass@) derive Debug in plain text. Never log them with `?` or `{:?}`. Only `Environment` has a redacting Debug. The rule is in `docs/conventions.md#logging`.
**Secrets in warnings (11):** `ResolveWarning::PatternMismatch.value` can hold a resolved secret, so history and logging must never store warnings.
**resolve nits (11):** a verbose-mode regex `(?x)...#c` can be misreported as `InvalidPattern`; problem dedupe keys on the Debug string; a test helper has a leftover `eprintln`; a scheme-less URL's `://` search can land in the query (the engine rejects such URLs anyway).
**Template helpers (22):** no resolve-level test pins `apply_path_params` to `path_range_plain` (an unclosed `{{` in the authority). `query_start_is_linear` asserts wall-clock < 100 ms and could flake on loaded CI. `expand` allocates all var refs before the budget check (linear, not unbounded). `lookup_var("")` is Unresolved while `resolve` reports Syntax for `{{ }}`. `query_start` can return a `?` inside a fragment.
**Types test (10, optional P3):** the File/Multipart unsupported test checks only the error kind, not the message.
**Body store (12):** the uid-ownership check on the spill directory has no test (it needs root). Three body tests spill into the real `<temp_dir>/wisp-<uid>` rather than a test base directory. The non-unix spill-directory code is not compiled or tested on this Mac.
**Engine blocking I/O (16):** spill-file writes in `BodyWriter::write` block the async worker. `File` and `Multipart` bodies are read fully into memory before sending; stream them later.
**Client cache (16):** `ClientCache` builds a client while holding a std `Mutex`. It is tested only through `ClientCache::get`, not `Engine::submit`.
**Engine tests and limits (16):** the multipart test leaves a temp file behind if an assertion fails. The app's `BODY_LIMIT` is now 64 KiB (first 64 KiB read from `bodies()`).

### Carry-forward: crates/app
**Escape (21):** Escape is bound with context None to `CancelRequest`. Hand-checked in task 41: Escape inside the focused URL input cancels a send (status "Cancelled").
**measure.sh (21):** the `WISP_EVENT` line was not separately confirmed in the output; status "measured" implies it.
**KvTable (29):** hand-checked in task 35 (typing in Params updates rows and the URL; Tab goes name, value, next name). Accepted residual: above the 50-row virtualisation threshold Tab cannot reach rows outside the viewport, because unpainted rows have no focus handle; scroll-to-focus is deferred. The table only virtualises if its container gives it a definite height (task 31). Optional: `locate` in `crates/app/src/ui/kv.rs` is O(n) per keystroke.
**Folder order (39, optional):** a list naming a request in both forms (`["a","a.toml"]`) has only its first match edited by rename, move and delete. `a.toml` is ambiguous when both `a.toml` and `a.toml.toml` exist. Hand-checked in task 41: rename and delete in the running app update `folder.toml` and the tree follows its order.
**Storage ops tests (23, optional):** `case_alias_sidecar_counts_as_shared` only exercises identity on delete and is vacuous on non-case-folding file systems. The rename and move tests should call `load_request` on the renamed and sharing requests instead of only checking text and `open_collection`.
**Palette and highlight (25, optional P3):** `palette.rs:136` lowercases only the first char of a label, so chars with a multi-char lowercase (such as "İ") never match. Write the hover " (uses a secret)" suffix format into the spec for task 31. Assert the full order in the `sen` ranking test.
**Formatting (26/36, optional P3):** `sniff_kind` strips the BOM after the UTF-8 check, so a sniff window cut inside a BOM reads as Text (not a real-world issue).
**Settings (27):** ToggleTheme and ToggleLayout (task 28) surface `settings::save` errors to the user, because save now returns Err on an unparsable or unreadable file instead of overwriting it. Optional P3: rename test `location_prefers_env_then_config_dir_then_none`, since its first case is the portable marker, not an env var.
**Draft and URL sync (24):** accepted residual: `Request.vars` has no `DirtySections` flag (vars change only through external edits in v1), so a draft can be dirty with no section highlighted. `merge_parsed_params` aligns old and parsed params with a 3-level order-preserving LCS (name+value, name, same slot); it is O(n*m) per keystroke, fine at real param counts (about 8 MB at 1000 params). Names containing `=` or `&`, and empty names, do not round-trip (documented on `display_url`). Optional cosmetic: group the test helpers `pairs` and `kv` at the top of the `url_sync` tests.
**Workspace shell (28):**
- *Measurement:* superseded by the task 35 figures under M2 (the shell measured about 96 MiB idle and binary 15.3 MB when it landed).
- *Hand-checked in task 35:* ⌘O from A to B (with dirty tabs: Discard, Open panel) closes A's tabs and resets the environment; ToggleTheme and ToggleLayout from the palette write only `theme` and `split` to settings.toml; theme="system" follows macOS Light/Dark live. The dev build wrote `~/Library/Application Support/wisp/settings.toml` during the check.
- *Mislabelled autosend error (fixed in task 54):* the failure line now reads `WISP_EVENT failed kind=Resolve problems=<names>`; see HISTORY.
- *Gaps:* (tab close button landed in task 55.) Routing, history and watcher wiring have no end-to-end tests because gpui test support is not enabled; the open-collection test covers only the `open_needs_confirm` helper.
- *Optional P3:* the `key` doc comment in `actions.rs` says "bound to `name`" but takes an index, and `bind_keys_indices_match_names` does not prove `key(i)` binds the right action.
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
- *Optional P3 (42):* the `jump_for_key` doc comment (`body.rs:64-65`) omits Ctrl and runs past the wrap; the `jump_still_valid` asserts sit inside the key test and could be split; when the 4 MB boundary falls mid-line the cap line follows a partial final line. Budgets measured: binary 18,998,304 B (+33,088), idle RSS 100,064 KiB, after request 104,512 KiB.
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
**History (15):** `preview_from` in `crates/storage/src/history.rs` decodes only the first 1024 bytes, so a 4-byte char straddling the limit becomes U+FFFD instead of being cut; decode up to `PREVIEW_LIMIT + 3` bytes, then cut on a char boundary (P3). Concurrent writers could race rotation of `history.jsonl`; v1 has one writer.
**Request files (17):** Method parsers unified in task 58 and `.toml` sidecar names are rejected. A sidecar may name a sibling request file in the same folder, and save will overwrite it. Hard links to the request file pass the identity check (load shows the TOML as the body; save is harmless; git does not store hard links). Windows `::$DATA` aliases are not covered. The drive-prefix check rejects any name with `:` as its second byte. Sidecar symlinks have a check-then-use race. `bare_relative_request_path_allows_inside_symlink` uses the process-global `set_current_dir`; safe today because all other tests use absolute paths, but a future relative-path test could flake.
**Collection tree (18, walk fixed in 58):** the cycle rule is now ancestor-only with a 10,000-folder whole-walk budget; directory symlinks pointing outside the root are followed; a broken `folder.toml` fails the whole open; `scope_for` with a non-canonical absolute root returns no folders (callers pass relative paths).
**Environments (19):** names with spaces are now rejected, so an existing environment with a space in its name will not load; `COM0` and `LPT0` are accepted; on case-insensitive filesystems `Dev` and `dev` map to the same file. A symlinked `environments/dev.toml` loads but is not listed, because `file_type` does not follow links; one `file_type` error aborts the whole listing. The plain file is written before the secrets file, so a save that half-fails can lose a var that is moving between them.
**Integration test (20, accepted):** the JSON body fixture has no comment because JSON cannot hold one.

## Next

## Someday
