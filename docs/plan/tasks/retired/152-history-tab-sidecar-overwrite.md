# 152 — Saving a path-less tab never writes to an existing sidecar

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/workspace.rs (the path-less `else` branch of `Workspace::save` :3140-3151, the tab update after it :3153-3161, one new private helper beside `save`, and new tests in its `tests` module only)

## Goal
A tab opened from history keeps the stored draft's `TextBody.sidecar`. Saving it
takes the path-less branch of `Workspace::save`, which creates a new TOML at the
collection root and calls plain `save_request` with no baseline and no sidecar
guard, so `write_body` overwrites `<root>/<sidecar>` with the history body
(possibly the redacted copy). That is silent data loss in the original request's
sidecar, or in any unrelated root file of that name. Rule (orchestrator's
decision): a save that creates a new request file never writes to a pre-existing
file; the new request is self-contained, with any Json/Text/Xml sidecar body
converted to an inline body (`sidecar = None`, content kept).

## Context
Verified by reading origin/main at 669b4f4 (PR #101 merged); the defect reproduces.
- crates/storage/src/history.rs:343 — `NewEntry.request` is "the unresolved draft"; `redact_request` (:441-512) touches body content only, never `sidecar`, and `TextBody` derives `Serialize` (crates/core/src/request.rs:255-260), so the stored request keeps `sidecar`.
- crates/app/src/workspace.rs:739 `sanitize_redacted` and :864-878 `sanitized_origin` — neither clears `sidecar`.
- crates/app/src/workspace.rs:2945-2985 `open_history_entry` → crates/app/src/tab.rs:188-194 `from_history` builds a tab with `path = None`, `saved = draft = stored request`.
- crates/app/src/workspace.rs:3041 `save`; :3140-3151 path-less branch: `ops::create_request(&root, "", ..)` then `save_request(&root.join(&rel), &draft)`. :3153-3161 then `mark_saved(rel)`, which sets `saved = draft.clone()` (tab.rs:230-236), so the tab would still carry the sidecar into later pathed saves; the fix must clear it in the tab's `draft` and `saved` too.
- crates/storage/src/request.rs:555 `save_request` → :569 `write_request` → :710-718 `write_body` writes `<dir>/<sidecar>` whenever its bytes differ. Not owned: the fix goes in the app, at the one caller that creates a request from an in-memory draft.
- Other path-less sources, checked on origin/main; none can carry a sidecar into this branch:
  - Scratch tabs (`new_scratch`, tab.rs:196) start from `blank_request`, and `save` bails "Open a collection to save" for them (workspace.rs:3042-3044).
  - Import writes through crates/storage/src/import/write.rs:213-217 with its own `create_request`/`save_request`; curl.rs:564, postman.rs:384 and openapi.rs:595 all build `sidecar: None`.
  - There is no duplicate or move-tab-into-collection operation. `on_node_moved` (:3181) only rewrites existing paths.
  - The body editor keeps the sidecar across text types (ui/request/body.rs:80) but only from the tab's own draft.
- Existing sidecar tests to keep green: workspace.rs:5161-5560 (`save_checks_sidecar_changes_before_watcher_and_after_keep`, the `sidecar_tab` helper :5298, and the keep-mine/deleted/non-UTF-8 family); crates/storage/src/request.rs sidecar tests (:852-1050).
- Test fixtures: `fixture` (:4855) and `history_fixture` (:4626). The AC1 test drives `open_history_entry` against the fixture collection's `HistoryStore` and runs `cx.run_until_parked()`. If the fixture cannot reach the store, it may push `RequestTab::from_history` built from `HistoryStore::load(..).request` passed through `sanitized_origin`, and must say so in the report.
- Worktree: `./.claude/scripts/wt-new.sh . 152 history-tab-sidecar-overwrite`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-152`. Run every command in the foreground; leave nothing running.
- Never read credential files (~/.cargo/credentials*, ~/.ssh, ~/.gnupg). Fixtures use invented names and literal fake values only.

## Acceptance
- [ ] AC1 Failing-first test `path_less_save_from_history_keeps_the_original_sidecar` (gpui) reproduces the sequence. `create-user.toml` at the root has a Json body with `file = "create-user.body.json"`. A history entry is appended with that request. The sidecar is then edited on disk. The entry is opened and saved. After Save: (a) `create-user.body.json` bytes equal the edited bytes; (b) a new TOML exists and `create-user.toml` is byte-identical; (c) the new TOML has no `file` key and its body `content` equals the history body. The implementer's report includes this test's failing output on unfixed code.
- [ ] AC2 Same test, redacted variant `path_less_save_of_a_redacted_history_body_keeps_the_original_sidecar`. The history body is redacted (a Text body that `redact_request` clears, or a Json body with a secret field). The original sidecar bytes are unchanged and the new TOML holds the redacted body inline.
- [ ] AC3 Test `path_less_save_never_touches_an_unrelated_root_file`. The original request lives in a subfolder with `file = "body.json"`, and an unrelated `<root>/body.json` exists. Saving the history tab leaves both files byte-identical and writes the body inline.
- [ ] AC4 After the path-less save, the tab's `draft` and `saved` bodies have `sidecar == None` with content unchanged, and the tab is clean. A second edit followed by Save (now the pathed branch) still writes inline and leaves the AC1 sidecar unchanged; assert this in the AC1 test.
- [ ] AC5 The conversion is one private helper in workspace.rs, applied to `draft` before `create_request`/`save_request` in the path-less branch. It maps Json/Text/Xml with `Some(sidecar)` to the same variant with `sidecar: None`, keeps `content`, and leaves every other body untouched. A unit test `path_less_save_inlines_every_text_body_kind` covers Json, Text, Xml and one non-text body.
- [ ] AC6 No other path-less source can carry a sidecar, per the Context list. The report re-confirms this on the branch head in one line per source, citing file:line. If one is found, add a test of the AC1 form for it before merging.
- [ ] AC7 Saves of existing sidecar requests are unchanged: the pathed branch (:3044-3139) is not edited, and every existing sidecar test in workspace.rs and crates/storage passes unmodified.
- [ ] AC8 fmt, clippy with `-D warnings` and the full workspace suite pass.

## Checks
- `path-less-unit` command — AC1, AC2, AC3, AC4, AC5 (Rust tests write no JUnit XML here; the count is gated by grep)
  run: `cargo test -p wisp-app --bin wisp path_less_save 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([4-9]|[1-9][0-9]+) passed; 0 failed'`
- `sidecar-regression` command — AC7
  run: `cargo test -p wisp-app --bin wisp sidecar 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]' && cargo test -p wisp-storage sidecar 2>&1 | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `diff-scope` command — AC5, AC7
  run: `git diff --name-only main...HEAD | grep -vx 'crates/app/src/workspace.rs' | wc -l | grep -qx ' *0'`
- `shape` read — AC1, AC5, AC6, AC7
  how: the production hunks are the helper, its call in the path-less branch, and the tab update setting the inlined body into `draft`/`saved`. There is no hunk in the pathed branch or in storage. The AC1 failing output and the AC6 source list are in the report.

## Out of scope
- Whether history should store redacted bodies or keep `sidecar` in stored requests (history.rs redaction).
- Clearing `from_history` on save (PLAN :500), and the #101 Lows.
- A storage-level guard in `save_request`/`write_body` or `ops::create_request`, and import's write path.
- Changing the sidecar guard, `save_request_if_unchanged`, or the pathed save branch.

## Review findings (round 1, 8d2409d)
- [High, in-spec, AC4] The save-time tab update sets `t.draft.body` without `DraftReplaced`, so the open BodyEditor's stash keeps `sidecar: Some(..)` (ui/request/body.rs:90-99, :232-280). The next editor edit restores the sidecar into the draft. The next save then runs the pathed branch and can, after Keep mine, overwrite the original sidecar. Fix: inline the sidecar when the history tab is opened (`from_history` / `open_history_entry`, workspace.rs), so no editor ever sees it. Keep the save-time helper as a backstop. Any save-time draft change goes through `replace_draft`. Test: drive the edit through the body editor's stash (or the real edit path the editor uses), then save twice; the original sidecar is untouched and the TOML stays inline.
- [Low, in-spec, AC5] `inline_sidecar_body` was inserted between `refused_body_message`'s doc comment and its fn (workspace.rs:3220-3224). Move it so each item keeps its own doc.

## Outcome
Merged as PR #102 (9c3e5b2); branch tip 7765d1f, 1331 tests passed, CI green on macOS, Ubuntu and Windows. `open_history_entry` inlines Json/Text/Xml sidecar bodies before building the tab, and the save-time backstop inlines again through `replace_draft`. Only `crates/app/src/workspace.rs` changed. Review round 1's High (the body editor's stash restoring the sidecar after a save) was fixed in 7765d1f. Follow-ups 102-1 to 102-4 are in PLAN.
