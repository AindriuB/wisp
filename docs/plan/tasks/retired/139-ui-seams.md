# 139 — Add the wave-2 UI seams: new sub-tabs, dirty flag, response checks, guard setting, workspace accessors

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/tab.rs
- crates/app/src/draft.rs
- crates/app/src/settings.rs
- crates/app/src/ui/mod.rs
- crates/app/src/ui/request/mod.rs
- crates/app/src/ui/request/assertions.rs (new, stub only)
- crates/app/src/ui/response/mod.rs
- crates/app/src/ui/response/checks.rs (new, stub only)
- crates/app/src/ui/explain.rs (new, stub only)
- crates/app/src/ui/history.rs (insert-only: `had_assertions: false` in the `HistoryOrigin` test literal at :418)
- crates/app/src/orkspace.rs (delete only: stray empty file added by a731a77)
- crates/app/src/workspace.rs — SERIAL FILE, edit (a) of the 0.3.0 queue: `scope_path_for` to `pub(crate)` and the read accessors below only

## Goal
Freeze every type, field, accessor and mount point that tasks 140 (Explain UI),
141 (assertions UI), 143 (save and close), 147 (wiring) and wave-3 3A (guard)
read, so none of them edits these files' shapes again. Stubs render a one-line
placeholder; no feature behaviour lands here.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Waves" 2A; "Frozen and shared types this feature changes"; 2b "Storage (per user, settings.toml)"
- crates/app/src/tab.rs:14-28 sub-tab enums; :55-70 `ResponseState`
- crates/app/src/draft.rs:5-16, :45-60, test literals :126-230
- crates/app/src/ui/request/mod.rs:27-44, :48-122 (mount pattern: `X::new(workspace, tab, window, cx)`)
- crates/app/src/ui/response/mod.rs:29-31, :94, :116 `status_line`, :216-218
- crates/app/src/settings.rs:22-34, :143-156, :238-263 (`open_collections` precedent and bad-key notice)
- crates/app/src/workspace.rs:662-686 `scope_path_for`, :1104-1124 how `prepare_send` builds the scope, :53-70 `OpenCollection` (`coll_id`, `history` private), :1340-1358 `owner_of`/`root_of`
- crates/core/src/assert.rs:78 `Assertion`, :317 `AssertionResult` (hand-written Debug omits actual)
- Worktree: `./.claude/scripts/wt-new.sh . 139 ui-seams`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-139`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 `RequestSubTab::Assertions` (after `Settings`) and `ResponseSubTab::{Checks, Explain}` exist; `SUB_TABS` has 7 entries; the request strip mounts `ui::request::assertions::AssertionsEditor`, the response strip mounts `ui::response::checks::ChecksPane` and `ui::explain::ExplainPane`, each constructed as `new(workspace: Entity<Workspace>, tab: Entity<RequestTab>, window, cx)` and rendering a placeholder line.
- [ ] AC2 `DirtySections` gains `assertions: bool`, set by `dirty_sections` from `Request.assertions`; `is_dirty` maps `Assertions`; the draft.rs literals are updated and one new test shows an assertions-only edit flags only `assertions`.
- [ ] AC3 `tab.rs` defines `pub struct CheckOutcome { pub assertion: Assertion, pub result: AssertionResult }` (Debug, Clone, PartialEq) and `ResponseState` gains `pub assertions: Vec<CheckOutcome>`, covered by its `Default`. The definition is stored beside the result so the Checks pane can show the expected value as sent, after the draft changes. `HistoryOrigin` (tab.rs:107-130) gains `pub had_assertions: bool` (false everywhere in this task; 147 sets it).
- [ ] AC4 The response status line shows `"<passed>/<total> checks"` when `assertions` is non-empty, from a pure `fn checks_summary(&[CheckOutcome]) -> Option<String>` in `ui/response/mod.rs` with a test (0 entries None; 3 pass of 4 "3/4 checks"; skipped counts in total, not passed).
- [ ] AC5 `Settings` gains `confirm_send: BTreeSet<GuardKey>` with `pub struct GuardKey { pub collection_id: String, pub environment: String }` (Ord); stored as `confirm_send = ["<id>:<env>", ...]`, split at the first `:`; key absent when empty; `Settings` stays `Eq`. Tests: round-trip; a malformed entry (no `:`, empty id or empty env) is dropped with the existing bad-key notice; an environment name containing `:` round-trips.
- [ ] AC6 `workspace.rs` gains, and nothing else changes behaviour: `scope_path_for` `pub(crate)`; `pub fn send_scope(&self, tab: &Entity<RequestTab>, cx: &App) -> Scope<'_>` returning the same scope `prepare_send` would use (tab's owning collection, `scope_path_for`, active environment); `pub fn history_scope(&self, tab: &Entity<RequestTab>, cx: &App) -> HistoryScope` with `pub enum HistoryScope { Records(PathBuf), Scratch, NotRecorded, OriginalMissing, UrlEdited, Off(String), Pending }`; `pub fn known_collection_id(&self, ix: usize) -> Option<String>` (Some only when the id state is known). `HistoryScope` is chosen by a pure function with a table test covering each variant, including one case for `NotRecorded`, one for `Scratch`, "no path + Off" → `Off`, and "no path + Pending" → `Pending`. `NotRecorded`: the tab is owned by no collection (scratch group; `append_history` returns early when `sent.root` is None, workspace.rs:2759-2761), so nothing is recorded; the context strip text is "scratch: not recorded" (140 renders it). `Scratch`: the tab is inside an open collection with no saved request path, and its sends are recorded without a request link (spec:59 "scratch: recorded without a request link" applies to `Scratch` only). Priority: `NotRecorded`, `Off`, `Pending`, `OriginalMissing`, `UrlEdited`, `Records`, with `Scratch` as the fallback for an in-collection tab with no scope path (orchestrator decision 2026-11-15, amended). Reason: with history Off nothing is recorded, so `Scratch` ("recorded without a request link") would be false; and `OriginalMissing`/`UrlEdited` also have no scope path, so `Scratch` cannot come before them. The `HistoryOrigin` literals (:821, :3900, :3996) gain `had_assertions: false`.
- [ ] AC7 No new dependency; the full suite, fmt and clippy pass.

