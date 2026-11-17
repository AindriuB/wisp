# 161 — Move outcome and history into wisp-service; add delivery (1B-ii, C1, E4, AC52, AC75, AC51)

**Repo:** .
**Round:** 0
**Depends on:** 160, 163, 164
**Owns:**
- crates/service/src/outcome.rs
- crates/service/src/history.rs
- crates/service/Cargo.toml (`[dev-dependencies]` only: add `serde_json`; Amendment 1)
- crates/app/src/workspace.rs (edit (b) only: outcome/history rewiring, AC52, AC51 call site, AC75)
- crates/storage/src/history.rs (the `Delivery` type and the `delivery` field only)
- crates/storage/tests/history.rs (insert-only: `delivery` in the `NewEntry` literal at :32)
- crates/app/src/ui/history.rs (the "outcome unknown" chip, and the `delivery` field in its test literals)
- scripts/check-boundaries.sh (insert-only: the `NewEntry {` half of exit check 3)

## Goal
The GUI derives check outcomes, the delivery state and the history entry through
`wisp_service::outcome::finish`, and appends history through
`wisp_service::history::record` using the collection id it already holds. History
entries gain an optional `delivery`, shown as "outcome unknown" in the history list.
Apart from AC52, AC75, the chip, (Amendment 2) history recovering once a
`collection.toml` with a bad id is repaired while open (AC14), and (Amendment 4)
after "Give this copy its own id", history reads and writes follow the new id
(AC15) and history reads use the known id while unresolved (AC16), no behaviour changes.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 1" (:62-70 what moves, :95-106 API, :116 `RecordError` split, :120-123 approach, AC52/AC51/AC75), "Delivery and outcome_unknown" (:171-195: the delivery table :183-189, `retry_safe` :193), "History vocabulary" (:225), Waves 1B-ii (:458).
- crates/app/src/workspace.rs: `HistoryJob` :556 (`open` id re-read :613-620), `check_outcomes` :879, `assertion_records` :899, `on_event` :2790 (matches at :2797, :2806, :2887, :2945 use `..`), body cap read :2864, `append_history` :2915 (`NewEntry` :2939-2955; spawn and notices :2956-2990, kept), cookies' known-id pattern :651-660, `resolve_id_then` :3523, cookie error log/notice :3581-3586, test `Workspace::new` resolving the real data dir :1529; moved tests :4213, :4247.
- crates/storage/src/history.rs:303 `HistoryEntry`, :343 `NewEntry`, :1063 literal, :1116-1119 (a 0.3.0 reader drops lines it cannot parse: why a field, not an `Outcome` variant).
- 164 adds `Event::Failed { nothing_sent }`; 163 maps the cookie parse error to a fixed message (AC51 storage half).
- Task 99 AC51, AC52, AC75 text: docs/plan/tasks/99-hardening-0-2-0.md:98, :101, :126.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 161 service-outcome-history`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-161`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (Amendment 1) A golden JSON string constant is captured from the pre-move app code (main before this task's move) for a fixed draft, scope and canned response with a `Completed` terminal and populated checks (no dynamic values; fixed time). A service test runs `plan::prepare` then `outcome::finish` on the same inputs, serialises the resulting `NewEntry` with `serde_json` and compares it to the golden; no field is overwritten before the compare except the timestamp `at`. The test fails if `finish` drops `assertions` or changes the `delivery` or `response_headers` values. The frozen `Failed`-terminal golden from fb21541 is removed.
- [ ] AC2 `wisp_service::outcome` holds `check_outcomes`, `assertion_records` (their tests :4213, :4247 move with them, names kept), `Terminal` (`Completed { body_prefix }`, `Failed { error, nothing_sent }`, `Cancelled`), `Response` (keeps `size: Option<u64>`; Amendment 1), `Finished { checks, delivery, status_ok, entry }`, `finish(record, &Response, &Terminal, expected_status) -> Finished`, `delivery(&Terminal, headers_seen) -> Delivery`, and `status_ok(status, expected) -> bool` (in `expected` when non-empty, else `< 400`).
- [ ] AC3 `delivery` implements spec table :183-189 exactly; a pure table test covers all five rows, including Cancelled without headers → `unknown`. `status_ok` has a table test (empty list with 399/400/404; list `[201]` with 200/201).
- [ ] AC4 `storage::history` gains `enum Delivery { NotSent, Unknown, Responded }` (serde `not_sent`/`unknown`/`responded`) and `delivery: Option<Delivery>` on `HistoryEntry` and `NewEntry` with `serde(default, skip_serializing_if = "Option::is_none")`. Tests: an index line written by 0.3.0 (no field) loads with `None`; an entry with `None` serialises without the key.
- [ ] AC5 `wisp_service::history::record(data, id, entry, body, bodies) -> Result<HistoryStore, RecordError>` takes the known `CollectionId` and never re-reads the id from disk (Amendment 2, narrowed by Amendment 3: one exception, re-reading after a cached `IdState::Failed` until the first successful re-read, for history only; see AC14); `RecordError` keeps the split "could not open the store" (history off) versus "append failed" (one entry dropped, notice), as `workspace.rs:2975-2986` does today.
- [ ] AC6 AC52: the GUI append passes `IdState::Known`'s id; when unknown, it goes through `resolve_id_then` first as today. It never re-reads the id from disk, except (Amendment 2, narrowed by Amendment 3) after a cached `IdState::Failed`, for history only and only until the first successful re-read (AC14). Test: open a collection, change `id` in `collection.toml` on disk, send; the entry lands in the original id's store and the new id's store does not exist.
- [ ] AC7 `on_event` copies `nothing_sent` from `Event::Failed` and whether a `Headers` event arrived into `Terminal`/`Response`, calls `finish` at the terminal event, and the appended entry carries `delivery`. The GUI passes `expected_status = &[]` and does not display `status_ok`.
- [ ] AC8 AC75: under test, `Workspace::new` never calls `DataDir::resolve()`; a test-only constructor path injects the temp data dir before anything is resolved. A test asserts the workspace's data dir is the injected one.
- [ ] AC9 AC51 call site: the cookie load error reaches the log and the notice only as the fixed storage message (line and column), never file content. Test: a cookie file containing `CANARY` in a broken JSON position yields a notice without `CANARY`.
- [ ] AC10 The history list shows the text "outcome unknown" in the existing chip for entries with `delivery == Some(Unknown)`; a pure test covers the chip text for `Some(Unknown)`, `Some(Responded)` and `None`.
- [ ] AC11 `scripts/check-boundaries.sh` fails if `crates/app/src/workspace.rs` above its first `#[cfg(test)]` contains `NewEntry {`; the script passes on this branch.
- [ ] AC12 No other existing test expectation changes (Amendment 2: the AC14 recovery is an allowed behaviour change beyond AC52, AC75 and the chip; Amendment 4 adds: after "Give this copy its own id", history reads and writes follow the new id, and history reads use the known id while unresolved); `workspace.rs` hunks are edit (b) only; no file outside Owns changes.
- [ ] AC13 (Amendment 1) The moved test `history_records_hold_no_expected_or_header_values` keeps its `serde_json` assertion from the app original, unweakened.
- [ ] AC14 (Amendments 2 and 3) When `coll_id` is `IdState::Failed` and no recovered history store is open yet, the history append re-reads the id from disk: on success it opens that id's history store and records the entry there; on failure today's "history is off" path runs. After the first successful re-read, every later append, list, load and clear uses that same store until the collection is reopened; later appends do not re-read the id. The shared `coll_id` is not changed, and `apply_id`/`on_id_known`, `plan_clash` and `cookies_not_saved` are not run, so cookie jars, secrets, clash handling and notices behave exactly as at d04e2be. Tests: (1) a collection opens with a bad id, `collection.toml` is repaired while open, a send follows; the entry is recorded under the repaired id, `coll_id` is still `Failed`, and the cookie jar is unchanged. (2) `recovered_history_reads_and_writes_use_one_store`: bad id → repair to A → send → change the id on disk to B → send; the history list shows both entries, and clear removes both. (Amendment 4) The first re-read to land sets the store; concurrent recovery appends use it. Test (3) `concurrent_recovery_appends_share_one_store`: `Failed`, repair to A, append 1, change the id on disk to B, append 2, park; the list shows two entries and B's history dir does not exist (a test-only hook to hold the first re-read is allowed). Tests (1) and (2) stay unchanged.
- [ ] AC15 (Amendment 4) Single store: each collection has one current history store (Amendment 4 invariant), and every append, list, load and clear uses it; only `reset_history` moves it to another id. Tests: `newid_moves_history_reads_and_writes_together` (open with A, send → `Ready(A)`, "Give this copy its own id" → B on disk, send: the list shows only the second entry, it is in B's store, A's store keeps the first, clear empties B and leaves A) and `late_append_after_newid_does_not_pin_old_store` (start a send with A, NewId before it finishes, park, send again: `history` is `Ready(B)` and the list shows the second entry).
- [ ] AC16 (Amendment 4) History reads use the known id while `history` is `Unresolved`: `history_append_uses_the_known_id_not_the_one_on_disk` is extended so that after the disk edit and before the send, `history_job().list()` reads A's store.

## Checks
- `service-unit` command — AC1, AC2, AC3, AC5, AC13
  run: `cargo test -p wisp-service --locked 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([8-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `storage-history` command — AC4
  run: `cargo test -p wisp-storage --locked history 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `boundaries` command — AC11
  run: `scripts/check-boundaries.sh && sed '/^#\[cfg(test)\]/,$d' crates/app/src/workspace.rs | rg -n 'NewEntry \{|(^|[^:.\w])resolve\('; test $? -eq 1`
- `suite-and-lint` command — AC6, AC8, AC9, AC10, AC12, AC14, AC15, AC16
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `scope` command — AC12
  run: `git diff --exit-code main...HEAD -- crates/core crates/cli crates/storage/src/collection.rs crates/storage/src/cookies.rs crates/app/src/tab.rs crates/app/src/vault.rs crates/service/src/plan.rs crates/service/src/send.rs`
- `rewire` read — AC1, AC6, AC7, AC8, AC9, AC12, AC13, AC14, AC15, AC16
  how: the reviewer checks the golden JSON comes from pre-move code with a `Completed` terminal and is compared without overwriting any field but `at`, that `crates/service/Cargo.toml` changes only `[dev-dependencies]` (adds `serde_json`), the known-id path, the `nothing_sent`/headers plumbing, the AC75 constructor, that the AC14 re-read leaves `coll_id` untouched and calls none of `apply_id`/`on_id_known`/`plan_clash`/`cookies_not_saved`, that every history append, list, load and clear goes through the one current store of the Amendment 4 invariant (only a current-generation, first-landing resolution sets `Ready`; only `reset_history` moves the store), and that `workspace.rs` hunks stay within edit (b).

## Out of scope
- `run.rs` and any CLI change (166); `HistoryStore::open_read_only` and H2 (168).
- Any other change to `Outcome`, history redaction or the history list layout beyond the chip text.
- GUI display of `expected_status` or `status_ok` (not in 0.4.0).

## Wave stopped — step 2 — two orchestrator-routed review findings
Code review at f75603323d467b385a1fdc44f7a97e0ec0e93100 (no round spent):
- [P2] crates/service/src/outcome.rs:344-350: the AC1 golden test overwrites `assertions` and `delivery` before comparing, and Debug output redacts header values, so it does not pin what `finish` builds. The frozen golden (fb21541) used a `Failed` terminal with checks, a state the old runtime never produced. A real fix needs a new `Completed` golden captured from pre-move code (changes AC1's inputs) and/or a JSON compare with `serde_json` as a dev-dependency of `crates/service/Cargo.toml` (outside Owns).
- [SPEC] crates/service/src/outcome.rs:49: `Response` adds `size: Option<u64>`, which spec :97 leaves out; task 166 is planned from that line.

## Amendment 1 (user, 2026-11-17)
Rulings on the two findings above; AC1, AC2 (`Response`), Owns and Checks are updated in place, AC13 added.
1. AC1 golden. The fb21541 golden pinned a `Failed` terminal with populated checks, a state the 0.3.0 runtime never produced, and the compare overwrote `assertions` and `delivery` and used Debug output, which redacts header values, so it did not pin what `finish` builds. Replace it with a `Completed`-with-checks golden captured from the pre-move app code, compared as JSON with only `at` fixed. `serde_json` becomes a dev-dependency of `crates/service/Cargo.toml` (now in Owns, `[dev-dependencies]` only), which also restores the `serde_json` assertion in the moved test `history_records_hold_no_expected_or_header_values` (AC13).
2. `Response` keeps `size: Option<u64>`. Spec docs/plan/specs/2026-11-17-wisp-0-4-0.md:97 must gain `size`; the scribe updates the spec at record (outside this task's Owns). Task 166 is planned from that line and should read the updated spec.

## Wave resumed — user: new Completed golden compared as JSON (serde_json dev-dep); Response keeps size, spec :97 updated at record (Amendment 1)

## Review round 1 — findings

Reviewed HEAD: f75603323d467b385a1fdc44f7a97e0ec0e93100

- [P2] crates/service/src/outcome.rs:282-353 — The AC1 golden is still the fb21541 `Failed`-terminal golden compared as Debug text. Before the compare, the test overwrites `entry.assertions` (:342-347) and `entry.delivery` (:348). The Debug output redacts header values (`"<redacted>"` in GOLDEN_ENTRY), so the test does not pin what `finish` builds. That breaks amended AC1 (new `Completed`-with-checks golden captured from pre-move code, JSON compare, only `at` fixed).
  route: implementer — Amendment 1 item 1 sets the fix, and Cargo.toml `[dev-dependencies]` is now in Owns.
  input: change `finish` to set `assertions: Vec::new()` or `delivery: None`, or to drop a response header's value  →  `prepare_then_finish_builds_the_entry_the_app_used_to` still passes, because assertions and delivery are overwritten and header values are redacted in Debug.
- [P2] crates/service/src/outcome.rs:243-260 — The moved test `history_records_hold_no_expected_or_header_values` lost the app original's `serde_json::to_string(&records)` / `!json.contains("CANARY")` assertion (removed at main's workspace.rs, deleted hunk around :3987). The test now checks only the Debug text. That breaks AC13.
  route: implementer — AC13, Amendment 1 item 1.
  input: an `AssertionRecord` Serialize impl that writes an expected or header value into the JSON while Debug stays clean  →  the moved test still passes.

## Accepted (orchestrator, 2026-11-17)
Cargo.lock (outside Owns) gains one line, `serde_json` under `wisp-service`: the direct result of the Amendment 1 dev-dependency under `--locked`.

## Review round 2 — findings

Reviewed HEAD: 7a0cc5d82fd57b5b5ba78587b96d93e4398c8d0c

- [P2] crates/service/src/outcome.rs:358 — The new AC1 golden test compares JSON as a string. The serialised key order depends on whether serde_json's `preserve_order` feature is on, and Cargo turns it on in workspace builds. `GOLDEN_JSON` (:279) has its keys in sorted order, which is the order without the feature. `cargo tree -e features -i serde_json --workspace` shows `preserve_order` is enabled through the app's dependencies, and `-p wisp-service` alone does not enable it. So the test passes under the `service-unit` check but fails under `cargo test --workspace` (the `suite-and-lint` check, and CI).
  route: implementer — compare parsed values instead, e.g. `assert_eq!(serde_json::from_str::<Value>(GOLDEN_JSON)?, json!({...}))`. That still meets AC1 as written: a JSON compare with only `at` fixed. Owns, AC and decisions do not change.
  input: `cargo test --locked -p wisp-service -p wisp-app --lib prepare_then_finish` (both crates in one build, as `--workspace` does)  →  `prepare_then_finish_builds_the_entry_the_app_used_to` panics at outcome.rs:358 (key order differs).

## Wave verified — d04e2be9492b884fc5ed7f239f0a8d5b79016f28

## External review — findings

Key: sha256:e7287bb64bc5 · reviewed head f7c5dea3e0e2c4dbc4a1e99395f2fbb5cc834042 (PR #127) · round 3

[P2] Retry failed collection-ID reads before disabling history — `crates/app/src/workspace.rs:2710–2712`
If a collection opens with an invalid ID in `collection.toml`, repairing the file while it remains open does not restore history. The resolver reuses the cached failure without rereading disk, and the new handler permanently disables history until the collection is reopened. This drops the current and subsequent history entries. Previously, opening the history store retried ID resolution and could recover.
Overall impact: the service extraction appears consistent, but this recovery path introduces history loss. No other actionable P0–P2 bugs were found.
Reviewed all 16 changed files (+910/−279), with matching revision checks before and after acquisition:
* Target: `main`
* Base: `2575b38a815de170e85796fb18111d69897fc256`
* Head: `f7c5dea3e0e2c4dbc4a1e99395f2fbb5cc834042`

## Wave stopped — step 7 — round-3 fix drifts (orchestrator) and the next round hits the cap
Re-review at 68d95d5fd0dd7cd7303a605d12f0d810e6dd46a1: the external P2 is fixed, but the fix resets the shared `coll_id` Failed→Absent and re-runs `apply_id`, which changes cookies, secrets and clash handling mid-session (session cookies can drop to another jar; a repaired id equal to another open collection's closes this one during a send; an extra "Cookies are not saved" notice). Design choice needed: (a) heal the shared id state, or (b) re-read the id only for the history append and leave `coll_id` alone (needs an AC5/AC52 ruling). Either way the Goal/AC12 "no behaviour changes" line must say it. Three rounds are used; any further fix is round 4, the cap.

## Amendment 2 (user, 2026-11-17)
User ruling on the round-3 stop: option (b). Reason: round 3's fix (68d95d5) reset the shared `coll_id` Failed→Absent in `append_history` and re-ran `apply_id`, which changed cookies, secrets and clash handling mid-session. History alone should recover from a repaired `collection.toml`; everything else keeps its d04e2be behaviour.
- After a cached `IdState::Failed`, the history append re-reads the id from disk for that append only; the shared `coll_id` stays as is and `apply_id`/`on_id_known`, `plan_clash` and `cookies_not_saved` are not run. Success records under the re-read id; failure keeps today's "history is off" path.
- AC5 and AC6 (AC52) gain this one exception to "never re-reads the id from disk". Goal and AC12 name the recovery as an allowed change. New AC14 with its test; Checks updated.
- Commit 68d95d5 (the shared Failed→Absent reset) is replaced by this design, not built on.

## Wave resumed — user: option (b) history-only id re-read (Amendment 2); one fresh round-4 run under systematic-debugging

## Review round 4 — findings

Reviewed HEAD: 68d95d5fd0dd7cd7303a605d12f0d810e6dd46a1

- [DRIFT, settled by Amendment 2] crates/app/src/workspace.rs:2699-2702 — The round-3 fix resets the shared `coll_id` from `Failed` to `Absent` and re-resolves through `resolve_id_then` → `apply_id`, which changes cookie jars, clash handling and notices mid-session.
  route: implementer — replace it with Amendment 2's design: on a cached `Failed` id, re-read the id from disk for the history append only; leave `coll_id`, cookies, secrets, `plan_clash` and `cookies_not_saved` untouched; add the AC14 test.
  input: open a collection with `id = "bad id"`, log in (session cookies in the canonical-root jar), fix the id on disk, send → at 68d95d5 `current_jar` switches to the real id's jar and the session cookies drop out; after the fix the jar is unchanged and the history entry is recorded.

## Wave verified — ed8f589c0460a78b2b2469da8766b7aef013245f

## Wave stopped — step 4 — PR #127 head is the scribe's record branch; updating it needs the user

## Wave resumed — user: scribe updates PR #127 (record/161) with ed8f589

## External review — findings

Key: sha256:17faf16b0b78 · reviewed head 49baa9a88c1d9472d5b8dd52d2581a5f0f504d9f (PR #127) · round 5 (past the cap; not routed)

P2] Keep history reads and writes on the same recovered store — `crates/app/src/workspace.rs:2712–2713`
After opening a collection with an invalid ID, repairing it to ID A and sending caches A’s history store. Changing the ID to B and sending again writes to B, while listing, loading and clearing history still use A. New requests disappear from the history list, and clearing history leaves them behind. The recovered read and write stores should remain consistent.
Overall impact: the service extraction appears consistent, but recovery can split a collection’s history across stores. No other actionable P0–P2 defects were found.
The complete diff covered 16 files (+1037/−279), with matching revision checks before and after acquisition:
* Target: `main`
* Base: `2575b38a815de170e85796fb18111d69897fc256`
* Head: `49baa9a88c1d9472d5b8dd52d2581a5f0f504d9f`

## Wave stopped — step 7 — external P2 at round 5 (cap) and design-pass trigger (second history-id recovery finding)

## Wave resumed — user: one more fresh fix run under systematic-debugging for the round-5 external P2 (past the cap)

## Wave stopped — step 2 — round-5 fix review: DRIFT (pins writes to the first recovered store, AC14 says re-read each append) needs an amendment; P3 race and a non-compiling intermediate commit 97a900d

## Wave resumed — user: record the pin-to-recovered-store design (Amendment 3) and squash 97a900d+d843606

## Amendment 3 (user, 2026-11-17)
User ruling on the round-5 fix review DRIFT. Reason: the round-5 external review found that re-reading the id on each append split a collection's history across stores (after repair to A then change to B, writes went to B while list, load and clear still read A).
- After a history-only recovery from a cached `Failed` id (Amendment 2), the first successful re-read opens that id's history store; every later append, list, load and clear uses that same store until the collection is reopened. Later appends do not re-read the id.
- AC5, AC6 and AC14 are updated to match; AC14 gains the test `recovered_history_reads_and_writes_use_one_store`.
- Amendment 2's constraints stay: no change to `coll_id`; `apply_id`/`on_id_known`, `plan_clash` and `cookies_not_saved` are not run.

## Wave stopped — step 2 — full-branch review at 1508052: P2 orchestrator (after "Give this copy its own id", writes go to B while list/load/clear read Ready(A)); third history-store/id finding, design-pass trigger

## Amendment 4 (user, 2026-11-17) — single history store
User ruling: adopt all five design steps below; history follows the new id after "Give this copy its own id". Reason: design pass after the third history-store/id finding. Reads and writes split because `HistoryState` picks the read store while `append_history` picks the write store again from `coll_id`, and any finishing append sets `Ready` without checking it is still current.

Invariant: each collection has one current history store, derived from `history` and `coll_id` alone:

| `history` | `coll_id` | current store |
|---|---|---|
| `Ready(s)` | any | s |
| `Unresolved` | `Known(id)` | store(id) |
| `Off` | any | none |
| `Unresolved` | Absent / Reading / Failed | whatever the next resolution yields |

Every append, list, load and clear uses it. Only a resolution started under the current history generation, and only the first to land, may move `Unresolved` → `Ready`. Only `reset_history` moves the store to another id (sets `Unresolved`, bumps the generation, emits `HistoryChanged`).

Design (crates/app/src/workspace.rs, edit (b)):
1. `HistoryJob` gains `id: Option<CollectionId>`; `history_job_of` (~:621) fills it from `known_id(&c.coll_id)` when `Unresolved`; `open_existing` (~:558) uses it before falling back to the disk read (kept only for Absent/Reading/Failed).
2. `append_history` (~:2703-2782): `job.store` is `Some(s)` → `RecordTo::Store(s)` whatever `coll_id` is (replaces the `recovered && Failed` arm); else `Known` → `RecordTo::Id(id)` (AC52); else `Failed` → Amendment 2 re-read; else `resolve_id_then`.
3. `OpenCollection` gains `history_gen: u64` from a workspace-wide counter at creation; `append_history` captures it and `spawn_record` takes it; `Ready` is set (~:2813) only if `history` is `Unresolved` and the generation is unchanged.
4. The `Failed` re-read opens the store in the background right after `read_id_state`; back on the UI thread in one update: if `history` is now `Ready(s)`, write to s and drop the re-read; else if the generation matches, set `Ready(new)` then write with `RecordTo::Store(new)`. The first re-read to land wins.
5. A `reset_history(ix)` helper; NewId calls it (~:1884) after `refresh_known_id` returns `Some`.

Amendment 2 limits still hold (no change to `coll_id`, `apply_id`, `on_id_known`, `plan_clash`, `cookies_not_saved`); AC52 holds. Goal, AC12 and AC14 are updated in place; AC15 and AC16 are added and covered by `suite-and-lint` and `rewire`. Owns does not change.

## Wave resumed — user: adopt the single-history-store design, all five steps; history follows the new id (Amendment 4)

## Review round 6 — findings

Reviewed HEAD: 040d31583b2c6c16ab09995404495df2dba0b007 (Amendment 4; single-store invariant checked clean across all transitions)

- [P2] crates/app/src/workspace.rs:57-59 — the new module-level `#[cfg(test)]` at column 0 (above `type HeldRecovery`) is now the first line that matches `^#\[cfg(test)\]`. Both boundary checks cut the file at that line with `sed '/^#\[cfg(test)\]/,$d'` (`scripts/check-boundaries.sh` `resolve(` check at :52 and the AC11 `NewEntry {` check at :58, and the task's `boundaries` Check), so they now scan only lines 1-57 of workspace.rs. The production region is clean today, but the guards can no longer catch a regression.
  route: implementer — move the test-only alias so that no column-0 `#[cfg(test)]` comes before the test module (e.g. define it inside `mod tests`).
  input: insert `let x = NewEntry { at: now };` and `let p = resolve(&d);` at workspace.rs line 2000 → both greps exit 1, check-boundaries.sh reports "ok".

## Wave stopped — step 2 — round 6 P2 (boundary-check bypass), past the round cap; a further fix run needs the user

## Wave resumed — user: one more fix run for the round-6 boundary-check P2

## Wave verified — 768b7eed2d8603086a6618a1c339f4d4e81c3e5d

## Outcome
Merged at 768b7ee (PASS + APPROVE: full workspace suite, fmt, clippy and boundaries clean; AC1-AC16 met), recorded on `record/161-service-outcome-history`. `wisp_service::outcome` and `wisp_service::history` now own check outcomes, the delivery table, `finish` and `record`; `storage::history` gains `Delivery`; the history list shows "outcome unknown". The AC1 golden is compared as parsed JSON (`preserve_order` changes key order in workspace builds). History recovers after a repaired `collection.toml` by re-reading the id for history only (Amendment 2). Amendment 4 gives each collection one current history store, so reads and writes agree across id recovery, concurrent recovery appends, and "Give this copy its own id", where history now follows the new id. Follow-ups are in PLAN.md.
