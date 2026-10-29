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
| 2026-10-29 | 07 | measure.sh/measure.ps1 and dispatch-only measure.yml; macOS arm64 measured, x64 binary size only; ps1 and CI platforms unverified | ## 2026-10-29 — RSS and binary-size measurement scripts and macOS results |
| 2026-10-29 | 06 | GPUI window (method dropdown, URL, Send, status line, body) wired to Engine; WISP_AUTOSEND_URL measurement hook | ## 2026-10-29 — GPUI walking-skeleton window wired to the Engine |
| 2026-10-29 | 04, 05 | Minimal core Engine over reqwest + rustls with Send/Cancel and Completed/Failed events; CI and four-target release workflows (Linux/Windows jobs unverified) | ## 2026-10-29 — Minimal core Engine and CI/release workflows |
| 2026-10-29 | 02, 03 | Cargo workspace with exact crates.io GPUI pins (gpui-component 0.8.1, gpui-pre and gpui-pre-platform 0.3.10); CLAUDE.md trimmed under 50 lines | ## 2026-10-29 — Cargo workspace scaffold and trimmed CLAUDE.md |
