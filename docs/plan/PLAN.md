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
- **M0 — Walking skeleton:** workspace, minimal GPUI window, minimal Engine over reqwest + rustls, CI release builds on four targets, and measured RSS and binary size that set the budgets in `docs/architecture.md`; exit decides whether GPUI stays.
- **M1 — Core and storage:** full model, `resolve`, body store, full event set and cancel, `toml_edit` storage with secrets and watcher, unit-tested without a window.
- **M2 — UI v1:** layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings.
**Done in M0:** workspace scaffold with pinned GPUI deps (02); minimal core Engine (04); CI and release workflows (05); GPUI window with the `WISP_AUTOSEND_URL` hook (06); RSS and binary-size measurement scripts, dispatch-only `measure.yml` and macOS results (07). Remaining M0 task: 08 record budgets and the GPUI keep/revisit decision.
**Needs the user:** task 08 cannot finish alone. The budgets and the GPUI keep-or-revisit decision are the user's, made on the 07 numbers (aarch64 macOS: binary 16,660,480 B, idle ~100 MB, ~103 MB after one request, hardware renderer).
**Open from 07:** `measure.ps1` is UNVERIFIED (no pwsh locally). Windows, Linux and macOS x64 RSS need a `measure.yml` run, which needs a GitHub remote and a push; CI figures are software/unknown-renderer and not comparable with the local hardware figure. x86_64-apple-darwin has binary size only (RSS unmeasured under Rosetta). Review P3s, unfixed: `measure.sh` runs a bare binary name via PATH while `-f` checks the local file; both scripts wait the full 30 s and report "no WISP_EVENT completed" even when a `WISP_EVENT failed` line or early exit explains it; `measure.ps1` has no `catch`, so a throw (e.g. `python3` missing on windows-latest, where it may only be `python`) writes no JSON. The `measure.yml` comment "Windows first" is wrong (Windows is third in the matrix; it means first real run of `measure.ps1`).
**Open from 06:** the method dropdown has no chevron (cosmetic, M2). UI automation needs the window frontmost.
**Open from 05:** the Linux and Windows jobs and a green `release.yml` run are unverified; only the x86_64-apple-darwin release build was run, locally. Not closable until the repo is pushed and the workflows run.
**Blocked by:** M1 on M0; M2 on M1 (M2 may start once the M1 API is frozen).

## Next

## Someday
