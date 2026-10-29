# Wisp (HTTP client) v1 — design spec

Status: agreed 2026-10-29 in a shaping session. Architectural work; hand to `/plan`.

## Outcome

A portable REST/HTTP client in Rust, in the spirit of Postman, Bruno and
Insomnia, for a developer working locally. One self-contained binary per
platform; no installer.

**Success:** download one binary on macOS, Windows or Linux, open a collection
folder, pick an environment, send a request, read the response — while staying
small and light on memory. Lightness is the product's reason to exist: every
existing client is large and memory-heavy.

## Hard requirements

- **Portable.** One binary, no installer, no registry, no system TLS library.
  Writes nothing outside the collection folder and `settings.toml`.
- **Lightweight.** Low idle memory and small binary. Budgets are set from the
  walking skeleton's measurements and then enforced (see Milestone 0).
- **Platforms for v1:** macOS (Apple Silicon + Intel), Windows x64, Linux x64
  (Wayland/X11; GPUI needs a Vulkan-capable driver).

## Decisions

| Area | Decision | Rejected |
|---|---|---|
| UI | GPUI (Zed's framework) + `gpui-component` | egui (look), Iced/Slint, ratatui (different product), Tauri (system webview breaks portability, memory) |
| HTTP | `reqwest` on `tokio`, `rustls` only | OpenSSL / native-tls |
| Storage | Folder of TOML files, Bruno-like; large bodies as sidecar files | YAML (`serde_yaml` unmaintained, multiline bodies), single SQLite file (opaque in git) |
| Writes | `toml_edit`, preserving comments and formatting | rewriting files with `toml` |
| Structure | Cargo workspace, UI-free service-shaped core (Option A) | single crate (B); daemon + clients (C) — but core's API is kept serialisable so C stays possible |
| Path params | Spring / OpenAPI / RFC 6570 `{name}` | Bruno/Postman `:name` |
| Scripting | None in v1 | Rhai, QuickJS |
| Save model | Explicit save (⌘S), dirty marker | autosave (noisy git diffs) |

## Workspace layout

```
crates/
  core/     model, variable + path-param resolution, auth, Engine (reqwest + rustls)
  storage/  read/write collections, environments, body files; file watcher
  app/      GPUI + gpui-component UI — the only crate that depends on GPUI
```

`core` and `storage` never depend on GPUI. A later `cli/` crate links `core`
and `storage` directly.

## Section 2 — Data model and file format

```
my-api/                         a collection is a folder
  collection.toml               format = 1, default vars, headers, auth
  environments/
    dev.toml                    committed vars
    dev.secrets.toml            secret vars; *.secrets.toml gitignored automatically
  users/                        folders are folders
    folder.toml                 optional: headers, auth, vars, order = [...]
    get-user.toml
    create-user.toml
    create-user.body.json       sidecar body
```

Request file:

```toml
name = "Create user"
method = "POST"
url = "{{baseUrl}}/orgs/{orgId}/files/{name}.{ext}"   # query lives in [[params]]

[path]
orgId = "{{orgId}}"
name = "report"
ext = "pdf"

[[params]]
name = "notify"
value = "true"
enabled = true

[[headers]]
name = "X-Trace"
value = "{{$uuid}}"
enabled = false

[auth]
type = "inherit"                     # none | inherit | basic | bearer | apikey

[body]
type = "json"                        # none | json | text | xml | form | multipart | file
file = "create-user.body.json"       # or: content = '''...''' for small bodies
```

Environment: `[vars]` table in `environments/<name>.toml`; secrets in
`environments/<name>.secrets.toml` with the same shape.

Rules:

- **Variable precedence:** request → nearest folder → … → outermost folder →
  environment (secrets over plain) → collection defaults.
- **Unresolved `{{var}}`** is highlighted and blocks sending; never sent literally.
- **Built-in dynamic vars (v1):** `{{$uuid}}`, `{{$timestamp}}` only.
- **Inheritance:** `auth.type = "inherit"` walks up folders to the collection.
  Headers merge down the chain; nearer wins.
- **Identity = file path.** No UUIDs in files. Order via `order = [...]` in
  `folder.toml`; unlisted items sort alphabetically.
- **Query params:** `[[params]]` is the single source of truth; the URL bar
  displays and edits them live.
- **Versioning:** `collection.toml` carries `format = 1`.

### Path parameters

| Syntax | Meaning |
|---|---|
| `{id}` | path param; value percent-encoded (`/` → `%2F`) |
| `{id}-{id2}`, `{name}.{ext}` | several params in one segment — required |
| `{id:\d+}` | Spring-style regex; validation warning only |
| `{*path}` | catch-all; slashes in the value inserted verbatim |

- `{{...}}` is resolved first, then `{...}`.
- `{...}` is a path param only in the path portion of the URL.
- A path param value may contain `{{var}}`.
- Literal braces cannot appear unencoded in a URL (RFC 3986), so `{...}` never
  collides with real URL text.
- UI shows one input per detected param and keeps `[path]` in sync with the URL.
- Postman/Bruno importers (later) convert `:id` → `{id}`.

## Section 3 — `core` service API

```rust
let (engine, events) = Engine::start(EngineConfig::default());
let id = engine.submit(Command::Send(prepared));   // RequestId, returns immediately
engine.submit(Command::Cancel(id));
while let Ok(ev) = events.recv().await { /* executor-agnostic channel */ }
```

- **`resolve(&Request, &Scope) -> Result<PreparedRequest, ResolveError>`** —
  pure, no I/O: variables, path params, inherited auth/headers, encoding. The
  UI calls it per keystroke for the URL preview and unresolved-var marks.
- **`Engine`** does only I/O on a `PreparedRequest`.
- **Commands:** `Send(PreparedRequest)`, `Cancel(RequestId)`.
- **Events:** `Started{id, at}`, `Headers{id, status, version, headers, ttfb}`,
  `Progress{id, bytes}` (≤ 1 per 100 ms), `Completed{id, total, size, body: BodyHandle}`,
  `Failed{id, error}` (typed: dns, connect, tls, timeout, body-file, …),
  `Cancelled{id}`.
- All command/event types are plain data and `serde`-serialisable; no GPUI
  types, no locks in the API — so a socket transport (Option C) can be added
  without changing them.
- **Bodies:** `BodyHandle` points into a body store — in memory up to ~10 MB
  (configurable), spilled to a temp file above that. UI reads pages; freed when
  the tab closes.
- **Runtime:** the engine owns a small tokio runtime (1–2 worker threads). GPUI
  keeps its own executor; the split is hidden inside `core`.
- **Clients:** one `reqwest::Client` per settings combination (TLS verify,
  proxy, redirect policy), reused for connection pooling.
- **v1 limits:** timings are TTFB + total only (phase breakdown needs a custom
  hyper connector later). Cookie jar per collection, in memory only.
- **History:** append-only log of request metadata + truncated body preview;
  written in v1, no UI yet.

`storage` is a plain library: `open_collection(path) -> CollectionTree`
(tree + request metadata only; bodies load lazily), `load_request`,
`save_request` (via `toml_edit`), and a watcher emitting `Changed(path)`.

## Section 4 — UI

```
┌──────────────┬──────────────────────────────────────────────────────┐
│ [dev ▾]   ⌘K │ Get user ●  │ Create user │ +                        │
│──────────────│──────────────────────────────────────────────────────│
│ ▾ my-api     │ [GET ▾] {{baseUrl}}/users/{id}?include=profile [Send]│
│   ▾ users    │ Params  Path  Headers  Auth  Body  Settings          │
│     Get user │ ☑ include   profile                                  │
│     Create.. │──────────────────────────────────────────────────────│
│   ▸ orders   │ 200 OK · 142 ms · 3.4 KB                             │
│              │ Body  Headers  Cookies          [Pretty|Raw] [Save]  │
└──────────────┴──────────────────────────────────────────────────────┘
```

- Zed-like: dense, dark + light themes (follow system), keyboard-first.
- Sidebar: env switcher + collection tree; create/rename/move/delete map to files.
- Request editor sub-tabs: Params, Path, Headers, Auth, Body, Settings
  (timeout, redirects, TLS verify). Variables highlighted; hover shows resolved value.
- Response: status line; Body (Pretty/Raw), Headers, Cookies. Draggable split,
  vertical/horizontal toggle.
- Keys: ⌘↵ send, ⌘S save, ⌘K palette/quick-open, ⌘E switch env (Ctrl on Win/Linux).

State:

- `Workspace` entity: open collection, active environment, `Engine` handle, settings.
- `RequestTab` entity per tab: draft, saved snapshot (diff → dirty), response.
- One background task drains engine events, routes by `RequestId` via weak handles.
- File watcher: reload silently if tab clean; banner "changed on disk — reload / keep mine" if dirty.
- Pretty-print JSON on GPUI's background executor, only for bodies < ~5 MB;
  larger → Raw, paged from `BodyHandle`. Virtualised editor/lists.
- Portable settings: `settings.toml` beside the executable when a `portable`
  marker file is there, else the OS config dir. Nothing else persisted.

## Not in v1

Scripting; global vars; dynamic vars beyond `$uuid`/`$timestamp`; OS keychain
secrets; multiple open collections; history UI; image/HTML preview; Digest,
OAuth2, AWS SigV4 auth; GraphQL; importers (Postman, OpenAPI, curl); CLI
runner; persisted cookie jar; phase timings; code signing/notarization.

## Milestones

**M0 — Walking skeleton (first wave).**
- Cargo workspace with the three crates.
- `app`: GPUI window with method dropdown, URL text box, Send button,
  response status + raw body.
- `core`: minimal `Engine` (`Send` / `Completed` / `Failed`) over reqwest + rustls.
- CI (GitHub Actions) building release binaries for macOS (arm64 + x64),
  Windows x64, Linux x64.
- Measure and record idle RSS, RSS after one request, and binary size per
  platform. These numbers become the budgets in `docs/architecture.md`.
- Exit: decide GPUI stays (or revisit) on the evidence.

**M1 — Core and storage.** Full model, `resolve` (vars, path params incl.
multi-param segments, regex warnings, catch-all, inheritance), body store,
full event set, cancel; `storage` read/write with `toml_edit`, secrets files,
watcher. Heavily unit-tested without a window.

**M2 — UI v1.** Layout above, tabs, dirty/save, env switcher, auth editors
(none/inherit/basic/bearer/apikey), body editors (all v1 types), response
viewer with pretty/raw/headers/cookies, palette, settings.
