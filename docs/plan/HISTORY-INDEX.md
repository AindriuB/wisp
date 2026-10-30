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
