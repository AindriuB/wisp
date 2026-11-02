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
- **M2 — UI v1 (in progress):** layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings. Tasks 21-35 in `docs/plan/tasks/`. 21 (module layout freeze), 22 (template helpers), 23 (storage tree operations), 25 (highlight and palette ranking), 26 (response formatting, paging, cookies), 27 (settings and themes) and 28 (workspace, tabs and root layout), 29 (key-value table editor), 24 (draft dirty tracking and URL/params sync), 36 (pretty-print budget) 30 (sidebar tree and environment switcher) 31 (URL bar and request sub-tabs) 32 (auth and body editors) and 34 (palette and environment switcher) landed.
**Next:** 33 is in flight in its own worktree. Task 35 must re-measure memory budgets: pretty-printing can now hold up to about 32 MiB of transient output, and the v1 shell already costs far more than the skeleton (see Workspace shell (28) under Carry-forward: crates/app).
**Housekeeping:** the repo moved from `/Users/Andrew/HttpClient` to `/Users/Andrew/workspace/wisp` on 2026-11-02. Open task files 31-35 still say `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-NN`; use `/Users/Andrew/workspace/wisp/target/wt-<id>` instead.

### Open budget and measurement questions (from M0)
**Per-platform budgets:** the user has not set budgets for Windows, Linux or macOS x64. Windows (24.9 MB) and Linux (35.1 MB) release binaries exceed the 20 MB figure, and Linux software-rendered RSS (about 171 MB idle) exceeds 120 MB. CI renderers are software or unknown and not comparable with the hardware figure; the set budgets still apply to macOS arm64 hardware only. Per-platform budgets are the user's call.
**x86_64-apple-darwin RSS:** unmeasured on CI because the main thread blocks in the headless runner's icon service (see HISTORY). Binary size only.
**09 review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s catch path does not print the stderr tail to the job log nor say whether stderr was empty; a set `CDPATH` could break `measure.sh`'s path resolution.
**06:** the method dropdown has no chevron; task 37 (embedded icon assets) delivers it and task 35's hand check confirms it. UI automation needs the window frontmost.

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
**Escape (21):** Escape is bound with context None to `CancelRequest`, so Escape inside the URL input may not cancel a send. Not exercised in the task 28 hand check; check at task 35.
**measure.sh (21):** the `WISP_EVENT` line was not separately confirmed in the output; status "measured" implies it.
**KvTable (29):** the hand check (typing in Params adds rows and updates the URL bar; Tab goes name, value, next name in a real window) is folded into task 35's hand check now that task 31 has mounted the table. Accepted residual: above the 50-row virtualisation threshold Tab cannot reach rows outside the viewport, because unpainted rows have no focus handle; scroll-to-focus is deferred. The table only virtualises if its container gives it a definite height (task 31). Optional: `locate` in `crates/app/src/ui/kv.rs` is O(n) per keystroke.
**Storage ops tests (23, optional):** `case_alias_sidecar_counts_as_shared` only exercises identity on delete and is vacuous on non-case-folding file systems. The rename and move tests should call `load_request` on the renamed and sharing requests instead of only checking text and `open_collection`.
**Palette and highlight (25, optional P3):** `palette.rs:136` lowercases only the first char of a label, so chars with a multi-char lowercase (such as "İ") never match. Write the hover " (uses a secret)" suffix format into the spec for task 31. Assert the full order in the `sen` ranking test.
**Formatting (26/36, optional P3):** `sniff_kind` strips the BOM after the UTF-8 check, so a sniff window cut inside a BOM reads as Text (not a real-world issue).
**Settings (27):** ToggleTheme and ToggleLayout (task 28) surface `settings::save` errors to the user, because save now returns Err on an unparsable or unreadable file instead of overwriting it. Optional P3: rename test `location_prefers_env_then_config_dir_then_none`, since its first case is the portable marker, not an env var.
**Draft and URL sync (24):** accepted residual: `Request.vars` has no `DirtySections` flag (vars change only through external edits in v1), so a draft can be dirty with no section highlighted. `merge_parsed_params` aligns old and parsed params with a 3-level order-preserving LCS (name+value, name, same slot); it is O(n*m) per keystroke, fine at real param counts (about 8 MB at 1000 params). Names containing `=` or `&`, and empty names, do not round-trip (documented on `display_url`). Optional cosmetic: group the test helpers `pairs` and `kv` at the top of the `url_sync` tests.
**Workspace shell (28):**
- *Measurement:* with the v1 shell, macOS arm64 hardware measures about 96 MiB idle (98,336 KiB) and 98 MiB after a request (100,256 KiB), binary 15,272,432 B. Idle was about 68 MiB before the shell; headroom against 120/128 MB is about 24/30 MB. Task 35 re-measures.
- *Deferred hand checks (user decision), for task 35:* (1) ⌘O from collection A to B closes A's tabs (the picker could not be driven in the background); (2) ToggleTheme cycles system/light/dark and writes only `theme`, and ToggleLayout flips the split axis; neither has a key binding, so both are reachable only through the ⌘K palette (task 34, now landed, so both can be checked); (3) theme="system" follows macOS Light/Dark live, theme="dark" does not. Passed in task 28: layout, ⌘T/⌘W, both dividers, and the dirty-tab "Discard unsaved changes?" prompt on ⌘O.
- *P3, mislabelled error:* with a collection open, autosend of a plain URL reports `WISP_EVENT failed kind=InvalidUrl` because the scratch tab inherits the collection's bearer auth `{{token}}`, which is unresolved without an environment. The fixed "InvalidUrl" label mislabels a resolve error; report the real resolve error kind.
- *Gaps:* tabs have no close button (⌘W only). Routing, history and watcher wiring have no end-to-end tests because gpui test support is not enabled; the open-collection test covers only the `open_needs_confirm` helper.
- *Optional P3:* the `key` doc comment in `actions.rs` says "bound to `name`" but takes an index, and `bind_keys_indices_match_names` does not prove `key(i)` binds the right action.
**Sidebar (30):**
- *Deferred hand check, to task 35:* the sidebar was verified by tests and review only, not in a window. In a window on a temp copy of `my-api`: tree order and open/collapse, each context-menu operation (create, rename, move, delete) against the files on disk, environment switching, and that a failed switch restores the select.
- *Optional:* a node created inside a collapsed folder is selected but hidden, because the parent is not expanded. An environment literally named "No environment" clashes with the `NO_ENV` entry. The delete-prune `collapsed.retain` has no unit test (correct by inspection).
**URL bar and sub-tabs (31):**
- *Deferred hand check, to task 35 (with the KvTable check above):* typing in the URL and the Params table stay in live sync, highlight colours and hover tooltips (including a secret showing `••••`), resolve problems disable Send, Cancel while sending, and the sub-tab dirty dots. Verified by tests and review only so far.
- *Decision on resync:* the "only on DraftReplaced" rule holds for Headers and Settings. Params, the URL input (when unfocused) and Path also resync on `DraftEdited`, behind equality guards so an edit never echoes back.
- *Optional:* a non-numeric or zero timeout is silently ignored in Settings (`settings.rs:73-75`). The URL bar also refreshes on `CollectionChanged` (harmless). A URL that does not round-trip through `apply_url_text` (an enabled param with an empty name) can still produce one extra edit.
**Auth and body editors (32):**
- *Deferred hand check, to task 35:* edit a JSON body and ⌘S writes the sidecar; Bearer, then Inherit, then Bearer restores the token; a multipart File part chosen via Choose… sends; the inherited-auth line names the right folder. Verified by tests (297) and review only so far.
- *Lesson for later UI work:* gpui-component 0.8.1 `InputState::set_value` emits no Change event, so any programmatic set must update the model or draft by hand (the cause of the round-1 P1/P2).
- *Optional:* `path.exists()` stats the filesystem on every render of the File body; a hand-typed relative File path is checked against the process cwd rather than the collection dir, so "File not found" can be wrong; a Choose… pick into a row deleted while the dialog is open is silently dropped.
**Palette and environment switcher (34):**
- *Deferred hand check, to task 35 (verified by tests and review only so far):* ⌘K `gu` opens a request; ⌘K `theme` toggles the theme; ⌘E lists No environment and dev, and choosing dev makes the hover values follow; Escape closes the palette without cancelling a request; shortcuts keep working after the palette closes (including after ⌘K then ⌘E while open, and after CloseTab from the palette); an outside click dismisses; the active environment is pre-selected. This also makes task 28's deferred ToggleTheme and ToggleLayout checks reachable.
- *Lesson:* in gpui-pre 0.3.10 key bindings dispatch before key-down listeners, and a context-less binding (such as escape to CancelRequest) beats component-context ones. Overlays must handle such actions with `on_action` on their own div, and an overlay that takes focus must restore it on dismiss.
- *Optional:* with more than 50 environments the active one past the limit is not pre-selected. ⌘K or ⌘E while the palette is open reloads it in place and clears the query rather than toggling it closed.
**Stub allows (21):** remaining stub `mod`s carry `#[allow(dead_code)]`; task 35 removes them. `mod format` in `main.rs` has one too; task 33 or 35 removes it when the response pane uses it.

