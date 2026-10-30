# 23 — Add create, rename, move and delete operations for collection nodes

**Repo:** .
**Depends on:** none
**Owns:**
- crates/storage/src/ops.rs (new)
- crates/storage/src/lib.rs (insert `pub mod ops;` only)
- crates/storage/src/error.rs (insert new variants only)

## Goal
The sidebar's create/rename/move/delete must map to files. Storage gets one module that performs these operations on request files (with their sidecar bodies) and folders, keeps `order = [...]` lists in `folder.toml`/`collection.toml` consistent, and never overwrites or escapes the collection root.

## Context
- crates/storage/src/collection.rs:159-230 — `load/save_folder_defaults`, `load/save_collection_defaults` (the `order` field)
- crates/storage/src/request.rs:91-485 — `load_request`/`save_request`, sidecar name and identity rules
- crates/storage/src/fs.rs:113-120 — `atomic_write`
- crates/storage/src/error.rs:7-30 — `StorageError`
- PLAN carry-forward (17): a sidecar may name a sibling request file; renames here must not clobber it
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-23`.

## Acceptance
- [ ] `wisp_storage::ops` exposes, all paths relative to `root` and all returning `Result<_, StorageError>`:
  `pub fn slug(name: &str) -> String`;
  `pub fn create_request(root: &Path, dir: &Path, name: &str) -> Result<PathBuf>` (writes a request with `name`, `method = "GET"`, `url = ""` to `<dir>/<slug>.toml`, appending `-2`, `-3`… when taken);
  `pub fn create_folder(root: &Path, parent: &Path, name: &str) -> Result<PathBuf>`;
  `pub fn rename_request(root: &Path, rel: &Path, new_name: &str) -> Result<PathBuf>` (sets `name` in place via `toml_edit`, renames the file to the new slug, renames a sidecar body along with it and rewrites `body.file`);
  `pub fn rename_folder(root: &Path, rel: &Path, new_name: &str) -> Result<PathBuf>`;
  `pub fn move_node(root: &Path, rel: &Path, new_parent: &Path) -> Result<PathBuf>` (request plus sidecar, or folder);
  `pub fn delete_node(root: &Path, rel: &Path) -> Result<()>` (request plus sidecar, or folder recursively).
- [ ] Rename, move and delete remove the old name from the parent's `order` list (`folder.toml`, or `collection.toml` at the root) and rename replaces it in place; comments in those files survive (test compares the untouched lines byte for byte).
- [ ] No operation overwrites an existing file or directory: a target collision returns a new `StorageError::Exists { path }` and leaves the disk unchanged (tested for request, sidecar and folder collisions).
- [ ] Operations reject, with `StorageError::Invalid`, any `rel`/`dir`/`new_parent` that is absolute, contains `..`, resolves outside `root`, or names the root itself, `environments/`, `.wisp/` or `collection.toml`; moving a folder into itself or a descendant is rejected.
- [ ] A rename whose new sidecar name equals an existing sibling request file fails with `Exists` (covers the PLAN 17 clobber case).
- [ ] Every operation's effect is visible to `open_collection` in a test (the tree shows the new name/location, the old one is gone).
- [ ] Tests use temp directories; `cargo test -p wisp-storage` passes with all tests present at `49da271` plus the new ones.
- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- Moving or deleting environments, or editing environment files.
- A trash/undo facility (delete is permanent; the UI confirms first).
- The other storage carry-forwards in PLAN (method parser unification, symlink visit order, broken `folder.toml` failing the open).
- Anything in `crates/app` or `crates/core`.

### Amendment (2026-10-30, ops safety)

Decisions (user, 2026-10-30):
1. **Sidecars on delete/move.** `delete_node` and `move_node` delete/move a request's sidecar only if it is a normal file: name not ending in `.toml` (case-insensitive), not a dot-file, not `collection.toml`/`folder.toml`, not a symlink pointing outside the request's folder, and no other request in the same folder references the same sidecar name. Otherwise the request alone is deleted/moved and the sidecar file is left untouched (no error).
2. **Dot-entries and case.** Every create/rename/move/delete refuses (`StorageError::Invalid`) any path whose first component starts with `.` (e.g. `.git`, `.github`, `.wisp`), and refuses creating or renaming a node to a name starting with `.`. All reserved-name comparisons (`collection.toml`, `folder.toml`, `environments`, dot-entries) are case-insensitive.

Reason (code review of task 23): a cloned collection's `body.file` could make deleting one request remove `collection.toml`, `folder.toml` or another request; `delete_node(root, ".git")` deleted the repository; case-sensitive guards let `Environments`/`.WISP`/`Collection.toml` through on macOS.

Additional acceptance:
- [ ] Deleting a request whose sidecar is `collection.toml`, `b.toml` (a sibling request), `.x`, or a sidecar shared with another request in the same folder returns `Ok` and leaves that file in place, byte for byte (one test per case); moving such a request likewise leaves the sidecar where it was.
- [ ] `delete_node(root, p)` for each `p` in `".git"`, `".WISP"`, `"Environments"`, `"Collection.toml"`, `"Folder.toml"` returns `StorageError::Invalid` and nothing on disk is removed (test checks the entry still exists).
- [ ] `move_node(root, rel, "ENVIRONMENTS")` returns `StorageError::Invalid` and the node stays where it was.
- [ ] `create_request`/`create_folder`/`rename_request`/`rename_folder` with a name starting with `.` return `StorageError::Invalid`.

### Amendment (2026-10-30, ops safety — rename)

Orchestrator note: this applies the user's existing decision 1 above ("leave unusual sidecars in place") to `rename_request`. No new user decision.

1. **Sidecars on rename.** `rename_request` renames a request's sidecar only if it is a safe sidecar under the decision-1 rule: a normal file whose name does not end in `.toml` (case-insensitive), is not a dot-file, is not `collection.toml`/`folder.toml`, is not a symlink pointing outside the request's folder, and is not shared with another request in the same folder. Otherwise only the request is renamed: its `body.file` still names the unchanged sidecar, and no error is returned.
2. **"Shared" means same file.** In `delete_node`, `move_node` and `rename_request`, a sidecar counts as shared when another request in the same folder references the same file. Compare the canonical paths (file identity), not the `body.file` strings, so case aliases and symlink aliases count as shared.

Reason (code review of task 23, round 2): `rename_request` renamed a body that another request also used, so that request lost its body. It also renamed a sibling request (`a.x.toml`) as if it were a body file.

Additional acceptance:
- [ ] Renaming a request whose sidecar is shared with another request in the same folder returns `Ok`, leaves the sidecar's name and content unchanged, and the other request still loads with its body (test).
- [ ] Renaming a request whose sidecar is a sibling `.toml` request file (e.g. `a.toml` with `body.file = "a.x.toml"`) returns `Ok`, leaves `a.x.toml` unchanged, and `a.x.toml` still loads as a request (test).
- [ ] Renaming a request whose sidecar is a case alias of another request's sidecar (e.g. `a.toml` file=`"Body.json"`, `b.toml` file=`"body.json"`) returns `Ok`, leaves `body.json` unchanged, and `b.toml` still loads with its body (test); deleting or moving `a.toml` in the same setup likewise leaves `body.json` in place.
- [ ] After any of the renames above, the renamed request's `body.file` still names the unchanged sidecar and the request loads.

## Review round 1 — findings

Reviewed HEAD: cb3f84913fa347d940c9f4e2d708e3132a65011f

- [P1] crates/storage/src/ops.rs:151-165,360,400 — `ensure_free` treats a target as free whenever its canonical path equals the source's, so renaming a symlinked request onto the file it points to replaces the real file with a self-referencing symlink (content lost). Same for sidecars. The "hard links to self" comment is wrong (canonicalize doesn't resolve hard links).
  route: implementer — `same_file` only accepts a genuine case/normalisation alias (same parent directory, names equal case-insensitively/normalised), not two paths that canonicalise to the same place.
  input: `b.toml` real, `a.toml -> b.toml`; `rename_request(root, "a.toml", "B")` → Ok, `b.toml` becomes a symlink to itself (FilesystemLoop).
- [P1] crates/storage/src/ops.rs:122-160 (clean_node), 388-394 (rename_request) — `clean_node` checks only the parent against the root, so rename/move on a request that is a symlink to an outside file writes the new name into that file via `atomic_write`.
  route: implementer — rename and move must refuse (`Invalid`) a node whose own canonical path is outside the root; delete may still unlink a symlinked node.
  input: `v.toml -> /elsewhere/victim.toml`; `rename_request(root, "v.toml", "v")` → Ok, outside file rewritten.
- [P2] crates/storage/src/ops.rs:83-90,138-146 — reserved-name guards are case-sensitive; on case-insensitive macOS `delete_node(root, "Environments" | ".WISP" | "Collection.toml")` deletes the real entries (secrets included); `Folder.toml` and `move_node(..., "ENVIRONMENTS")` get through too.
  route: implementer — compare case-insensitively. Also apply the user decision below on dot-entries.
- [P2] crates/storage/src/ops.rs:512-524 (delete_node), 464-472 (move_node) — delete/move act on whatever `body.file` names, so a crafted request can make delete remove `collection.toml`, `folder.toml` or another request. Settled by Amendment (2026-10-30, ops safety) — implement it.
  route: implementer
- [P3] crates/storage/src/ops.rs:388-398 — `rename_request` writes the new `body.file` before renaming the sidecar; a failed sidecar rename leaves the request pointing at a missing file.
  route: implementer — rename the sidecar first (undo if the request write fails), then the request file, then `order`.
- [P3] crates/storage/src/ops.rs:446-458, 418-428, 360 — the folder-vs-request-stem collision check is uneven: moving/renaming a folder onto an existing `n.toml` stem, or renaming a request onto an existing folder name, is allowed.
  route: implementer — apply the same collision check in all four operations.
  input: `users.toml` at root, `move_node(root,"x/users","")` → Ok("users"); `users3.toml` exists, `rename_folder(root,"users2","users3")` → Ok.

## Review round 2 — findings

Reviewed HEAD: 655e7790e74fa685db4be22e68a73211ae8318dd

- [P1] crates/storage/src/ops.rs:69-90, 101-108, 131-160, 111-119 — reserved and dot-entry guards check only the literal `rel`/`new_parent`; a symlink inside the root pointing into `.git`, `environments/`, `.wisp/` or at `collection.toml` gets past them.
  route: implementer — after canonicalising, check the first component of the resolved path relative to the root, and the resolved file name, for the parent, any followed node and `new_parent`; refuse with Invalid.
  input: `g -> .git`, `delete_node(root,"g/objects")` → removes `.git/objects`; `envlink -> environments`, `move_node(root,"envlink/dev.secrets.toml","")` → moves secrets to root; `v.toml -> collection.toml`, `rename_request(root,"v.toml","x")` → rewrites collection.toml's name; `v.toml -> environments/prod.secrets.toml` → writes into the secrets file.
- [P2] crates/storage/src/ops.rs:85, 150-153, 206 — reserved names compared with `to_lowercase()`, but APFS case folding is wider (`environmentſ`, U+017F, opens `environments`); Windows aliases (`environments.`, `ENVIRO~1`) likewise.
  route: implementer — compare the resolved (canonical) name instead of the literal text.
  input: `delete_node(root, "environmentſ")` → deletes `environments/`.
- [P2] crates/storage/src/ops.rs:366 — `safe_sidecar`'s "shared" check compares `body.file` strings; case or symlink aliases of the same file aren't detected.
  route: implementer — compare by file identity (canonical path).
  input: `a.toml` file="Body.json", `b.toml` file="body.json"; delete `a.toml` → `body.json` deleted under b.
- [P2] crates/storage/src/ops.rs:446-460 — `rename_request` renames the sidecar without the decision-1 checks. Settled by Amendment (2026-10-30, ops safety — rename): apply the same safe-sidecar rule to rename.
  route: implementer
  input: `a.toml` and `b.toml` share `a.body.json`; rename a → c renames the shared body; `a.toml` with file="a.x.toml" renames a sibling request.
- [P3] crates/storage/src/ops.rs:583-586 — `move_node` moves the sidecar first and doesn't undo it if the request rename fails.
  route: implementer — same undo pattern as `rename_request`.

## Review round 3 — findings

Reviewed HEAD: 759d445a8afa25b5bee24758f2a616048528f1aa

- [P2] crates/storage/src/ops.rs:1385-1424 (`rename_leaves_unsafe_sidecars`) and `case_alias_sidecar_counts_as_shared` — the rename tests pass without the safe-sidecar check: four of five cases don't start with `a.` so rename never tries them; the round-2 input (shared `a.body.json`) is untested; the case-alias test's rename step uses `Body.json` and its delete step is missing.
  route: implementer — use stem-prefixed sidecars (shared `a.body.json`; case alias `a.toml` file=`A.body.json` / `b.toml` file=`a.body.json`) and add the delete step. Show the test fails with `safe_sidecar(&abs, sc).is_some()` (ops.rs:487) replaced by `!sc.to_lowercase().ends_with(".toml")`.
- [P2] crates/storage/src/ops.rs:1427-1448 (`move_undoes_sidecar_when_request_rename_fails`) — never reaches the undo: the read-only folder makes the sidecar rename fail first, and `if res.is_err()` makes the assertion optional.
  route: implementer — make only the request rename fail (on macOS `chflags uchg a.toml`; clear it after), assert unconditionally: result is Err, `a.json` back at the root, `f/a.json` absent. Show the test fails with the undo block (ops.rs:627-629) removed.
- [P3] crates/storage/src/ops.rs:236-239 (`reserved_at_root`) — new names are compared with `to_lowercase()`, so on APFS `create_folder(root,"","environmentſ")` or `rename_folder(root,"x","environmentſ")` creates/renames into the `environments` slot.
  route: implementer — use full Unicode case folding for root-level names in create_folder, rename_folder and move_node (or verify what the name opens after creating it and refuse/undo).
- [NOTE] (optional) ops.rs:494 — the `new == "{new_stem}.toml"` branch is now unreachable; remove it.
- [NOTE] The acceptance line "a rename whose new sidecar name equals an existing sibling request file fails with Exists" is superseded by Amendment (ops safety — rename): `.toml` sidecars are never renamed. Recorded here; the rewritten `rename_sidecar_onto_sibling_request_file_fails` keeps the sidecar-collision Exists case.

## Review round 4 (test-only, user-approved extra round) — findings

Reviewed HEAD: 9266c84837cdfe0d77ae60f14cc3802f754c2901

- [P2] crates/storage/src/ops.rs:1372-1391 (`case_alias_sidecar_counts_as_shared`) — rename and delete steps pass with the safe-sidecar check removed: `rename_request` matches the stem prefix case-sensitively so `"A.body.json"` never triggers a rename, and `delete_node(r, "f/c.toml")` looks in `f/`, never touching the root `a.body.json`.
  route: implementer — test change only: `a.toml` file=`"a.body.json"`, `b.toml` file=`"A.body.json"`, real file `a.body.json`; run the delete on a new root-level `d.toml` file=`"a.body.json"` instead of `f/c.toml`. Show the test fails under (1) `safe_sidecar(&abs, sc).is_some()` → `!sc.to_lowercase().ends_with(".toml")` (at the rename assert) and (2) `delete_node` dropping the shared check (at the delete assert), then passes on the real code.
