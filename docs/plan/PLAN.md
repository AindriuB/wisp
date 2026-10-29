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
**Done in M0:** workspace scaffold with pinned GPUI deps (02); minimal core Engine (04); CI and release workflows (05); GPUI window with the `WISP_AUTOSEND_URL` hook (06); measurement scripts, dispatch-only `measure.yml` and macOS results (07); budgets recorded in `docs/architecture.md` and GPUI kept (08); measure diagnostics and first CI measurements (09). M0 is complete. **M1 is next** and needs `/plan` to split it into tasks.
**Open from 09, per-platform budgets:** the user has not set budgets for Windows, Linux or macOS x64. Windows (24.9 MB) and Linux (35.1 MB) release binaries exceed the 20 MB figure, and Linux software-rendered RSS (about 171 MB idle) exceeds 120 MB. CI renderers are software or unknown and not comparable with the hardware figure; the set budgets still apply to macOS arm64 hardware only. Per-platform budgets are the user's call.
**Open from 09, x86_64-apple-darwin RSS:** unmeasured on CI because the main thread blocks in the headless runner's icon service (see HISTORY). Binary size only.
**Open from 09, review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s catch path does not print the stderr tail to the job log nor say whether stderr was empty; a set `CDPATH` could break `measure.sh`'s path resolution.
**Open from 07:** the `measure.yml` comment "Windows first" is wrong (Windows is third in the matrix; it means first real run of `measure.ps1`).
**Open from 06:** the method dropdown has no chevron (cosmetic, M2). UI automation needs the window frontmost.
**Blocked by:** M1 on M0; M2 on M1 (M2 may start once the M1 API is frozen).

## Next

## Someday
