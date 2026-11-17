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
Apart from AC52, AC75 and the chip, no behaviour changes.

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
- [ ] AC5 `wisp_service::history::record(data, id, entry, body, bodies) -> Result<HistoryStore, RecordError>` takes the known `CollectionId` and never re-reads the id from disk; `RecordError` keeps the split "could not open the store" (history off) versus "append failed" (one entry dropped, notice), as `workspace.rs:2975-2986` does today.
- [ ] AC6 AC52: the GUI append passes `IdState::Known`'s id; when unknown, it goes through `resolve_id_then` first as today. Test: open a collection, change `id` in `collection.toml` on disk, send; the entry lands in the original id's store and the new id's store does not exist.
- [ ] AC7 `on_event` copies `nothing_sent` from `Event::Failed` and whether a `Headers` event arrived into `Terminal`/`Response`, calls `finish` at the terminal event, and the appended entry carries `delivery`. The GUI passes `expected_status = &[]` and does not display `status_ok`.
- [ ] AC8 AC75: under test, `Workspace::new` never calls `DataDir::resolve()`; a test-only constructor path injects the temp data dir before anything is resolved. A test asserts the workspace's data dir is the injected one.
- [ ] AC9 AC51 call site: the cookie load error reaches the log and the notice only as the fixed storage message (line and column), never file content. Test: a cookie file containing `CANARY` in a broken JSON position yields a notice without `CANARY`.
- [ ] AC10 The history list shows the text "outcome unknown" in the existing chip for entries with `delivery == Some(Unknown)`; a pure test covers the chip text for `Some(Unknown)`, `Some(Responded)` and `None`.
- [ ] AC11 `scripts/check-boundaries.sh` fails if `crates/app/src/workspace.rs` above its first `#[cfg(test)]` contains `NewEntry {`; the script passes on this branch.
- [ ] AC12 No other existing test expectation changes; `workspace.rs` hunks are edit (b) only; no file outside Owns changes.
- [ ] AC13 (Amendment 1) The moved test `history_records_hold_no_expected_or_header_values` keeps its `serde_json` assertion from the app original, unweakened.

## Checks
- `service-unit` command — AC1, AC2, AC3, AC5, AC13
  run: `cargo test -p wisp-service --locked 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([8-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `storage-history` command — AC4
  run: `cargo test -p wisp-storage --locked history 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `boundaries` command — AC11
  run: `scripts/check-boundaries.sh && sed '/^#\[cfg(test)\]/,$d' crates/app/src/workspace.rs | rg -n 'NewEntry \{|(^|[^:.\w])resolve\('; test $? -eq 1`
- `suite-and-lint` command — AC6, AC8, AC9, AC10, AC12
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `scope` command — AC12
  run: `git diff --exit-code main...HEAD -- crates/core crates/cli crates/storage/src/collection.rs crates/storage/src/cookies.rs crates/app/src/tab.rs crates/app/src/vault.rs crates/service/src/plan.rs crates/service/src/send.rs`
- `rewire` read — AC1, AC6, AC7, AC8, AC9, AC12, AC13
  how: the reviewer checks the golden JSON comes from pre-move code with a `Completed` terminal and is compared without overwriting any field but `at`, that `crates/service/Cargo.toml` changes only `[dev-dependencies]` (adds `serde_json`), the known-id path, the `nothing_sent`/headers plumbing, the AC75 constructor and that `workspace.rs` hunks stay within edit (b).

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

## Outcome
Merged at d04e2be (PASS + APPROVE, AC1-AC13), recorded on `record/161-service-outcome-history`. `wisp_service::outcome` and `wisp_service::history` now own check outcomes, the delivery table, `finish` and `record`; `storage::history` gains `Delivery`; the history list shows "outcome unknown". Two review rounds: the AC1 golden was rebuilt from a `Completed` terminal and compared as parsed JSON (not a string, because `preserve_order` changes key order in workspace builds). Follow-ups are in PLAN.md.
