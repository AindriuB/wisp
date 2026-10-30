# 20 — Add an end-to-end fixture test from disk through resolve

**Repo:** .
**Depends on:** 11, 14, 17, 18, 19
**Owns:**
- crates/storage/tests/**

## Goal
Prove that the M1 pieces compose with no window, using the spec's example
collection as a committed fixture. The test opens the collection, loads a
request and its environment, builds the scope, resolves, round-trips every file
byte-for-byte, and sees an external edit through the watcher. This is the
evidence that the M1 API is ready for M2 to freeze against.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md — section 2, the full layout and request example
- crates/storage/src/{collection,request,environment,watcher}.rs; crates/core/src/resolve/** (from 11, 14, 17, 18, 19)

## Acceptance
- [ ] `crates/storage/tests/fixtures/my-api/` contains the spec's layout. It has `collection.toml` with `format = 1`, a `baseUrl` var and a collection `Bearer {{token}}` auth. It has `environments/dev.toml` with `orgId = "acme"` and `dev.secrets.toml` with `token`. It has `users/folder.toml`, `get-user.toml`, `create-user.toml` and `create-user.body.json`, and every file has at least one comment.
- [ ] The fixture's `dev.secrets.toml` holds the placeholder value `fixture-not-a-secret`. The fixture also has `environments/.gitignore` containing `*.secrets.toml`, which is what 19 would write, so the round-trip test sees no new file. The secrets file is therefore committed with `git add -f`, and `git ls-files crates/storage/tests/fixtures` lists it.
- [ ] One test copies the fixture to a `tempfile` directory, opens it, loads `users/create-user.toml` and `dev`, calls `scope_for`, and resolves. It asserts that the URL is exactly `<baseUrl>/orgs/acme/files/report.pdf?notify=true`, that `authorization` is `Bearer fixture-not-a-secret`, that no `x-trace` header is present (it is disabled), and that the body equals the sidecar's bytes with `content-type: application/json`.
- [ ] One test loads and saves every request, folder, collection and environment file unchanged, then asserts every file in the copy is byte-identical to the fixture.
- [ ] One test starts a `Watcher` on the copy, rewrites `users/get-user.toml` from outside the API, and receives `Changed { path: "users/get-user.toml" }` within a 5 s bounded wait.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Fixing defects in 11–19 here. File each one as a successor task against the owning file.
- Engine I/O, since it is covered in 16
- Editing any file under `crates/*/src`

## Review round 1 — findings

Reviewed HEAD: 425a43318e8d8a4724193061c374b3835ae31164

- [P2] crates/storage/tests/resolve_integration.rs:118-148 (`watcher_sees_external_edit`) — passes even when no external edit happens: `copy_dir` writes `users/get-user.toml` just before `Watcher::start`, FSEvents delivers that create after the stream starts, and the loop accepts it.
  route: implementer — after `Watcher::start`, write a sentinel and drain events until its `Changed` arrives, then make the edit and wait for `users/get-user.toml`.
  input: replace the edit with a no-op → the test passed 3 of 5 runs.
- [STYLE] crates/storage/tests/resolve_integration.rs:140-142 — the wait loop sleeps 20 ms between `try_recv` calls.
  route: implementer — use `std::thread::yield_now()` inside the 5 s deadline loop (the idiom in watcher.rs's test `next()`).
  rule: docs/conventions.md, Tests: "no sleeps to wait for state".

Accepted residual (orchestrator, 2026-10-30): `create-user.body.json` has no comment because JSON cannot hold one; "every file has at least one comment" applies to the TOML fixture files only.
