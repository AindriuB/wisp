# 112 — Workspace holds several open collections

**Repo:** `.`
**Depends on:** none
**Size impact:** binary none expected (no new dependencies). RSS risk: every open collection keeps a tree and a watcher, so measure it (AC11).
**Owns:**
- crates/app/src/workspace.rs
- crates/app/src/settings.rs
- crates/app/src/main.rs
- crates/app/src/actions.rs
- crates/app/src/palette.rs
- crates/app/src/ui/palette.rs
- call-site edits only, with no behaviour change, where these read the single `collection` field: crates/app/src/ui/{root.rs,sidebar.rs,import.rs,history.rs,cookie_jar.rs,url_bar.rs}, crates/app/src/ui/request/{auth.rs,headers.rs}
- scripts/measure/measure.sh, scripts/measure/measure.ps1 (Amendment 1, item 1)
- crates/app/src/ui/sidebar.rs behaviour, limited to clearing pending Move/Rename, selection and path-keyed collapsed state on active-collection change (Amendment 1, item 2)
- crates/app/src/ui/root.rs behaviour: the Close collection handler and its dirty-tabs prompt, and removing the discard prompt from Open collection (Amendment 1, item 3)

## Goal
`Workspace` becomes a list of open collections instead of one. Each collection has its own tree, watcher, environment selection, collection id, history state and tabs, and one of them is active. `settings.toml` remembers the open list and the active collection. This task adds palette commands to switch and close collections. The sidebar switcher is task 113. The Engine and its per-collection client and jar cache stay shared.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md:78-83 — the multiple-collections design and the cap of about 8
- crates/app/src/workspace.rs:38-43 (`OpenCollection`), :95-110 (`IdState`), :895-929 (`Workspace` fields), :1106 (`open_collection`, which replaces today), :1320 (scratch tabs), :1374 (`scope_for`)
- crates/app/src/settings.rs:23-42, :103, :187 — `Settings`, load and save
- crates/app/src/main.rs:169-181 — startup settings and the argv collection
- PLAN, follow-ups from task 107. Quit saves only the open collection's jar; this task makes it save every open collection.
- PLAN, accepted residual (user 2026-11-10): clones that share an id share history, tokens and cookies. Opening a clone beside its original is new here; see AC8.

## Acceptance
- [ ] AC1 `Workspace` holds an ordered list of per-collection states and the active index, with an `active_collection()` accessor. Each state owns its tree, environments, active environment, watcher, `IdState`, history state and tabs. A tab belongs to exactly one collection. Scratch tabs created with no collection open belong to a scratch group, which shows when no collection is active.
- [ ] AC2 "Open collection" adds a collection and makes it active; it no longer replaces. Opening a path that is already open (compared after canonicalising) activates the existing one, and no second watcher is created (test).
- [ ] AC3 At most `MAX_OPEN_COLLECTIONS = 8` are open. Opening a ninth is refused with a notice, and nothing changes (test).
- [ ] AC4 `settings.toml` stores `open_collections = [..paths]` and `active_collection = "<path>"`, written with `toml_edit` so other keys and comments survive. It is written on open, close and activate, and restored at startup. An argv path is added and activated. A path that is missing at startup is dropped with one notice and is gone after the next write (tests on the settings functions).
- [ ] AC5 Closing a collection with dirty tabs asks for confirmation first. Closing cancels in-flight sends and authorizations for its tabs, drops its watcher, and queues a cookie save if its persistence is on and the jar is dirty.
- [ ] AC6 Engine events route to their tab in any collection, so a background collection's response completes its tab. The history entry goes to the sending tab's collection id, not the active one (test).
- [ ] AC7 The quit plan saves the jar of every open collection that has saving on and a dirty jar (test over two collections).
- [ ] AC8 A collection whose `collection.toml` id matches an already open collection at a different path is refused with a notice naming the open one. The check runs once the id has been read. Nothing is shared and nothing is written (test).
- [ ] AC9 Palette commands "Switch collection…" (lists open collections by name, with the path as detail) and "Close collection" (closes the active one) exist. Switching swaps the tree, the environment picker and the visible tabs, and emits `CollectionChanged`.
- [ ] AC10 `WISP_AUTOSEND_URL` and `scripts/measure/measure.sh` behave as before with no collection open. Each Wisp launch in `measure.sh` and `measure.ps1` uses a fresh temp `WISP_DATA_DIR` that the script creates and removes, so measurement never reads or writes the developer's settings, collections or history (Amendment 1).
- [ ] AC11 The PR reports macOS idle RSS with 1 collection and with 8 small collections open. The app is launched with a distinct bundle id and a temp `WISP_DATA_DIR`, and only the launched pid is stopped.
- [ ] AC12 On active-collection change the sidebar clears any pending Move or Rename and the selection, and the collapsed state too if it is keyed by relative path. A test, or a test on a pure helper the sidebar calls, shows a pending move or rename does not survive a collection change (Amendment 1).

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `app-unit` command — AC2, AC3, AC4, AC6, AC7, AC8, AC12
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-112; test "$(cargo test -p wisp-app --locked collections -- --list 2>/dev/null | rg -c ': test$')" -ge 8 && cargo test -p wisp-app --locked`
  (new tests have `collections` in their name or module path)
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-112; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock`
- `call-sites` read — Owns
  how: the files listed as call-site edits change only the lines that read the collection, with no new behaviour.
