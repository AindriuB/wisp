# 188 — Compare two history entries, with a collection ignore list

**Repo:** .
**Round:** 0
**Depends on:** 183
**Owns:**
- crates/service/src/compare.rs (new; may become `compare/**`)
- crates/service/src/lib.rs (`pub mod compare;` only)
- crates/service/Cargo.toml (`serde_json` moves from dev-dependencies to dependencies)
- Cargo.lock (the `wisp-service` dependency list only)
- crates/storage/src/collection.rs (the `[compare]` table read and write only)
- crates/storage/tests/compare_ignore.rs (new)

## Goal
Add the pure comparison that the compare view (task 192) renders: a status
diff, a header diff and a structural JSON body diff between two history
entries. It applies a per-collection ignore list stored in `collection.toml`.
It must state missing or truncated evidence explicitly and never call an
incomplete side equal.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 2": the behaviour, the evidence wording, the ignore syntax, the limits and the model sketch.
- This brief assumes open questions 4 and 5 are answered as recommended (`collection.toml`, no built-ins). The orchestrator amends it if not.
- Task 183's `HistoryEntry.evidence` (`crates/storage/src/history.rs`). `StoredEntry` (:386-395) has `response_headers`, `response_redacted` and `body`.
- crates/storage/src/collection.rs:161-201: `load_collection_defaults`, `with_root_lock`, `save_collection_defaults` (in-place `toml_edit` writes).
- Worktree: `./.claude/scripts/wt-new.sh . 188 compare-engine-ignore-list`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-188`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `IgnoreList::parse(&[String])` accepts `header:<name>` (matched case-insensitively) and `body:<RFC 6901 pointer>`, where a `*` segment matches any one key or index and a match also covers the whole subtree. It returns problems (index plus reason) for an empty name, a pointer not starting with `/`, a bad `~` escape or an unknown prefix. Valid patterns are still used.
- [ ] AC2 Storage `load_compare_ignore(root)` and `save_compare_ignore(root, &[String])` read and write `[compare] ignore` in `collection.toml` under `with_root_lock`, keeping comments and other tables. A missing table is an empty list. More than 256 entries, an entry over 1,024 bytes, or a non-string value is an error naming the key. `save_collection_defaults` keeps an existing `[compare]` table. Tests in `tests/compare_ignore.rs`, prefix `compare_ignore_`.
- [ ] AC3 Status: equal, changed (both values), or missing on a side.
- [ ] AC4 Headers:
  - names compare case-insensitively, and repeated headers compare as ordered value lists;
  - results are added, removed or changed;
  - a name in either side's `response_redacted` is reported as redacted and its values are not compared;
  - `header:` patterns hide matches and count them;
  - `evidence.headers == false` on a side gives a "headers not recorded" note.
- [ ] AC5 Body, when both sides are stored complete and both parse as JSON:
  - changes are added, removed, changed or type-changed at pointer paths;
  - arrays compare by position;
  - integers compare exactly, and if either side is a float both compare as f64;
  - `body:` patterns hide matching subtrees and count them.

  When either side is not JSON, the result is `NotJson { equal }`, with byte equality.
- [ ] AC6 Evidence: when either side's body is `truncated`, `off`, `unreadable` or `no_response`, or has `evidence: None` with `body_stored == false` or `body_truncated == true`, the body result is `Incomplete` with each side's reason. An incomplete side never yields `Equal`. Pre-0.5.0 entries are marked "recorded before 0.5.0".
- [ ] AC7 Limits: at most 1,000 changes are listed, with the total count kept. Values are cut to 256 characters at a char boundary, with a flag. Nesting deeper than 128 gives `TooDeep` and no partial list. The work is linear in nodes, with no quadratic lookups.
- [ ] AC8 Compare tests in `crates/service/src/compare.rs`, prefix `compare_`, cover AC1 and AC3 to AC7. Each evidence reason, the pre-0.5.0 fallback, `*` and subtree patterns, a header redacted on one side, a float/int pair, a type change and the 1,000 cap each have a test. The cap test uses data, not timing.
- [ ] AC9 No file outside Owns changes. No new crate in `Cargo.lock`. fmt, clippy and the workspace suite pass.
- [ ] AC10 (Amendment 1) Stored body unavailable: a new body evidence variant (`BodyKind::StoredUnavailable` or similar) applies when a side's evidence says `Stored` or `Truncated` but its `StoredEntry` could not be loaded (`None`) or its `.bin` file is missing. Its text is "Stored body unavailable (deleted or unreadable)".
  - That side is incomplete: the body result is `Incomplete` with this reason, never `Equal` or `NotJson { equal: true }`.
  - When the `StoredEntry` is `None`, that side's headers count as not recorded (the AC4 note), not as an empty header list.
  - A missing `.bin` is detected without changing storage: the side's constructor is told whether the body file exists (callers use `HistoryStore::body_path`, `crates/storage/src/history.rs:1504`). An empty `body` alone is not taken as missing.
  - Tests, prefix `compare_`: `from_entry(e, None)` on both sides is not `Equal` and reports headers not recorded; two entries whose `.bin` was deleted from a real `HistoryStore` in a tempdir are not `Equal`.

## Checks
- `compare-tests` command — AC1, AC3, AC4, AC5, AC6, AC7, AC8, AC10
  run: `cargo test -p wisp-service --locked --lib compare_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[4-9]|[2-9][0-9]) passed; 0 failed; 0 ignored'`
