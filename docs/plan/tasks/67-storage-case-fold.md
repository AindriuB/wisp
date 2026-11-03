# 67 — Unify reserved-name case folding in storage ops on fold_case

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/storage/src/ops.rs
- docs/plan/tasks/67-storage-case-fold.md (the `## Notes` section only)

**Build:** run every cargo command with
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-67`.

## Goal
`clean()` rejects a leading dot or `environments` using `to_lowercase`, while
`clean_node` repeats the same check with `fold_case`. Paths that go only through
`clean()` (move destinations, create targets) therefore miss wide case-fold
aliases: on Linux `move_node(r, "a.toml", "environment\u{17f}")` gives Io NotFound
where `delete_node` of the same name gives Invalid. Make `clean()` the single
fold_case check, drop the duplicate, and close the test gaps 62 left.

## Context
- crates/storage/src/ops.rs:69-89 — `clean()`; :83-87 is the `to_lowercase` check.
- crates/storage/src/ops.rs:160-185 — `clean_node`; :172-178 is the duplicate
  `fold_case` block and its comment. The `collection.toml`/`folder.toml` check at
  :179-182 was the actual Linux fix from 62 and stays.
- crates/storage/src/ops.rs:249-257 — `fold_case` and `reserved_at_root`.
- crates/storage/src/ops.rs:1257-1278 `delete_refuses_dot_and_reserved_names_any_case`,
  :1281-1292 `move_into_reserved_any_case_rejected`, :1390-1401
  `delete_refuses_wide_case_fold_alias` (the conditional assert at :1397-1399),
  :1494 `reserved_names_fold_wide_case` — the existing test shapes to follow.
- docs/plan/HISTORY.md — `grep '## 2026-11-03 — Linux CI green: reserved names rejected before any filesystem lookup'`
  for why the textual checks come before any lookup.

## Acceptance
- [ ] `clean()` compares the first component with `fold_case(..)` (both the
      leading-dot and the `ENV_DIR` test); no `to_lowercase` reserved-name check
      remains in ops.rs outside `fold_case` itself.
- [ ] The first-component block in `clean_node` (:172-178) is removed, and the
      comment above the remaining checks no longer implies that block was the
      Linux fix. Error kinds are unchanged: every path that returned
      `StorageError::Invalid` before still does.
- [ ] New test: `move_node(r, "a.toml", "environment\u{17f}")` returns
      `StorageError::Invalid` on every OS and leaves the tree snapshot unchanged.
- [ ] New tests: `rename_request`, `rename_folder` and `move_node` each called with
      a **source** spelling that is reserved and does not exist on disk (for example
      `Environments`, `environment\u{17f}`, `.WISP`, `f/Folder.toml`) return
      `StorageError::Invalid`, not Io NotFound, and change nothing.
- [ ] `delete_refuses_wide_case_fold_alias` asserts `Invalid` unconditionally
      (the `if r.join(..).exists()` guard is gone) and still asserts the snapshot.
- [ ] Each new test fails with its fix reverted: state in `## Notes` which test
      was run red against the old `clean()` and what it returned.
- [ ] `cargo test -p wisp-storage`, `cargo clippy --workspace --all-targets -- -D warnings`
      and `cargo fmt --all --check` pass.
- [ ] Binary size: storage-only logic change; no measurement required. Note in
      `## Notes` if `cargo build --release -p wisp-app` size moves by more than 4 KiB.
- [ ] Do not open a PR or push; commit on `task/67-storage-case-fold` only.

## Out of scope
- `crates/storage/src/request.rs` (the 58 trailing-dot alias and the unreachable
  name check at :224-226) and `environments.rs`.
- Windows `::$DATA` aliases, symlink races, folder-order duplicates (17, 39).
- `case_alias_sidecar_counts_as_shared` being vacuous on case-sensitive file systems (23).
- docs/, PLAN.md, HISTORY*.md (scribe).

## Notes

- Red check: reverted `clean()` to `to_lowercase` (with the clean_node block gone) and ran
  `cargo test -p wisp-storage --lib` on macOS/APFS: all 139 still passed, because APFS
  resolves `environment\u{17f}` to `environments` and `confine` rejects it. The failure
  (Io NotFound from `move_node`/`rename_*`) only reproduces on a case-sensitive file
  system; no Linux runner (Docker daemon down) was available, so the red run is not shown.
  Reserved-source cases using ordinary case (`Environments`, `.WISP`) were never red.
- Also switched the two reserved-name `to_lowercase` checks in `confine` to `fold_case`
  so none remain outside `fold_case`.
- Binary size: not measured (storage-only logic change).
- Linux red check: CI run 41651467164 (`check (ubuntu-latest)`) on a throwaway branch with
  `clean()` reverted to `to_lowercase` and the new tests kept. Three tests failed:
  `ops::tests::delete_refuses_wide_case_fold_alias`,
  `ops::tests::move_into_wide_case_fold_alias_rejected`,
  `ops::tests::reserved_source_spellings_rejected_not_notfound`
  (the unconditional Invalid assertions got Io NotFound). This supersedes the macOS note above.
