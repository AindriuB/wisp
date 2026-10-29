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
**Blocked by:** M1 on M0; M2 on M1 (M2 may start once the M1 API is frozen).

## Next

## Someday
