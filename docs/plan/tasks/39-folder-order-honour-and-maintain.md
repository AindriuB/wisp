# 39 — Honour and maintain folder `order` entries written as file names

**Repo:** .
**Depends on:** none
**Owns:**
- crates/storage/src/collection.rs
- crates/storage/src/ops.rs
- crates/storage/tests/folder_order.rs (new)
- crates/app/src/ui/sidebar.rs (only if the investigation shows the sidebar reorders rows)

## Goal
In the running app the sidebar shows Create user before Get user although `users/folder.toml` has `order = ["get-user.toml", "create-user.toml"]`, and renaming get-user.toml to fetch-user.toml then deleting create-user.toml left that `order` unchanged. Find which layer is at fault, reproduce it in a test against the `my-api` fixture, and make display order and `order` maintenance on rename, move and delete work for the fixture's form.

## Context
- crates/storage/tests/fixtures/my-api/users/folder.toml:2 — entries carry the `.toml` extension
- crates/storage/src/collection.rs:253-327 — `scan_dir` keys entries by stem and matches `order` names exactly (`n == name`), so `get-user.toml` never matches stem `get-user` (likely cause; confirm)
- crates/storage/src/collection.rs:376,395-420 — existing tests use extension-less entries only
- crates/storage/src/ops.rs:277-330 — `plan_order` finds the entry by exact string; `apply_edit` keeps decor on Replace
- crates/storage/src/ops.rs:427-680 — create, rename, move, delete call sites of `plan_order`
- crates/storage/src/ops.rs:809-975 — task 23's order tests (extension-less)
- crates/app/src/ui/sidebar.rs:65 and its `flatten` — check whether it keeps `CollectionTree` order or re-sorts
- crates/storage/tests/resolve_integration.rs:20 — `copy_dir` helper pattern; copy it into the new test file rather than editing that file
- docs/plan/specs/2026-10-29-http-client-v1.md:106-107 — "Order via `order = [...]` in `folder.toml`; unlisted items sort alphabetically"
- HISTORY 2026-10-30 "Storage-to-resolve integration test and fixture collection" — the fixture is round-tripped byte-for-byte and `dev.secrets.toml` is force-added; never modify the fixture in place
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-39`

## Acceptance
- [ ] The PR states which layer was at fault (storage, sidebar or both), citing `path:line`.
- [ ] New `crates/storage/tests/folder_order.rs` copies `my-api` into a tempdir and asserts, failing on `main` before the fix: (a) `open_collection` lists `users` children as Get user then Create user; (b) after `rename_request` of `users/get-user.toml` to `fetch-user`, `users/folder.toml` reads `order = ["fetch-user.toml", "create-user.toml"]` with the comment line kept and the tree lists Fetch/Get user first; (c) after `delete_node` of `users/create-user.toml`, `order` is `["fetch-user.toml"]`; (d) after `move_node` of a request out of `users`, its entry is removed from `users/folder.toml`.
- [ ] An `order` entry matches a request whether written as `stem` or `stem.toml`; edits keep the form the entry was written in (an extension-less entry stays extension-less, as task 23's existing tests assert). Unit tests in `collection.rs` cover both forms in one list.
- [ ] Task 23's existing ops tests pass unchanged.
- [ ] If the sidebar reorders rows, a unit test in `sidebar.rs` asserts `flatten` preserves `CollectionTree` order; otherwise `sidebar.rs` is untouched.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-39 cargo test --workspace` passes; `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean; `git status` shows no change under `crates/storage/tests/fixtures/`.
- [ ] Hand check (orchestrator, temp copy of `my-api`): the sidebar shows Get user above Create user; renaming Get user's file to fetch-user then deleting Create user leaves `users/folder.toml` `order = ["fetch-user.toml"]`.

## Out of scope
- Changing the fixture or `resolve_integration.rs`.
- Drag-and-drop reordering, or writing new `order` entries on create (only maintain existing ones).
- Other collection-tree residuals in PLAN (walk-wide visited set, outside-root symlinks, broken `folder.toml` failing the open).
- The sidebar optional items (create inside a collapsed folder, "No environment" clash, collapsed-prune test).
