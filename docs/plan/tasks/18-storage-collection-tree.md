# 18 — Open a collection as an ordered tree, with folder and collection defaults

**Repo:** .
**Depends on:** 13
**Owns:**
- crates/storage/src/collection.rs

## Goal
Add `open_collection(root)`, which returns the folder tree and per-request
metadata (name and method) without loading bodies. Also add read and write of
`collection.toml` and `folder.toml`, and a helper that builds a resolve `Scope`
for a request path. That is what M2's sidebar and send path will consume.

## Context
- crates/storage/src/codec.rs, crates/storage/src/fs.rs (from 13)
- crates/core/src/request.rs — `CollectionDefaults`, `FolderDefaults`, `Environment`; crates/core/src/resolve/mod.rs — `Scope` (from 10)
- docs/plan/specs/2026-10-29-http-client-v1.md — section 2 (layout, ordering, `format = 1`)

## Shape
- `CollectionTree { root: PathBuf, defaults: CollectionDefaults, folders: HashMap<PathBuf, FolderDefaults>, nodes: Vec<Node> }`. `root` is canonical. `folders` is keyed by relative directory path and holds an entry for every folder, using `Default` when it has no `folder.toml`.
- `Node`:
  - `Folder { path, name, children: Vec<Node> }`
  - `Request(RequestMeta { path, name, method })`
  - `Invalid { path, error: String }`, used when a request file fails to parse. It does not fail the open.
- All `path`s are relative to `root`.
- `open_collection(root) -> Result<CollectionTree, StorageError>` behaves as follows:
  - A missing `collection.toml` yields `NotACollection`.
  - `format` other than `1` yields `UnsupportedFormat`.
  - It skips `environments/`, `STATE_DIR`, dot-entries, `collection.toml`, `folder.toml`, any `*.body.*` sidecar and non-`.toml` files.
- **Ordering.** `order = [...]` in `folder.toml` (or in `collection.toml` for the root) lists entry names: directory names for folders, and file stems without `.toml` for requests. Listed entries come first in list order. Unlisted entries follow, sorted alphabetically by name, case-insensitively. Listed names that do not exist are ignored.
- `load_collection_defaults` and `save_collection_defaults(root, &CollectionDefaults)` work on `collection.toml`. Saving a new collection writes `format = 1`. `load_folder_defaults` and `save_folder_defaults(dir, &FolderDefaults)` work on `folder.toml`. All of them edit in place through the codec.
- **Default auth (user decision, 2026-10-30).** `codec::write_auth(.., &Auth)` keeps its signature and always writes an `[auth]` table. The save functions skip calling it when the value equals that file's read default (`Auth::None` for `collection.toml`, `Auth::Inherit` for `folder.toml`) and the file had no `[auth]` table before, so an unchanged save never adds `[auth] type = "<default>"`. If the file already had `[auth]`, it is always written.
- `CollectionTree::scope_for<'a>(&'a self, request: &Path, env: Option<&'a Environment>) -> Scope<'a>` builds `folders` from outermost to nearest, following the request's ancestor directories.

## Acceptance
- [ ] Unit tests in `collection.rs`, using `tempfile` directories, show the following:
  - The spec's layout, with `users/` holding `order = ["create-user"]` and requests `get-user` and `create-user`, opens with `create-user` before `get-user`.
  - Opening does not read `create-user.body.json`. The test sets the sidecar's permissions to unreadable, and the open still succeeds.
  - A request file with broken TOML appears as `Node::Invalid`, and its siblings still load.
  - `format = 2` yields `UnsupportedFormat { found: 2 }`. A directory without `collection.toml` yields `NotACollection`.
  - `environments/dev.toml`, `.wisp/history.jsonl` and `folder.toml` do not appear as nodes.
  - `scope_for("a/b/req.toml")` returns folders `[a, a/b]` in that order.
  - Saving a `folder.toml` that has comments, with one header changed, alters only that header's table.
  - A `collection.toml` and a `folder.toml`, each with no `[auth]` table, loaded and saved unchanged, produce byte-identical output (no `[auth]` table is added).
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Loading full requests (17) and environments (19)
- Create, rename, move and delete of folders, and editing `order` from the UI (M2)
- Multiple open collections
