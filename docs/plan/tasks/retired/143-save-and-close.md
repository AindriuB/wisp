# 143 — Offer Save and close, and close the save data-loss gaps (F2, F3, F4)

**Repo:** .
**Depends on:** 139
**Owns:**
- crates/app/src/save_close.rs (new)
- crates/app/src/main.rs (the `mod` line and the quit prompt, :89-130)
- crates/app/src/ui/root.rs (the tab-close prompt :40-75, the collection-close prompt :466-495, `on_save` :418, the banner's Keep actions)
- crates/app/src/workspace.rs — SERIAL FILE, edit (b): the checked save inside `save` (:2842-2877), `keep_mine` (:2150), `apply_disk_state` (:2106) only; plus GPUI tests
- crates/app/src/tab.rs (`mark_saved` :209-213 and one disk-baseline field on `RequestTab`)
- crates/app/src/draft.rs (`on_disk_change` :68-90 and its tests)
- crates/storage/src/request.rs (`save_request_if_unchanged` and its tests)
- crates/storage/tests/save_if_unchanged.rs (new, optional)

## Goal
The three "Discard unsaved changes?" prompts offer Save first. Every save, plain
or from a prompt, writes only if the file still holds what the tab last read,
saved or kept, so an outside edit is never overwritten even before the watcher
reports it (F2); an outside edit that leaves a clean tab's file unparseable
raises the banner (F3); and a successful save clears "Deleted on disk" (F4).

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md 2a (behaviour, invariants 1-5, placement, tests); Amendment 1 (F2-F4); Risks 8
- Open question 1 answered from the code at 3c68586: 128 refuses only while `ChangedOnDisk` (workspace.rs:2846-2849), does not re-read at write time, and `keep_mine` (:2150-2152) sets `InSync` without re-baselining. So `save_request_if_unchanged` is needed and Keep must re-baseline.
- crates/app/src/draft.rs:76-80 the `Some(Err(_))` + clean → `Ignore` arm (F3); crates/app/src/tab.rs:209-213 `mark_saved` (F4); :107-130 `HistoryOrigin`
- crates/app/src/vault.rs:846-856 three-button prompt precedent; ui/root.rs:546-575 banner (Keep mine / Keep)
- 128's GPUI tests in workspace.rs (grep `Changed on disk`) — the pattern for new ones
- Worktree: `./.claude/scripts/wt-new.sh . 143 save-and-close`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-143`. Run every command in the foreground; leave nothing running.

## Checked save (one path for Ctrl-S and every prompt)
The tab's disk baseline is what its file held when the tab last loaded, reloaded, saved or kept it (a request, "missing", or "unreadable").

| Tab state | File now vs baseline | Result |
|---|---|---|
| no owning collection | — | `Err("Open a collection to save")`, nothing written |
| path None (scratch, from-history) | — | `create_request` fresh name, write (unchanged from today) |
| `ChangedOnDisk` | any | `Err`, nothing written (128, unchanged) |
| `InSync` | equal to baseline | write; `mark_saved` sets baseline = draft and disk `InSync` |
| `InSync` | differs, unreadable, or missing | `Err`, nothing written, tab set `ChangedOnDisk` (`DeletedOnDisk` if missing), stays open |
| `DeletedOnDisk` | still missing | write (recreates; nothing to overwrite); `mark_saved` → `InSync` |
| `DeletedOnDisk` | file exists again | `Err`, nothing written, tab set `ChangedOnDisk` |
| after Keep mine / Keep | baseline := file as it is at Keep time | rows above apply |

## Acceptance
- [ ] AC1 `wisp_storage::request::save_request_if_unchanged(path, expected, draft)` re-reads the file (sidecar body included; `expected` can also be "missing" or "unreadable", the latter compared by raw request-file bytes) and returns `Ok(SaveOutcome::Conflict)` without writing on any difference; otherwise writes through `save_request` and returns `Ok(SaveOutcome::Written)`. `SaveOutcome` lives in request.rs, so `storage/src/error.rs` is unchanged. Tests: unchanged writes; modified, deleted and sidecar-body-changed each conflict and leave the file byte-identical.
- [ ] AC2 `Workspace::save` implements the table; a GPUI test writes an outside edit to a clean tab's file with no watcher event processed, calls `save`, and shows `Err`, the outside bytes intact and the tab `ChangedOnDisk` (F2). Another shows Keep mine then Save writes.
- [ ] AC3 F3: `on_disk_change(false, _, Some(Err(_)))` returns `ShowBanner`; the decision-table test is updated.
- [ ] AC4 F4: `mark_saved` sets disk state `InSync`; a test saves a `DeletedOnDisk` tab whose file is still missing and shows the file recreated and the banner gone.
- [ ] AC5 Tab close offers `["Save and close", "Discard", "Cancel"]` (Save is index 0, the default) when the tab can be saved, else `["Discard", "Cancel"]` with the detail "Open a collection to save". Collection close offers "Save all and close" and quit "Save all and quit" with Discard and Cancel. Save-and-close closes only after `Ok` and `!is_dirty()`; save-all stops at the first failure, names that tab in a notice, and closes or quits nothing. `prompt_up` is cleared on every path.
- [ ] AC6 `save_close.rs` holds the pure button specs and the decision `(button, save result, dirty after) -> Close | KeepOpen(notice) | Cancel`, with a table test over: save ok, save error, conflict, no collection, still dirty after save, and save-all with the second of three failing.
- [ ] AC7 `close_step` and `quit_step` are unchanged; no secret or draft content in any new notice or log; full suite, fmt and clippy pass.

## Checks
- `storage-save` command — AC1
  run: `cargo test -p wisp-storage save_request_if_unchanged 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `app-save` command — AC2-AC6
  run: `cargo test -p wisp-app --bin wisp 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `serial-edit` read — AC2, AC7
  how: `git diff main...HEAD -- crates/app/src/workspace.rs` touches only `save`, `keep_mine`, `apply_disk_state` and tests; `close_step`/`quit_step` untouched.

## Out of scope
- README "Unsaved changes" wording (scribe, wave 3 docs).
- The environment guard and anything in `url_bar.rs`, `actions.rs`, `palette.rs` (wave 3, 3A; 148).
- Closing the remaining read-then-write window (the spec does not claim it is zero).

## Outcome
Merged as PR #101 (669b4f4) on 2026-11-16; CI green. Adds checked saves (a write only if the file still matches the disk baseline, sidecar contents included) and Save and close, Save all and close and Save all and quit prompts. Review rounds hardened sidecar handling: 10e9f46, 95e99d9, bddc261, 91f09b5 (101-1), a53bbd6 (101-2), 7c49485, f6997e8. Open Lows are in PLAN under "Follow-ups from tasks 143 and 152"; 101-3 and 101-4 are not recorded there or on PR #101.
