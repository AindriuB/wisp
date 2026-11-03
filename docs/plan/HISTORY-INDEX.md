# History index

One row per `## YYYY-MM-DD` entry in `HISTORY.md`, newest first. Only `scribe`
edits this file, and it writes the row in the same commit as the entry.

`HISTORY.md` grows without bound and is the single largest file a session can
accidentally load. This index exists so `planner` can answer "has this been
built before, and where do I read about it?" without opening it. Rows are taken
from `HISTORY.md`'s own headings rather than re-summarized, so the index can be
checked against its source by eye.

Read a full entry by grepping the exact string in the **Heading** column:

```
grep -n '<heading text>' docs/plan/HISTORY.md
```

Then read that section only. A stale index is worse than none — an entry with
no row is one `planner` cannot find, and will re-plan.

| Date | Task IDs | Summary | Heading (grep this exact string) |
|---|---|---|---|
| 2026-11-03 | 69 | Dead scroll ids removed, close-button and key comments fixed, settings test renamed, sen ranking asserts full order, stray eprintln removed | ## 2026-11-03 — Small app and core nits cleared |
| 2026-11-03 | 66 | Failed request shows full wrapped, scrollable error with Copy in the Body area; Done line no longer reads `· · size`; +16,528 B binary | ## 2026-11-03 — A failed request shows its full error in the Body area |
| 2026-11-03 | 65 | Per-platform budgets in budgets.json enforced by measure.sh/ps1 (exit 3) and measure.yml; only macOS x64 tolerated; both Linux backends kept | ## 2026-11-03 — Per-platform budgets enforced by the measure scripts and measure.yml |
| 2026-11-03 | 64 | Release profile panic=abort, deps opt-level z, Linux gc-sections/ICF: binaries -38% to -54%; macOS x64 job still fails | ## 2026-11-03 — Release binaries shrunk 38-54% with panic=abort, size-optimised dependencies and Linux link flags |
| 2026-11-03 | 63 | measure.sh/ps1 add PSS, RssAnon, Windows private working set and renderer device; current-main baseline recorded for all four targets | ## 2026-11-03 — Truthful per-platform measurement: PSS, RssAnon, private working set, renderer device |
| 2026-11-03 | 62 | Storage ops reject reserved names before any filesystem lookup (fold_case); Linux CI green for the first time since task 23 | ## 2026-11-03 — Linux CI green: reserved names rejected before any filesystem lookup |
| 2026-11-03 | 61 | Window minimum 720×480; URL row and panes clip; Done status line truncates with full tooltip; 369 tests, binary 19,181,072 B | ## 2026-11-03 — Minimum window size; URL row and Done status line fit narrow panes |
| 2026-11-03 | 52-60 | M3 v1 hardening closed: 367 tests pass; binary 19,181,072 B, idle RSS 68.8 MiB, after request 73.9 MiB; narrow-window defect found (task 61) | ## 2026-11-03 — M3 v1 hardening closed: integration check and macOS arm64 budgets |
| 2026-11-03 | 57 | File and Multipart bodies over 1 MiB stream from disk in 64 KiB chunks with exact Content-Length; spill writes use block_in_place | ## 2026-11-03 — Large File and Multipart bodies stream from disk; spill writes run off the async worker |
| 2026-11-03 | 59 | History previews cut on a char boundary; list_environments follows symlinks; save_environment writes the file gaining a moved var name first | ## 2026-11-03 — History previews on char boundaries, linked environments, safe environment save order |
| 2026-11-02 | 53 | Cookies columns get minimum widths (584 px table, Flags 256 px) and scroll sideways below that; failed-status message truncates with tooltip | ## 2026-11-02 — Cookies table and failed-status line fit narrow panes |
| 2026-11-02 | 52 | Path, Auth, Settings, Multipart and File sub-tabs scroll with a scrollbar | ## 2026-11-02 — Request sub-tabs scroll |
| 2026-11-02 | 54, 56 | Autosend failure line reports `kind=Resolve problems=<names>` (names only, escaped); dead root counter dropped, provider Err logged, every bundled root checked against a RootCertStore, TLS tests use 127.0.0.1 | ## 2026-11-02 — Autosend resolve error and engine roots/TLS cleanup |
| 2026-11-02 | 55, 58 | Close button on each request tab; one case-insensitive method parser, folder walk bounded by ancestor cycle rule plus 10,000-folder budget, `.toml` sidecars rejected | ## 2026-11-02 — Tab close button and storage hardening (method parser, folder walk, sidecars) |
| 2026-11-02 | 51 | Three runtime TLS tests through the Engine against a self-signed rustls server (UnknownIssuer, verify_tls false round-trip, 1,000 headers); no new TLS stack | ## 2026-11-02 — Runtime TLS tests against a self-signed local server |
| 2026-11-02 | 50 | reqwest 0.13.5 (rustls ring, bundled webpki-root-certs); HTTP/1 response-header limit raised to 1000; binary +99,952 B | ## 2026-11-02 — reqwest 0.13.5 and a 1,000-header response limit |
| 2026-11-02 | 46, 47, 48, 49 | Fixed KvTable virtual-row widths, clear error for >100 response headers (limit stays 100; reqwest bump open), URL strip clip, Cookies table fit | ## 2026-11-02 — Pane-scrollbar defects fixed: KvTable virtual rows, header limit, URL strip, Cookies width |
| 2026-11-02 | 43, 44, 45 | Draggable scrollbars on sidebar tree, response Headers/Cookies and KvTable; request sub-tabs bounded; virtual KvTable branch now renders (columns zero-width, open) | ## 2026-11-02 — Scrollbars on the sidebar, response Headers and Cookies, and KvTable |
| 2026-11-02 | 42 | Body view scrollbar plus End/Home (and ⌘/Ctrl ↑↓) jump keys that load to the 4 MB cap; cap line hand-verified, closes the Raw-scrollbar P3 | ## 2026-11-02 — Raw and Pretty scrollbar with End/Home jump keys |
| 2026-11-02 | 41 | Hand check of M2 gaps passed (folder order, focus reclaim, URL resync, Raw prefetch, Pretty tooltip, Save sha256, Escape cancel, multipart file); 4 MB Raw cap line unreached, open P3 | ## 2026-11-02 — M2 verification gaps hand-checked |
| 2026-11-02 | 38, 40 | Root reclaims focus on the next frame after an unmount (guarded for overlays and prompts); URL bar resyncs from the draft while focused | ## 2026-11-02 — Root focus reclaim and URL resync while focused |
| 2026-11-02 | 39 | Folder `order` entries written as `stem.toml` now match requests in storage scan and rename/move/delete; window hand check deferred to task 41 | ## 2026-11-02 — Folder order honoured and maintained |
| 2026-11-02 | 35 | Scaffold dead_code allows removed; M2 budgets measured (binary 18.97 MB, idle 97.5 MiB, after request 102.3 MiB); window hand check passed with three defects filed (focus loss, folder order, Params checkbox) | ## 2026-11-02 — M2 integration and budgets |
| 2026-11-02 | 37 | 19 embedded icons (gpui-kit-assets, Apache-2.0) with a walk test for missing IconNames; select chevrons fixed; binary 18.98 MB, headroom ~2 MB | ## 2026-11-02 — Embedded icon assets |
| 2026-11-02 | 33 | Response pane: status line, Body Pretty/Raw (background pretty, paged virtualised Raw with cap), streaming Save, selectable headers with ⌘C, cookies table; window hand check deferred to task 35 | ## 2026-11-02 — Response viewer: status line, Pretty/Raw body, headers and cookies |
| 2026-11-02 | 34 | ⌘K palette (quick-open and commands) and ⌘E environment switcher over one overlay view; key-binding-before-listener lesson; window hand check deferred to task 35 | ## 2026-11-02 — Command palette and environment switcher |
| 2026-11-02 | 32 | Auth editor (none/inherit/basic/bearer/apikey, masked secrets, inherited line) and Body editor (JSON/text/XML/form/multipart/file); set_value-no-event lesson; window hand check deferred to task 35 | ## 2026-11-02 — Auth and body sub-tab editors |
| 2026-11-02 | 31 | URL bar with method select, variable highlight strip and hover, resolve problems gating Send/Cancel; Params, Path, Headers, Settings sub-tabs with dirty dots; window hand check deferred to task 35 | ## 2026-11-02 — URL bar and request sub-tabs |
| 2026-11-02 | 30 | Sidebar env select, collapsible ordered tree, context-menu create/rename/move/delete via storage ops; window hand check deferred to task 35 | ## 2026-11-02 — Sidebar tree, environment switcher and file operations |
| 2026-10-30 | 28 | Workspace, RequestTab and root layout replace view.rs; event routing, save, history, watcher reload; idle ~96 MiB, binary 15.3 MB | ## 2026-10-30 — Workspace, request tabs and root layout replace the skeleton view |
| 2026-10-30 | 36 | `pretty_json` output budget raised to max(32 MiB, 4x input); nesting tests assert exact outcomes; `SPACES` tied to indent cap | ## 2026-10-30 — Pretty-print budget raised to 32 MiB |
| 2026-10-30 | 24 | Per-section draft dirty diff and disk-change decisions; URL and params table kept in sync via order-preserving 3-level LCS merge | ## 2026-10-30 — Draft dirty tracking and URL/params sync |
| 2026-10-30 | 23, 25, 26, 27 | Storage create/rename/move/delete ops with sidecar and order handling; highlight spans and palette ranking; response pretty-print, paging, cookies; portable settings and themes | ## 2026-10-30 — M2 storage ops, highlighting, formatting and settings |
| 2026-10-30 | 29 | `KvTable` key-value editor: checkbox, masked values, trailing empty row, one Changed per edit, silent `set_rows`, annotations, virtual list over ~50 rows; hand check deferred to task 31 | ## 2026-10-30 — Reusable key-value table editor |
| 2026-10-30 | 21 | All M2 app modules as compiling stubs with frozen types, signatures and action names; Request/ResponsePane mount sub-editors; view.rs still live; toml_edit edge on wisp-app | ## 2026-10-30 — M2 app module layout frozen as compiling stubs |
| 2026-10-30 | 22 | Public `wisp_core::template` (`var_refs`, `path_params`, `path_range`, `query_start`; linear, skip `{{...}}`), resolve uses them, `lookup_var` with redacting Debug | ## 2026-10-30 — Public template helpers and lookup_var in core |
| 2026-10-30 | 10-20 | M1 summary: core and storage delivered and tested without a window; user amendments; wave-2 disk-full stall and shared-target-dir stale-binary mistake | ## 2026-10-30 — M1 complete: core and storage without a window |
| 2026-10-30 | 20 | Fixture `my-api` collection and end-to-end disk → resolve test, byte-for-byte round-trip, watcher external-edit test; secrets fixture force-added | ## 2026-10-30 — Storage-to-resolve integration test and fixture collection |
| 2026-10-30 | 17 | Storage `load_request`/`save_request`: sidecar bodies, in-place `toml_edit` via the codec, default-auth skip, settings defaults kept; sidecar name and identity rules enforced before any read or write | ## 2026-10-30 — Request file storage with sidecar bodies |
| 2026-10-30 | 18 | Storage `open_collection` tree (`CollectionTree`/`Node`/`RequestMeta`, `Node::Invalid`), collection/folder defaults load/save, `scope_for`, symlink cycles skipped | ## 2026-10-30 — Collection tree and folder/collection defaults storage |
| 2026-10-30 | 11 | Pure `resolve()`: precedence, dynamic vars, path params, query, header merge, auth inheritance, bodies, settings; bounded expansion (depth 8, 64 KiB, 10,000 budget) | ## 2026-10-30 — resolve(): variables, path params, inheritance and bounded expansion |
| 2026-10-30 | 19 | Storage environments: list/load/save, comment-preserving edits, `*.secrets.toml` written 0600 via `atomic_write_private` and git-ignored, strict name allowlist, name-only logging | ## 2026-10-30 — Environment and secrets file storage |
| 2026-10-30 | 16 | Engine emits Started/Headers/Progress(throttled)/Completed/Failed/Cancelled, streams bodies into `BodyStore`, real cancel, File and Multipart bodies sent, one cached client per `ClientSettings`; app reads first 64 KiB and shows body errors | ## 2026-10-30 — Engine full event set, real cancel and per-settings clients |
| 2026-10-30 | 14 | Storage `Watcher::start`: 100 ms per-path coalescing, time-ordered `Changed` events, state/temp files and directories ignored (known dirs scanned at start and on move-in, never walked through symlinks), notify errors logged, channel closes on drop | ## 2026-10-30 — Collection file watcher |
| 2026-10-30 | 13 | Storage TOML codec (vars, key-value arrays by LCS diff, auth; comments preserved) and atomic fs writes (create_new temp, mode kept, symlinks followed, `atomic_write_private`) | ## 2026-10-30 — Storage TOML codec and atomic file writes |
| 2026-10-30 | 12 | Body store (`BodyStore`/`BodyWriter`/`BodyHandle`): memory up to the limit, then private per-user spill dir with 0600 files; range reads, release, cleanup on drop; failed writes poison the writer | ## 2026-10-30 — Response body store with spill to disk |
| 2026-10-30 | 15 | Append-only history log (`.wisp/history.jsonl`, 4 MiB rotation, torn-line recovery, bounded preview); stores no secrets, headers or error messages | ## 2026-10-30 — Append-only request history log |
| 2026-10-30 | 10 | M1 core types frozen (authoring model, PreparedRequest, resolve and storage error types), all M1 deps added, scaffold modules; secret-bearing types print Debug in plain text | ## 2026-10-30 — M1 core types frozen, dependencies added, modules scaffolded |
| 2026-10-29 | 09 | measure.sh/ps1 stderr capture and early failure reasons, Linux ICD fix, app exits on window failure; first CI measurements (Linux, Windows, macOS arm64; x64 macOS RSS blocked by icon service) | ## 2026-10-29 — Measure diagnostics and first CI measurements |
| 2026-10-29 | 08 | Budgets recorded (macOS arm64: 20 MB binary, 120 MB idle, 128 MB after request); GPUI kept; other platforms unmeasured | ## 2026-10-29 — M0 budgets recorded and GPUI kept |
| 2026-10-29 | 07 | measure.sh/measure.ps1 and dispatch-only measure.yml; macOS arm64 measured, x64 binary size only; ps1 and CI platforms unverified | ## 2026-10-29 — RSS and binary-size measurement scripts and macOS results |
| 2026-10-29 | 06 | GPUI window (method dropdown, URL, Send, status line, body) wired to Engine; WISP_AUTOSEND_URL measurement hook | ## 2026-10-29 — GPUI walking-skeleton window wired to the Engine |
| 2026-10-29 | 04, 05 | Minimal core Engine over reqwest + rustls with Send/Cancel and Completed/Failed events; CI and four-target release workflows (Linux/Windows jobs unverified) | ## 2026-10-29 — Minimal core Engine and CI/release workflows |
| 2026-10-29 | 02, 03 | Cargo workspace with exact crates.io GPUI pins (gpui-component 0.8.1, gpui-pre and gpui-pre-platform 0.3.10); CLAUDE.md trimmed under 50 lines | ## 2026-10-29 — Cargo workspace scaffold and trimmed CLAUDE.md |
