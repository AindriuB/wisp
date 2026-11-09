# 85 — Close the storage gaps: sidecars aliasing request files, Windows reserved names, hard links

**Repo:** .
**Depends on:** none
**Owns:**
- crates/storage/src/request.rs
- crates/storage/src/environment.rs
- crates/storage/src/fs.rs
- docs/plan/tasks/85-storage-safety.md (the `## Notes` section only)

Branch `task/85-storage-safety`, worktree `.worktrees/wisp/85-storage-safety`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-85`. Commit on the
branch; do not push.

## Goal
Three gaps remain open in PLAN.md "Carry-forward: crates/storage". First,
**Request files (17)**: a sidecar can name a sibling request file, and save then
overwrites it, and a hard link passes the identity check. Second, **Environments
(19)**: `COM0`/`LPT0` are accepted. Third, **Residuals (58)**: a trailing-dot or
trailing-space `.toml` alias passes `request.rs:221`. Close all three, and give
each one a test that fails before the fix.

## Context
- crates/storage/src/request.rs:206-246 — `sidecar_path`. It is called by load (:255), by the save pre-check (:325-330) and by the body write (:463-470). The `.toml` check is at :221, the self-name check at :224-227, the canonicalize identity check at :229-236 (it misses hard links) and the symlink-inside check at :237-244 (a link to a sibling `b.toml` inside the folder passes).
- crates/storage/src/request.rs:900-1030 — the existing sidecar tests to follow (`sidecar_req`, `sidecar_symlink_to_request_file_rejected`).
- crates/storage/src/environment.rs:24-41 — `valid_name`, which has its own reserved-name check (COM1-9/LPT1-9 only).
- crates/storage/src/ops.rs:103-111 — `same_entry` (dev/ino on Unix) shows the identity idea. Do not edit ops.rs. If you need the helper, copy it or move a shared version into fs.rs.

## Contract
A sidecar name is rejected with `StorageError::Invalid` (key `body.file`) on load and on save, before any read or write, when any of these holds:

| Rule | Example |
|---|---|
| ends with `.` or ` ` (Windows strips these, so the name aliases another) | `b.toml.`, `b.toml `, `body.json.` |
| its stem is a Windows reserved device name (below) | `CON.json`, `com0.txt`, `LPT¹.xml` |
| it resolves, through a symlink, to a file whose name, ignoring case, ends with `.toml` | `body.txt -> b.toml` |
| it is the same file-system entry (Unix dev/ino) as the request file or as any `*.toml` file in the request's folder | a hard link `body.txt` to `a.toml` or `b.toml` |

**Windows reserved device names**, one helper in fs.rs used by environment.rs and request.rs: `CON PRN AUX NUL COM0-COM9 LPT0-LPT9 COM¹ COM² COM³ LPT¹ LPT² LPT³`. The match ignores ASCII case. The stem is the name up to the first `.`, with trailing spaces removed.

Environment names reject the full set; today they miss COM0 and LPT0. Their other rules do not change.

Windows hard links: std has no stable file-identity API on Windows, so the hard-link rule applies on Unix only. Notes must say so; do not add a dependency.

## Acceptance
- [ ] AC1 Each contract row has tests named `sidecar_rejects_*`. Each covers load, and save leaves both the request file and the aliased file byte-identical. The hard-link tests are `#[cfg(unix)]`. At least 6 such tests: trailing dot, trailing space, reserved stem, symlink to a sibling `.toml`, hard link to the request, hard link to a sibling.
- [ ] AC2 A test `reserved_device_names_cover_the_full_set` checks every name in the set (mixed case, with and without an extension) and `COM10`, `CONSOLE` and `LPT` as not reserved. A test `environment_rejects_com0_and_lpt0` checks environment names.
- [ ] AC3 The tests are committed before the fix, in their own commit (subject `85: failing tests for …`). At that commit, the AC1 and AC2 tests that cover new rules fail. Notes names the commit and which tests failed there.
- [ ] AC4 The existing sidecar tests still pass, including `sidecar_symlink_outside_folder_rejected_inside_allowed`: a symlink to a non-`.toml` file inside the folder is still allowed.
- [ ] AC5 No new dependency; the `Cargo.toml` files and `Cargo.lock` are unchanged.
- [ ] AC6 `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Checks
- `storage-new` command — AC1, AC2. cargo test does not write JUnit XML here, so the command asserts the pass count itself (≥ 8 on Unix).
  run: `cargo test -p wisp-storage --lib -- sidecar_rejects_ reserved_device_names_cover_the_full_set environment_rejects_com0_and_lpt0 2>&1 | grep -E '^test result: ok\. ([8-9]|[1-9][0-9]+) passed'`
- `failing-first` command — AC3
  run: `git checkout <failing-tests commit> && cargo test -p wisp-storage --lib -- sidecar_rejects_ reserved_device_names environment_rejects_com0; test $? -ne 0; git checkout task/85-storage-safety`
- `storage-tests` command — AC4
  run: `cargo test -p wisp-storage`
- `no-new-deps` command — AC5
  run: `git diff --exit-code main...HEAD -- Cargo.toml Cargo.lock crates/storage/Cargo.toml`
- `lint` command — AC6
  run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

## Out of scope
- Reserved device names for request and folder names created through ops.rs (`CON.toml`). This is not in the carry-forward list; it is raised with the user separately.
- `::$DATA` aliases, the drive-prefix check, the sidecar symlink check-then-use race, and `safe_sidecar`'s `to_lowercase` (Residuals (67)).
- Two requests sharing one sidecar body. Saving one rewrites the shared body, which is the existing behaviour.
- ops.rs, collection.rs and any wisp-core file.

## Notes
