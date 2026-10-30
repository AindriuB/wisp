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
- **M2 — UI v1 (in progress):** layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings. Tasks 21-35 in `docs/plan/tasks/`. 21 (module layout freeze), 22 (template helpers), 23 (storage tree operations), 25 (highlight and palette ranking), 26 (response formatting, paging, cookies), 27 (settings and themes) and 29 (key-value table editor) landed; 24 is in review.
**In flight:** 24 (in review)
**Next:** 28 is unblocked (depends on 21 and 23). Task 36 (follow-up, to be written): raise the pretty-print output budget in `crates/app/src/pretty.rs` from max(1 MiB, 4x input) to max(32 MiB, 4x input), by user decision on 2026-10-30. Fold in the optional 26 items below.

### Open budget and measurement questions (from M0)
**Per-platform budgets:** the user has not set budgets for Windows, Linux or macOS x64. Windows (24.9 MB) and Linux (35.1 MB) release binaries exceed the 20 MB figure, and Linux software-rendered RSS (about 171 MB idle) exceeds 120 MB. CI renderers are software or unknown and not comparable with the hardware figure; the set budgets still apply to macOS arm64 hardware only. Per-platform budgets are the user's call.
**x86_64-apple-darwin RSS:** unmeasured on CI because the main thread blocks in the headless runner's icon service (see HISTORY). Binary size only.
**09 review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s catch path does not print the stderr tail to the job log nor say whether stderr was empty; a set `CDPATH` could break `measure.sh`'s path resolution.
**06:** the method dropdown has no chevron (cosmetic, M2). UI automation needs the window frontmost.

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
**Escape (21):** Escape is bound with context None to `CancelRequest`, so Escape inside the URL input may not cancel a send. Check at the task 28/35 hand check.
**measure.sh (21):** the `WISP_EVENT` line was not separately confirmed in the output; status "measured" implies it.
**KvTable (29):** the hand check (typing in Params adds rows and updates the URL bar; Tab goes name, value, next name in a real window) is deferred until task 31 mounts the table. Accepted residual: above the 50-row virtualisation threshold Tab cannot reach rows outside the viewport, because unpainted rows have no focus handle; scroll-to-focus is deferred. The table only virtualises if its container gives it a definite height (task 31). Optional: `locate` in `crates/app/src/ui/kv.rs` is O(n) per keystroke.
**Storage ops tests (23, optional):** `case_alias_sidecar_counts_as_shared` only exercises identity on delete and is vacuous on non-case-folding file systems. The rename and move tests should call `load_request` on the renamed and sharing requests instead of only checking text and `open_collection`.
**Palette and highlight (25, optional P3):** `palette.rs:136` lowercases only the first char of a label, so chars with a multi-char lowercase (such as "İ") never match. Write the hover " (uses a secret)" suffix format into the spec for task 31. Assert the full order in the `sen` ranking test.
**Formatting (26, optional, fold into 36):** `deep_nesting_is_iterative` and `hostile_nesting` accept any `Err`; assert the budget error and an offset past the last opening bracket. Tie the `SPACES` length to `MAX_INDENT_LEVELS` with a const assert. `sniff_kind` does not skip a BOM.
**Settings (27):** task 28's ToggleTheme and ToggleLayout must surface `settings::save` errors to the user, because save now returns Err on an unparsable or unreadable file instead of overwriting it. Live OS-appearance following for theme="system", and its hand check, belong to task 28. Optional P3: rename test `location_prefers_env_then_config_dir_then_none`, since its first case is the portable marker, not an env var.
**Stub allows (21):** each stub `mod` carries `#[allow(dead_code)]` (mounted by 28, allow removed by 35).

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
