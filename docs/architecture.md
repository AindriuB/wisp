# Architecture

The shape of the system: what the pieces are, what talks to what, and where the
boundaries are. `explorer` and `architect` read this before inferring
structure from the filesystem, so keeping it honest saves every session a
search.

Detail and rationale live in the v1 design spec,
[`plan/specs/2026-10-29-http-client-v1.md`](plan/specs/2026-10-29-http-client-v1.md).
This file records only the stable shape.

## Purpose

A portable REST/HTTP client in Rust, in the spirit of Postman, Bruno and
Insomnia, for a developer working locally. Lightness is the reason the product
exists: existing clients are large and memory-heavy.

- **Portable.** One self-contained binary per platform. No installer, no
  registry, no system TLS library. It writes nothing outside the open collection
  folder and `settings.toml`.
- **Lightweight.** Low idle memory and a small binary. The numeric budgets are
  set from Milestone 0 measurements (see Budgets).
- **v1 platforms:** macOS (Apple Silicon and Intel), Windows x64, Linux x64
  (Wayland or X11; GPUI needs a Vulkan-capable driver).

## Components

A Cargo workspace of three crates under `crates/`. Port column omitted: nothing
here listens on a port.

| Component | Directory | Owns |
|---|---|---|
| `core` | `crates/core` | Request/response model, variable and path-param resolution, auth, the `Engine` (reqwest on tokio, rustls only), body store |
| `storage` | `crates/storage` | Reading and writing collections, environments and body files; file watcher |
| `app` | `crates/app` | GPUI and `gpui-component` UI; the binary |

The crate package and library names are an open decision (a crate called `core`
clashes with std `core`); see `conventions.md#naming`.

## Repository topology

Single repo, this directory. Nothing is untracked or fetched separately, so
`git clone` obtains everything. `.claude/` is untracked on purpose: it holds
worktree scripts installed from the agent-kit repo, and `agents/` and
`commands/` are symlinked into `~/.claude`.

## How they talk

Dependencies point one way: `app` depends on `core` and `storage`; `storage`
may depend on `core` for the model types; `core` depends on neither. A later
`cli/` crate will link `core` and `storage` directly.

### The Engine

`core` exposes a service-shaped `Engine`: commands in, events out.

```rust
let (engine, events) = Engine::start(EngineConfig::default());
let id = engine.submit(Command::Send(prepared));
engine.submit(Command::Cancel(id));
```

- Commands are `Send(PreparedRequest)` and `Cancel(RequestId)`. Events are
  `Started`, `Headers`, `Progress` (at most one per 100 ms), `Completed`,
  `Failed` (typed error) and `Cancelled`.
- Every command and event is plain data and `serde`-serialisable. No GPUI types
  and no locks appear in the API, so a socket transport (a daemon with clients)
  can be added later without changing them.
- The Engine owns a small tokio runtime of one or two worker threads. GPUI keeps
  its own executor; the split is hidden inside `core`, and the event channel is
  executor-agnostic.
- Response bodies are returned as a `BodyHandle` into a body store: in memory up
  to about 10 MB (configurable), spilled to a temp file above that. The UI reads
  pages and the body is freed when its tab closes.
- One `reqwest::Client` is kept per settings combination (TLS verify, proxy,
  redirect policy) for connection pooling. The cookie jar is per collection and
  in memory only.

### resolve() is pure

`resolve(&Request, &Scope) -> Result<PreparedRequest, ResolveError>` performs no
I/O. It applies variables, path params, inherited auth and headers, and
encoding. The Engine does only I/O on a `PreparedRequest`. The UI calls
`resolve` per keystroke for the URL preview and unresolved-variable marks, which
is why it must stay cheap and side-effect free.

### storage

A plain library: `open_collection(path)` returns the tree and request metadata
only (bodies load lazily), plus `load_request`, `save_request` and a watcher
emitting `Changed(path)`. Writes go through `toml_edit`.

## On-disk collection format

A collection is a folder of TOML files, similar to Bruno. Full examples are in
spec section 2.

```
my-api/
  collection.toml          format = 1, default vars, headers, auth
  environments/
    dev.toml               committed vars
    dev.secrets.toml       secret vars; *.secrets.toml is gitignored automatically
  users/
    folder.toml            optional: headers, auth, vars, order = [...]
    get-user.toml
    create-user.toml
    create-user.body.json  sidecar body
```

- **Identity is the file path.** No UUIDs in files. Order comes from
  `order = [...]` in `folder.toml`; unlisted items sort alphabetically.
- **`{{var}}`** is a variable, resolved first. **`{name}`** is a path param,
  resolved second, and only in the path portion of the URL. Syntax follows
  Spring, OpenAPI and RFC 6570: several params per segment, `{id:\d+}` regex
  (warning only), and `{*path}` catch-all. Values are percent-encoded except
  for catch-all.
- **Variable precedence:** request, then nearest folder outward to the
  outermost folder, then environment (secrets over plain), then collection
  defaults. An unresolved `{{var}}` blocks sending and is never sent literally.
  Built-in dynamic vars in v1 are `{{$uuid}}` and `{{$timestamp}}` only.
- **Inheritance:** `auth.type = "inherit"` walks up folders to the collection.
  Headers merge down the chain and the nearer one wins.
- **Query params:** `[[params]]` is the single source of truth; the URL bar
  displays and edits them.
- **Versioning:** `collection.toml` carries `format = 1`.
- **Save model:** explicit save, with a dirty marker. No autosave.

## Settings location

`settings.toml` sits beside the executable when a file named `portable` is
there, otherwise in the OS config directory. Nothing else is persisted outside
the collection folder.

## Boundaries that must not be crossed

- Only `crates/app` may depend on GPUI or `gpui-component`. `core` and
  `storage` must build and test without them.
- `core`'s command and event types stay plain serialisable data: no GPUI types,
  no locks, no handles to runtime internals.
- `resolve` performs no I/O.
- No TLS other than `rustls`; no OpenSSL or native-tls anywhere in the
  dependency tree.
- Nothing writes outside the collection folder and `settings.toml`
  (body-store temp files are the one exception and must be removed on close).
- Secrets (`*.secrets.toml` values) never appear in logs, history or committed
  files.

## Budgets

TBD. The memory budgets (idle RSS, RSS after one request) and the binary-size
budget per platform have not been set. They will be set from the Milestone 0
walking-skeleton measurements and recorded here, after which they are enforced.
Until then, no change may be justified or rejected by reference to a number
that is not in this section.

## Decisions worth knowing

All dated 2026-10-29; rationale in the spec's Decisions table.

- GPUI with `gpui-component` for the UI. Rejected: egui, Iced, Slint, ratatui,
  Tauri (system webview breaks portability and memory).
- TOML folder storage. Rejected: YAML (unmaintained `serde_yaml`, awkward
  multiline bodies) and a single SQLite file (opaque in git).
- Cargo workspace with a UI-free service-shaped core. Rejected: a single crate,
  and a daemon with clients, though the serialisable API keeps the latter open.
- `{name}` path params, not `:name`. Importers will convert later.
- No scripting in v1.