- `ignore-storage` command — AC2
  run: `cargo test -p wisp-storage --locked --test compare_ignore compare_ignore_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([5-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `no-new-crate` command — AC9
  run: `test "$(git diff main...HEAD -- Cargo.lock | grep -c '^+name = ')" = 0`
- `scope` command — AC9
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/service/src/compare' -e '^crates/service/src/lib.rs$' -e '^crates/service/Cargo.toml$' -e '^Cargo.lock$' -e '^crates/storage/src/collection.rs$' -e '^crates/storage/tests/compare_ignore.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC9
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `linear` read — AC7
  how: the reviewer checks that the traversal visits each node once, that header grouping uses maps, and that no pattern match scans the whole list per node beyond the pattern count.

## Out of scope
- Any GUI (task 192), any CLI command, and copy or export.
- Text or line diff, and keyed or LCS array matching.
- Changing what history stores (task 183).
- `wisp-cli validate` checks for `[compare]`.
- Changing `HistoryStore::load` or `StoredEntry` in `crates/storage/src/history.rs` to flag a missing `.bin`.

## Amendment 1 (2026-11-20)
Added AC10, a "stored body unavailable" evidence state, and added AC10 to the `compare-tests` check. Reason: code review round 1 orchestrator finding, decided by the user. A side whose `StoredEntry` cannot be loaded (`from_entry(e, None)`), or whose `.bin` is missing (for example after eviction), has an empty body and no headers, so two such sides compared as `NotJson { equal: true }`. That broke AC6 and spec Feature 2 ("an incomplete side never yields Equal"), and the spec's evidence table (docs/plan/specs/2026-11-19-wisp-0-5-0.md:93-101) had no reason for the case. The missing `.bin` is detected through `HistoryStore::body_path`, so Owns is unchanged.

## Wave stopped — step 2 — orchestrator finding: compare with an unloadable StoredEntry (Side::from_entry(e, None) or a missing .bin) yields Equal; spec evidence table (:93-101) has no reason for it

## Wave resumed — user decided "Yes, add it" (2026-11-20): AC10 "stored body unavailable" via Amendment 1; one fix round

## Review round 1 — findings

Reviewed HEAD: 3ab099d2a8da69397c4a603116a909d046fb1217

- [P2] crates/storage/src/collection.rs:286-293 (save_compare_ignore_locked, `compare.insert("ignore", …)`): every save deletes the comments attached to the `ignore` key, both the comment line above it and the comment after it on the same line. AC2 and docs/conventions.md:96 both say comments must be kept.
  route: implementer — replace the value in place and keep its decor, like `write_order` at crates/storage/src/collection.rs:122-134. Add a test with `# lead` above and `# trail` after the `ignore` key.
  input: collection.toml `format = 1\n[compare]\n# lead\nignore = ["header:a"] # trail\n`, then `save_compare_ignore(root, &["header:b"])`  →  the file becomes `format = 1\n[compare]\nignore = ["header:b"]\n`, with both `# lead` and `# trail` gone.
