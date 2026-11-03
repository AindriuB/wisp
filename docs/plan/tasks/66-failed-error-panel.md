# 66 — Show the full error for a failed request in the Body area

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/ui/response/mod.rs
- crates/app/src/ui/response/body.rs
- crates/app/src/ui/response/error.rs (new, optional: only if the panel is split out of body.rs)
- docs/plan/tasks/66-failed-error-panel.md (the `## Notes` section only)

**Build:** run every cargo command with
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-66`.

## Goal
A failed request today shows one truncated status line ("Connection failed error
sending request for url (…): client erro…"); the full text is only in a hover
tooltip and the Body sub-tab says "No body". When the phase is Failed, the Body
area must show the whole error, wrapped and scrollable, with a Copy action. The
one-line status line stays as it is. The same file also carries the 61 residual
where a Done line with no recorded time reads `· · 1.2 KB`.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:163-186 — Section 4, response
  area: status line, then Body / Headers / Cookies. The panel goes inside the Body
  sub-tab, not as a new sub-tab or a banner.
- crates/app/src/ui/response/mod.rs:32-43 — `error_words(kind)`, the heading per
  `ErrorKind`. :96-175 — `status_line`; :145-170 is the Failed arm (its tooltip
  text is `format!("{words} {msg}")`). :116-143 is the Done arm; :125-128 builds
  `timing` as `"· {total} · {size}"`, which gives `· · 1.2 KB` when `r.total` is None.
  :208-215 `done_tooltip` and its test at :221-235 show the pure-helper pattern to follow.
- crates/app/src/ui/response/body.rs:623-712 — `BodyPane::render`; :689-690 is the
  `"No body"` branch the panel replaces when the phase is Failed. :64-66 — the
  `jump_for_key` doc comment (42 residual: it omits Ctrl and runs past the wrap).
- crates/app/src/tab.rs:44-70 — `Phase` and `ResponseState` (`phase`, `error:
  Option<EngineError>`). crates/core/src/engine.rs:619-662 — `classify`: `message`
  already holds the full reqwest/hyper source chain joined with `": "`, so no
  core change is needed.
- crates/app/src/ui/response/headers.rs:130-136 — the existing
  `cx.write_to_clipboard(ClipboardItem::new_string(..))` copy pattern.
- crates/app/src/ui/request/settings.rs:162 — `.id(..).size_full().overflow_y_scrollbar()`,
  the scroll pattern for a pane.
- crates/app/src/assets.rs:96-140 — the icon guards: every `IconName::X` written in
  app sources must be embedded. `IconName::Copy` and `IconName::Check` are already
  embedded; use only icons already in `crates/app/assets/icons/`.

## Acceptance
- [ ] A pure function (in mod.rs or error.rs) returns the full error text for an
      `Option<&EngineError>`: `"{error_words(kind)}: {message}"`, or just the words
      when the message is empty or the error is None (`"Request failed"`). The
      Failed status-line tooltip and the Copy action both use it. A unit test
      covers a Connect error with a multi-part message, an empty message and None.
- [ ] When `response.phase == Phase::Failed`, the Body sub-tab shows, instead of
      "No body": the `error_words` heading in the theme's danger colour, then the
      full `message` in a block that wraps (no `truncate`) and scrolls vertically
      when taller than the pane, and a Copy button (`IconName::Copy`, label or
      tooltip "Copy error") that writes the full error text to the clipboard.
- [ ] The Failed status line (mod.rs:145-170) still renders as one truncated line;
      its layout is unchanged apart from the tooltip text now coming from the
      shared function.
- [ ] For phases other than Failed, the Body sub-tab is unchanged ("No body" still
      shows for Done with no body, e.g. a HEAD or 204).
- [ ] The Done line's timing text comes from a pure helper; with `total` None it
      reads `· 1.2 KB` (no empty `· ·` segment), with a total it still reads
      `· 12 ms · 1.2 KB`. A unit test covers both, and `done_tooltip` is unaffected.
- [ ] The `jump_for_key` doc comment (body.rs:64-65) names Ctrl alongside Cmd for
      Down/Up and fits within the surrounding comment width.
- [ ] `cargo test -p wisp-app`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo fmt --all --check` pass.
- [ ] `cargo build --release -p wisp-app` then
      `RENDERER=hardware scripts/measure/measure.sh <target/wt-66/release/wisp> logs/66-measure.json`
      exits 0 (within the `aarch64-apple-darwin` budgets in
      `scripts/measure/budgets.json`). Record binary bytes and both RSS figures,
      with the delta in bytes against the same build of `main` at b91117a, under
      `## Notes`.
- [ ] Do not open a PR or push; commit on `task/66-failed-error-panel` only.

## Out of scope
- A friendlier parsed summary ("Connection refused — 127.0.0.1:1"); show the
  heading and raw text only.
- Any change to `crates/core` (the message text, `ErrorKind`, `classify`).
- Text selection inside the panel (GPUI has no partial selection here; the Copy
  button is the copy path).
- The Headers and Cookies sub-tabs, the Pretty/Raw/Save toolbar, and the
  header-selection-reset residual (33).
- Adding or embedding icons, and `crates/app/src/assets.rs`.
- docs/, PLAN.md, HISTORY*.md (scribe).

## Notes

Measure (aarch64-apple-darwin, RENDERER=hardware, within budget, exit 0):
- Branch: binary 10436688 B, idle RSS 97696 KiB, after-request RSS 102176 KiB.
- main b91117a: binary 10420160 B, idle RSS 98224 KiB, after-request RSS 101952 KiB.
- Delta binary: +16528 bytes. RSS deltas are within run noise (idle -528 KiB, after request +224 KiB).