## Checks
- `app-seams` command — AC2, AC4, AC5, AC6
  run: `cargo test -p wisp-app --bin wisp 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && git diff --exit-code main...HEAD -- Cargo.lock`
- `seam-shape` read — AC1, AC3, AC6
  how: the diff shows the three stub files with the stated constructors; `workspace.rs` changes are limited to the visibility change, the three accessors, `HistoryScope` and its pure helper and test, and the `had_assertions: false` literal insertions.

## Out of scope
- Explain content and the context strip (140); editor and Checks content (141); save-and-close and F2-F4 (143); evaluating assertions, `Sent.environment`, history chips (147); the guard prompt, palette command and badge (wave 3, 3A).
- `Request.vars` dirty flag (0.4.0).

## Review round 1 — findings
Reviewed HEAD a731a77.
- [P2] workspace.rs:2331, :685 — `Scratch` conflated "not recorded" with "recorded without a request link". Settled by the 2026-11-15 decision in AC6: add `NotRecorded`, narrow `Scratch`, add a test case for each.
- [SCOPE] crates/app/src/orkspace.rs — empty stray file added by the commit, outside Owns. Delete it.
- [P3, follow-up] workspace.rs:2345-2346 — `pending` is true for `IdState::Failed` too, so a collection whose id cannot be read shows `Pending` instead of `Off(reason)`. Not in this round.

## Attempt 1 — failed
Verify reviewer REQUEST CHANGES at 9e7062c: the AC6 order in the brief (`Scratch` before `Off`/`Pending`) contradicted the code. Resolved by amending AC6 to the implemented order. Two table cases to add: "no path + Off" → `Off`, "no path + Pending" → `Pending`.

## Outcome
Merged as PR #97 (9d4d39a) at 462e135. Stub modules `ui/explain.rs`, `ui/request/assertions.rs` and `ui/response/checks.rs`, the `DirtySections.assertions` flag, `CheckOutcome` and `ResponseState.assertions`, `HistoryOrigin.had_assertions`, `Settings.confirm_send`, and the `send_scope`, `history_scope` and `known_collection_id` accessors are in. Open item: `history_scope` shows `Pending` for `IdState::Failed` (PLAN, wave 2 follow-ups).
