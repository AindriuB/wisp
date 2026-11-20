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

## Review round 1 — findings
Reviewed HEAD: b24c8d0cc456787504204a51915043bbd77de0f0

- [P2] crates/app/src/ui/compare.rs:197-212, 236-248 — Saving an ignore pattern overwrites the whole `[compare] ignore` array with the copy the view loaded when it opened (`self.patterns`). Any change made to the file since then is lost. The spec says confirming "appends the pattern" (docs/plan/specs/2026-11-19-wisp-0-5-0.md:117).
  route: implementer — inside the background task in `apply`, call `load_compare_ignore(&root)` again, then add the pattern if it is absent or remove it by value (not by index), and save that list. This brings the code in line with "appends" and needs no file outside Owns. Making the read and write atomic under the root lock would need a storage API change, which is outside Owns.
  input: open Compare while collection.toml has `ignore = ["header:date"]`; add `"body:/meta"` outside the view (an editor, a git pull, or a second Wisp instance); click Ignore on an `etag` header row and Confirm  →  the file now holds `["header:date","header:etag"]` and `body:/meta` is deleted without any message. A related case: two Confirms within one save's latency (the save can wait up to 5 s for the root lock) start two saves, each built from the same stale list. The first pattern is lost, and because the two background saves can finish in either order, the file can end up different from what the view shows.
- [P2] crates/app/src/ui/history.rs:271-272 (and :219-227) — `start_compare` increments the same `generation` counter that `reload` uses to guard its result. A compare click and a reload therefore cancel each other's results. Also, when the root changes, `reload` keeps the old collection's rows on screen until the new list arrives.
  route: implementer — give the compare its own generation counter, and set `listing = Listing::Loading` in `reload` when the root changes. This keeps AC1 and AC6 as written.
  input: (a) with the History panel open, a request finishes (HistoryChanged → `reload`, generation n) and the user clicks "Compare with previous" before `job.list()` returns (generation n+1)  →  the reload result is discarded and the list stays without the new entry until the next HistoryChanged. (b) The opposite order: the user clicks Compare, then a HistoryChanged reload starts before `open_existing` returns  →  the click does nothing. (c) The user switches collection X→Y: `reload` clears the mark but still shows X's rows. The user clicks Compare on an X row  →  `start_compare` uses Y's root and Y's job with X's entries; the reload result is discarded, so X's rows stay listed under Y; both sides show as missing (unavailable body, headers not recorded); and Ignore writes to Y's collection.toml.

## Attempt 2 — failed
/verify at 8b6faa63d46cad6eb6d78a032551de600e8919a0: tester PASS (suite, fmt, clippy, 9 compare_view_ tests); reviewer REQUEST CHANGES.
- AC1 [?] — crates/app/src/ui/history.rs:361-377, :227 — no `compare_view_` test exercises "Compare from"/"Compare to" marking, or that the mark clears when the collection changes. history.rs:734 is outside the check's filter and asserts the listing and generations, not `mark`. Code reads correctly. Fix (test-only, within Owns): add a `compare_view_` gpui test that marks a row, compares to another, and then switches collection, asserting `mark` is None.
- AC3 [?] — crates/app/src/ui/compare.rs:390-498 — compare-view-tests asserts only the Status row. No test covers section order, header added/removed/changed/redacted rows, the "not recorded" note, NotJson/TooDeep text, "Hidden by the ignore list: N", or "and N more". Fix (within Owns): factor the section lines into a pure function and add `compare_view_` tests over it covering those cases.
- Suggestion carried: compare.rs:318, :324 — pass the request path and environment name in the side header through `escape_display`, like the URL on the same line (AC5).

## Review round 3 — findings
Reviewed HEAD: 88a5d330cd16828d0c37b0840c5885afe531aaed

- [P2] crates/app/src/ui/history.rs:346-351, :369-379 (test at :734-785) — The second half of AC1 still has no test. Nothing checks that "Compare from" marks a row, or that another row (and not the marked row itself) then offers "Compare to" with the marked entry as side A. The renamed test only sets `v.mark = Some("e1")` directly and checks that the mark clears when the collection changes. That covers the clearing half of the round-2 AC1 item. The fix the item asked for was a test that "marks a row, compares to another, and then switches collection", and that is not done.
  route: implementer — test-only, inside Owns. Move the per-row decision (previous entry, `marked`, the `from` entry found from `mark`) into a pure helper in history.rs and add `compare_view_` tests over it. Alternatively, drive the "compare-from" / "compare-to" buttons in the existing gpui test. AC1 and the rest of the spec stay as written.
  input: change `.filter(|_| !marked)` at history.rs:350 to `.filter(|_| true)`, so the marked row offers "Compare to" itself and compares an entry with itself. Or make the "Compare from" handler at :371 never set `mark`.  →  every `compare_view_` test still passes, so the evidence for AC1 is still missing at /verify.

## Attempt 4 — failed
/verify at f3ca17eb28740f2b8363843ef7bf278f3ae8efde: tester PASS (suite 1656 passed, fmt, clippy, 14 compare_view_ tests); reviewer REQUEST CHANGES.
- AC3 [?] — crates/app/src/ui/compare.rs:439-445 — No test reaches the `BodyDiff::Incomplete` branch, which is where the Body section shows the evidence notes. The note wording is tested only through `evidence_note` (:629). If that branch pushed no rows, every `compare_view_` test would still pass. Fix (test-only, inside Owns): add one `report_rows` case with `BodyDiff::Incomplete { a: Some(..), b: Some(..) }`. Assert the "A: …"/"B: …" note rows directly after "Body", and assert that no row says "equal". (Suggestion: use `BodyKind::StoredUnavailable` as one side.)

## Wave stopped — step 3 — round cap (attempt 4 of 4); remaining gap is one test-only AC3 case (BodyDiff::Incomplete rows). Code passes the full suite and every other AC.

## Wave resumed — user chose (a): one fresh implementer run past the cap under systematic-debugging, to add the one test-only AC3 case (report_rows with BodyDiff::Incomplete) from Attempt 4

## Wave verified — 4e369ac929a1b8ca0263508d98c43f0e137b30f5

## Outcome
Merged via PR from `task/192-compare-view`, verified at 4e369ac929a1b8ca0263508d98c43f0e137b30f5 (tester PASS, reviewer APPROVE) after attempts 1 to 4 and one run past the round cap, authorised by the user. The History panel offers Compare with previous and Compare from/to; the comparison view and ignore-list editing live in `crates/app/src/ui/compare.rs`. Open follow-ups are in PLAN.md under "Follow-ups from task 192".
