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
Build the portable GPUI HTTP client in three milestones, in this order. Spec: `docs/plan/specs/2026-10-29-http-client-v1.md`. Needs `/plan` to split into tasks. Product is Wisp; crate naming is settled in `docs/conventions.md#naming`.
- **M0 — Walking skeleton (complete):** workspace, minimal GPUI window, minimal Engine over reqwest + rustls, CI release builds on four targets, and measured RSS and binary size that set the budgets in `docs/architecture.md`; exit decides whether GPUI stays.
- **M1 — Core and storage:** full model, `resolve`, body store, full event set and cancel, `toml_edit` storage with secrets and watcher, unit-tested without a window.
- **M2 — UI v1:** layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings.
**Done in M0:** workspace scaffold with pinned GPUI deps (02); minimal core Engine (04); CI and release workflows (05); GPUI window with the `WISP_AUTOSEND_URL` hook (06); measurement scripts, dispatch-only `measure.yml` and macOS results (07); budgets recorded in `docs/architecture.md` and GPUI kept (08); measure diagnostics and first CI measurements (09). M0 is complete.
**M1 in progress:** tasks 10-20 are planned. 10 (frozen core types, M1 dependencies, scaffold modules), 11 (resolve), 12 (body store), 13 (storage codec and fs), 14 (watcher), 15 (history log), 16 (full engine events), 17 (request files), 18 (collection tree) and 19 (environment storage) have landed; only 20 (storage/resolve integration) remains.
**Open from 09, per-platform budgets:** the user has not set budgets for Windows, Linux or macOS x64. Windows (24.9 MB) and Linux (35.1 MB) release binaries exceed the 20 MB figure, and Linux software-rendered RSS (about 171 MB idle) exceeds 120 MB. CI renderers are software or unknown and not comparable with the hardware figure; the set budgets still apply to macOS arm64 hardware only. Per-platform budgets are the user's call.
**Open from 09, x86_64-apple-darwin RSS:** unmeasured on CI because the main thread blocks in the headless runner's icon service (see HISTORY). Binary size only.
**Open from 09, review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s catch path does not print the stderr tail to the job log nor say whether stderr was empty; a set `CDPATH` could break `measure.sh`'s path resolution.
**Open from 06:** the method dropdown has no chevron (cosmetic, M2). UI automation needs the window frontmost.
**Carry-forward from 10, secrets in Debug:** `Request`, `FolderDefaults` and `CollectionDefaults` (through `Auth`: Basic.password, Bearer.token, ApiKey.value) and `ClientSettings.proxy` (the URL may contain user:pass@) derive Debug in plain text. Never log them with `?` or `{:?}`. Only `Environment` has a redacting Debug. The rule is in `docs/conventions.md#logging`.
**Carry-forward from 10, review P3s (optional):** the File/Multipart unsupported test checks only the error kind, not the message. The module doc in `crates/core/src/body.rs` was a task 12 item and is now rewritten.
**Carry-forward from 15, review P3 (optional):** `preview_from` in `crates/storage/src/history.rs` decodes only the first 1024 bytes, so a 4-byte char straddling the limit becomes U+FFFD instead of being cut. Decode up to `PREVIEW_LIMIT + 3` bytes, then cut on a char boundary.
**Carry-forward from 15, single writer:** concurrent writers could race rotation of `history.jsonl`. v1 has one writer; revisit if that changes.
**Carry-forward from 11, resolve nits:** a verbose-mode regex `(?x)...#c` can be misreported as `InvalidPattern`; problem dedupe keys on the Debug string; a test helper has a leftover `eprintln`; a scheme-less URL's `://` search can land in the query (the engine rejects such URLs anyway).
**Carry-forward from 18, collection tree:** the visited set spans the whole walk, so a symlink to a sibling folder shows under whichever name `read_dir` returns first (push/pop ancestors instead); directory symlinks pointing outside the root are followed; a broken `folder.toml` fails the whole open; `scope_for` with a non-canonical absolute root returns no folders (callers pass relative paths).
**Carry-forward from 17, method parser:** `request.rs` has its own method parser alongside `collection.rs`'s private `parse_method`; unify them.
**Carry-forward from 17, sidecar limits:** a sidecar may name a sibling request file in the same folder, and save will overwrite it. Hard links to the request file pass the identity check (load shows the TOML as the body; save is harmless; git does not store hard links). Windows `::$DATA` aliases are not covered. The drive-prefix check rejects any name with `:` as its second byte. Sidecar symlinks have a check-then-use race.
**Carry-forward from 17, test flake risk:** `bare_relative_request_path_allows_inside_symlink` uses the process-global `set_current_dir`. That is safe today because all other tests use absolute paths; a future relative-path test could flake.
**Carry-forward from 11 review, secrets in warnings:** `ResolveWarning::PatternMismatch.value` can hold a resolved secret, so history and logging must never store warnings.
**Carry-forward from 12, body store test gaps:** the uid-ownership check on the spill directory has no test (it needs root). Three body tests spill into the real `<temp_dir>/wisp-<uid>` rather than a test base directory. The non-unix spill-directory code is not compiled or tested on this Mac.
**Carry-forward from 13, codec limits:** `write_vars` never reorders existing keys. Flipping an explicit `enabled = true` to false may drop a trailing comment on that line. New entries in multi-line inline arrays get `toml_edit` default spacing.
**Carry-forward from 13, untested:** the Windows readonly-target save path in `atomic_write` is untested.
**Carry-forward from 13, for tasks 17-19:** `write_auth` keeps its signature, so callers skip the call when the value equals that file's default and the file had no `[auth]`. Use `atomic_write_private` for files that hold secrets.
**Carry-forward from 14, accepted residual:** nested directory renames within the same 100 ms window may emit one spurious `Changed` for a directory-derived path. Consumers must re-check the disk, so task 20 and the M2 reload must tolerate `Changed` for non-file paths.
**Carry-forward from 14, symlinks on Linux:** notify follows symlinks into existing linked directories by default there. Consider `follow_symlinks: false` in the watcher config.
**Carry-forward from 14, harmless inconsistency:** the event-time `p.is_dir()` follows symlinks while `skip_directory` does not.
**Carry-forward from 16, blocking I/O:** spill-file writes in `BodyWriter::write` block the async worker. `File` and `Multipart` bodies are read fully into memory before sending; stream them later.
**Carry-forward from 16, client cache:** `ClientCache` builds a client while holding a std `Mutex`. It is tested only through `ClientCache::get`, not `Engine::submit`.
**Carry-forward from 16, tests and limits:** the multipart test leaves a temp file behind if an assertion fails. The app's `BODY_LIMIT` is now 64 KiB (first 64 KiB read from `bodies()`).
**Carry-forward from 19, names:** environment names with spaces are now rejected, so an existing environment with a space in its name will not load. `COM0` and `LPT0` are accepted. On case-insensitive filesystems `Dev` and `dev` map to the same file.
**Carry-forward from 19, listing:** a symlinked `environments/dev.toml` loads but is not listed, because `file_type` does not follow links. One `file_type` error aborts the whole listing.
**Carry-forward from 19, save order:** the plain file is written before the secrets file, so a save that half-fails can lose a var that is moving between them.
**Blocked by:** M1 on M0; M2 on M1 (M2 may start once the M1 API is frozen).

## Next

## Someday
