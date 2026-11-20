# 192 — Show a comparison of two history entries

**Repo:** .
**Round:** 0
**Depends on:** 183, 188
**Owns:**
- crates/app/src/ui/history.rs
- crates/app/src/ui/compare.rs (new)
- crates/app/src/ui/mod.rs (`mod compare;` only)

## Goal
From the History panel, a user compares two runs and sees what changed in
status, headers and JSON body. The view says plainly when a side's evidence
is missing or truncated. Volatile fields can be hidden with a per-collection
ignore list that the user builds from the view.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 2": the behaviour, the evidence wording table, the ignore list UX and security.
- Task 188 API: `wisp_service::compare::{compare, IgnoreList, Comparison}`; storage `load_compare_ignore`, `save_compare_ignore`.
- crates/app/src/ui/history.rs:
  - `HistoryView` :175-260;
  - its generation-guarded `reload` :209 and `open` :248;
  - `delivery_chip` :112, `checks_chip` :100, `origin_line` :122.
- crates/app/src/workspace.rs:2934 `history_job()` (store and root), and `HistoryStore::load`. If a new `Workspace` method is needed, stop and report; `workspace.rs` is not in Owns.
- Theme tokens only (`cx.theme()`), as elsewhere in `ui/`.
- Worktree: `./.claude/scripts/wt-new.sh . 192 compare-view`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-192`. Foreground commands only; never `pkill` or `killall`; stop only a pid you launched.

## Acceptance
- [ ] AC1 Each history row whose request path has an earlier entry offers "Compare with previous", which opens the comparison of that pair. Any row can be marked "Compare from"; another row then offers "Compare to". Marks clear when the panel reloads for another collection.
- [ ] AC2 The view's header shows, for each side: relative time, request path, method and URL template, environment name, outcome or status, the delivery chip and the checks chip.
- [ ] AC3 The sections, in order:
  1. **Status.**
  2. **Headers:** added, removed, changed, redacted, and the "headers not recorded" note.
  3. **Body:** the evidence notes in the spec's words come first; then the JSON changes as pointer path, A value, B value; or "Bodies are not JSON; text diff not available (equal / different)"; or "Too deeply nested to compare".
  4. **Hidden by the ignore list: N.**

  When more than 1,000 changes exist, it shows "and N more".
- [ ] AC4 Each header and body change row has "Ignore". It shows an inline confirmation with the exact pattern (`header:<name>` or `body:<pointer>`) and states that the pattern is written to this collection's `collection.toml`. Confirming saves the pattern and recomputes the comparison. The view lists the current patterns, each with Remove. Invalid patterns are shown with their reason.
- [ ] AC5 Values are shown as compare returns them (capped), with control and bidi characters escaped. There is no copy or export action.
- [ ] AC6 Loading both entries and the ignore list, computing the comparison, and saving patterns all run off the UI thread. A stale result (older generation) never overwrites a newer one.
- [ ] AC7 Tests, prefix `compare_view_`:
  - pure tests: previous-entry selection (same path, older, skips other paths); evidence note text for each reason; pattern built from a header row and from a body row; escaping of a value with a bidi character;
  - one gpui test that opens a comparison between two stored entries and finds the status row.

  At least 7 tests.
- [ ] AC8 No file outside Owns changes. No new dependency. fmt, clippy and the workspace suite pass.

## Checks
- `compare-view-tests` command — AC1, AC3, AC4, AC5, AC7
  run: `cargo test -p wisp-app --locked compare_view_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([7-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC8
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/app/src/ui/history.rs$' -e '^crates/app/src/ui/compare.rs$' -e '^crates/app/src/ui/mod.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `ux-and-threads` read — AC2, AC4, AC6
  how: the reviewer checks the header fields, that the confirmation names `collection.toml` before any write, that every load and save is spawned off the UI thread with the generation guard, and that no copy action exists.

## Carried from task 188's review
- Build every compare `Side` from `HistoryStore::load`, not `load_without_body`; the compare needs the stored bodies.

## Out of scope
- The compare logic and the storage of patterns (188).
- Comparing the live unsaved response, a CLI compare, or export.
- Palette commands or shortcuts for compare.