### Carry-forward: crates/storage
**Codec (13):** `write_vars` never reorders existing keys. Flipping an explicit `enabled = true` to false may drop a trailing comment on that line. New entries in multi-line inline arrays get `toml_edit` default spacing. The Windows readonly-target save path in `atomic_write` is untested. `write_auth` keeps its signature, so callers skip the call when the value equals that file's default and the file had no `[auth]`; use `atomic_write_private` for files that hold secrets.
**Watcher (14):** nested directory renames within the same 100 ms window may emit one spurious `Changed` for a directory-derived path (accepted); the M2 reload must re-check the disk and tolerate `Changed` for non-file paths. On Linux notify follows symlinks into existing linked directories by default; consider `follow_symlinks: false`. The event-time `p.is_dir()` follows symlinks while `skip_directory` does not (harmless).
**History (15):** `preview_from` in `crates/storage/src/history.rs` decodes only the first 1024 bytes, so a 4-byte char straddling the limit becomes U+FFFD instead of being cut; decode up to `PREVIEW_LIMIT + 3` bytes, then cut on a char boundary (P3). Concurrent writers could race rotation of `history.jsonl`; v1 has one writer.
**Request files (17):** `request.rs` has its own method parser alongside `collection.rs`'s private `parse_method`; unify them. A sidecar may name a sibling request file in the same folder, and save will overwrite it. Hard links to the request file pass the identity check (load shows the TOML as the body; save is harmless; git does not store hard links). Windows `::$DATA` aliases are not covered. The drive-prefix check rejects any name with `:` as its second byte. Sidecar symlinks have a check-then-use race. `bare_relative_request_path_allows_inside_symlink` uses the process-global `set_current_dir`; safe today because all other tests use absolute paths, but a future relative-path test could flake.
**Collection tree (18):** the visited set spans the whole walk, so a symlink to a sibling folder shows under whichever name `read_dir` returns first (push/pop ancestors instead); directory symlinks pointing outside the root are followed; a broken `folder.toml` fails the whole open; `scope_for` with a non-canonical absolute root returns no folders (callers pass relative paths).
**Environments (19):** names with spaces are now rejected, so an existing environment with a space in its name will not load; `COM0` and `LPT0` are accepted; on case-insensitive filesystems `Dev` and `dev` map to the same file. A symlinked `environments/dev.toml` loads but is not listed, because `file_type` does not follow links; one `file_type` error aborts the whole listing. The plain file is written before the secrets file, so a save that half-fails can lose a var that is moving between them.
**Integration test (20, accepted):** the JSON body fixture has no comment because JSON cannot hold one.

## Next

## Someday