- `model` read — AC1, AC5, AC9, AC10
  how: no per-collection state is left as a single `Workspace` field. Close cancels by tab. No disk I/O on the UI thread is added. Both measure scripts create, pass and remove a temp `WISP_DATA_DIR` per launch (AC10).
- `hand` read — AC11
  how: the PR gives both RSS figures, and how they were measured.

## Out of scope
- The sidebar collection switcher and close buttons (task 113). Keychain anything (114, 115).
- Re-keying a cloned collection's id. The accepted residual stands for clones that are not open together.
- Other follow-ups from task 107, including 47-4 `id_waits` on close_tab, unless the rewrite deletes that code anyway.

## Amendment 1 (user decisions, 2026-11-12)
1. **Measure scripts.** `scripts/measure/measure.sh` and `scripts/measure/measure.ps1` join Owns. Each Wisp launch there uses a fresh temp `WISP_DATA_DIR`, created and removed by the script, so measurement never reads or writes the developer's real settings, collections or history. AC10 is checked against that.
2. **Sidebar pending state.** Owns gains behaviour edits in `crates/app/src/ui/sidebar.rs`, limited to this: on active-collection change, clear the pending Move/Rename state and the selection, and the collapsed state too if it is keyed by relative path. AC12 covers it.
3. **Root handlers.** Owns gains behaviour in `crates/app/src/ui/root.rs`: the Close collection handler with its dirty-tabs prompt (AC5), and removing the discard prompt from Open collection, since open now adds. Reason: both follow directly from 112's model change. Task 113 (view-only switcher) calls this same handler and does not own a copy.

## Review round 1 — findings
Reviewed HEAD `bdd09b5947e233813c542badd68f8c2ec0b1aa1b`.
- [P2] workspace.rs:3480 `collections_history_goes_to_the_sending_collection` never builds a `Sent` or calls `append_history`; it passes if `append_history` used the active collection instead of `sent.root`. Make the test go through `append_history` (or the pure function it uses) with a `Sent` from a non-active collection.
- [P2] workspace.rs:3500 `collections_quit_saves_every_open_collection_with_saving_on` computes the saving id list inline; it never calls `saving_ids`/`take_quit_work`, so reverting `saving_ids` to active-only still passes. Exercise the real function.
- [P2] Measure scripts: per Amendment 1 item 1.
- [P2] Sidebar pending state on switch: per Amendment 1 item 2.
- [P3] root.rs:301-340 `on_close_collection` captures index `ix` before the prompt and closes by index afterwards; `PaletteKind::Collection(ix)` likewise. An AC8 clash refusal landing in between shifts indices, closing or activating the wrong collection. Capture the root and resolve with `coll_ix` after the prompt or selection.
- [P3] workspace.rs:418-433 with 8 remembered collections and an argv path, the argv path is refused (correct) but active falls back to index 0 and is persisted, instead of keeping the remembered active collection.

Non-blocking notes for /verify:
- Settings are saved on the UI thread via `update_settings` on open, close and activate (existing idiom), and restore scans up to 8 trees at startup.
- Clone refusal (AC8) picks the last collection as active rather than the previous one.
- Paths are stored via `to_string_lossy`.

## Review round 2 — findings

Reviewed HEAD: d161b416bb66d7e08a2f3f99997a2756c1e3d466

- [P2] crates/app/src/ui/sidebar.rs:273 — AC12 behaviour is in place (`reset_on_switch` clears pending, selected and collapsed on root change), but no test guards it: making `reset_on_switch` a no-op leaves every test passing. Move the compare-and-clear into a pure function (e.g. taking `(&mut shown_root, new_root, &mut pending, &mut selected, &mut collapsed)`), call it from `reset_on_switch`, and add a `collections_`-named test showing a pending Rename or Move is cleared when the root changes and kept when it does not.
