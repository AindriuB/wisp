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

## Checks
- `compare-tests` command — AC1, AC3, AC4, AC5, AC6, AC7, AC8
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