- [P2, now AC10] crates/service/src/compare.rs:128-137 with 359-391 and 298-321: if `Side::from_entry(entry, None)` is called because the StoredEntry could not be loaded, or its `.bin` is missing, the side gets an empty body and no headers even when its evidence says `Stored` and headers were recorded. Two such sides come out as "equal".
  route: implementer — implement AC10 (Amendment 1) exactly.
  input: a HistoryEntry `{status:200, body_stored:true, evidence:{body:"stored",headers:true}}`, `compare(&Side::from_entry(&e,None), &Side::from_entry(&e,None), &IgnoreList::default())`  →  `body: NotJson { equal: true }` and `headers.changes: []`.

Follow-ups, not this round:
- [P3] JSON nested exactly 128 deep returns TooDeep, because of serde_json's limit, while AC7 says beyond 128 (compare.rs:421-427, 506-509).
- [P3] An entry from before 0.5.0 (`evidence: None`) with `status: None` doesn't flag its headers as not recorded (compare.rs:300-301).

## Review round 2 — findings

Reviewed HEAD: e7a052c650ba2bea419a1dff0985de77ecbf7e6a

- [SPEC] crates/service/src/compare.rs:226-234 — `BodyKind::text()` uses its own wording for four reasons instead of the spec's evidence wording (docs/plan/specs/2026-11-19-wisp-0-5-0.md:97-101).
  route: implementer (orchestrator decision 2026-11-20: use the spec table strings verbatim, lowercase as the spec writes them; StoredUnavailable keeps AC10's exact text). Required strings:
    - Truncated → "body cut at 1 MiB; not compared"
    - Off → "body not stored (history bodies off, or a `wisp-cli run` entry)" (the backticks are part of the spec text; render them literally)
    - Unreadable → "body could not be read when recorded"
    - NoResponse → "no response body (request failed or was cancelled)"
    - StoredUnavailable → "Stored body unavailable (deleted or unreadable)" (AC10)
  Add one test asserting each string.
- [STYLE] crates/service/src/compare.rs:906 — no blank line between the end of the previous test (:905) and `fn stored_entry`; every other item in the tests module has one.
  route: implementer

Note for task 192's brief (scribe/planner, later): build a `Side` only from a `StoredEntry` returned by `HistoryStore::load`, never from `load_without_body`, which returns an empty body even when the `.bin` exists.

## Attempt 3 — failed

Tester FAIL at c3fbb927fcf3affc8a56cbcc74a935a341c8f260: `cargo fmt --all --check` exits 1. rustfmt wants the long `assert_eq!` lines wrapped in `body_kind_text_matches_spec_wording` (crates/service/src/compare.rs ~:906 and ~:914). All tests, clippy, scope and no-new-crate pass. Fix: run `cargo fmt --all`; nothing else.

## Wave verified — 3bfa82102036a273f8f590b21b66c544933ca0c4

(Reviewer APPROVE at c3fbb92; 3bfa821 is rustfmt-only on one test, read by the orchestrator. Tester PASS at 3bfa821.)

## Outcome
Tester PASS and reviewer APPROVE at 3bfa82102036a273f8f590b21b66c544933ca0c4 after three attempts (two review rounds, one rustfmt-only fix) and Amendment 1 (AC10, "stored body unavailable"). Task 192 renders this comparison. Follow-ups are in `docs/plan/PLAN.md` under "Follow-ups from task 188".
