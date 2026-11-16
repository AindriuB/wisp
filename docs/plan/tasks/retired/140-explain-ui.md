# 140 — Show the Explain pane and the pre-send context strip

**Repo:** .
**Depends on:** 139
**Owns:**
- crates/app/src/ui/explain.rs (replaces the 139 stub)
- crates/app/src/ui/url_bar.rs (the context strip only)

## Goal
The response pane's Explain sub-tab shows where every variable, parameter,
header, auth setting, request setting and the body came from, before or after a
send, and a one-line strip under the URL row always shows the active
environment, the auth source and the history scope. Read-only; no secret value
is ever shown.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md Feature 1 "Behaviour" (pane sections, strip, display rules) and "Security and redaction"
- crates/core/src/resolve/report.rs:303 `ResolutionReport`, :318 `explain`, :385 `explain_auth`, :37-53 `ShownPart`/`Shown`, :72 `Origin` (folder by index)
- 139's accessors in crates/app/src/workspace.rs: `send_scope`, `history_scope` / `HistoryScope`; `OpenCollection.env_status` (:58)
- `HistoryScope` has a `NotRecorded` variant (tab owned by no collection), rendered as "scratch: not recorded", per 139's 2026-11-15 decision (139 AC6).
- crates/app/src/highlight.rs:119-137 200-char display cap and folder-name mapping; crates/app/src/ui/url_bar.rs:38 `folder_names`
- crates/app/src/theme.rs — theme tokens; use them, add no literal colours
- Worktree: `./.claude/scripts/wt-new.sh . 140 explain-ui`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-140`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 `ExplainPane` renders the report's sections (Variables with shadowed layers, URL and params, Headers with overrides and synthetic origins, Auth with application and field names, Settings with request/default, Body kind, length and variables used, Problems/Warnings) from `explain(&draft, &ws.send_scope(tab))`; it rebuilds only while the Explain sub-tab is visible, on draft edits, environment changes and secrets changes.
- [ ] AC2 Rendering goes through pure functions `fn part_text(&ShownPart, folders: &[String]) -> String` and `fn origin_text(&Origin, folders: &[String]) -> String`; tests cover `Secret{resolved}` → `secret NAME (resolved)`, unresolved → `secret NAME (not set)`, `Dynamic` → `$NAME, generated at send`, `Hidden` → `hidden: credential name`, text over 200 characters truncated, and `Folder(i)` mapped to its name.
- [ ] AC3 The strip is one line under the URL row: environment (`none`, or name plus `secrets loaded` / `secrets failed` / `keychain`), auth source from `explain_auth` (e.g. `Bearer, from folder users`), and history scope from `HistoryScope` (`records under <path>`, `scratch: recorded without a request link`, `off: <reason>`, and the original-missing and URL-edited cases). Built by a pure `fn strip_text(...) -> String`, table-tested per `HistoryScope` variant without GPUI. The strip never calls `explain` (only `explain_auth`).
- [ ] AC4 Canary: a test builds a report with an environment secret whose value is a unique marker used in URL, header and auth, renders every section's text through the AC2/AC3 functions, and asserts the marker is in none of the strings.
- [ ] AC5 Theme tokens only (no new hex or `rgb(` literal in the two files); full suite, fmt and clippy pass.

## Checks
- `explain-ui` command — AC2, AC3, AC4
  run: `cargo test -p wisp-app --bin wisp explain 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `strip` command — AC3
  run: `cargo test -p wisp-app --bin wisp url_bar 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-literal-colours` command — AC5
  run: `! git diff main...HEAD -- crates/app/src/ui/explain.rs crates/app/src/ui/url_bar.rs | rg '^\+.*(rgb\(|rgba\(|hsla\(|0x[0-9a-fA-F]{6})'`
- `pane-wiring` read — AC1
  how: the diff shows the rebuild gated on the Explain sub-tab being active and subscriptions to tab and workspace events; hand-checked in 149.

## Out of scope
- Any change to `wisp_core::resolve` or `report.rs` (a report defect is a follow-up, not fixed here).
- The guard badge in the strip (wave 3, 3A); hover unification onto the report (later).
- Any file 139 owns: `tab.rs`, `ui/response/mod.rs`, `ui/mod.rs`, `workspace.rs`.

## Outcome
Merged as PR #99 (afb9812, 2026-11-16) at 38d8d21. The Explain pane renders the resolution report through `part_text` and `origin_text`, and a one-line strip under the URL row shows environment, auth source and history scope through `strip_text`. The strip never calls `explain`, and a canary test asserts no secret marker reaches any rendered string. Visual hand testing is part of task 149.
