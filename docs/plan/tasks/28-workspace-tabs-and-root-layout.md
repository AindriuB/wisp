# 28 — Build the Workspace and RequestTab entities and mount the root layout

**Repo:** .
**Depends on:** 21, 23
**Owns:**
- crates/app/src/main.rs
- crates/app/src/workspace.rs
- crates/app/src/tab.rs
- crates/app/src/ui/root.rs
- crates/app/src/view.rs (delete)
- crates/app/src/actions.rs (keystroke strings in `bind_keys`, its key-string helpers and their doc comment, and the binding test only; see amendment and review round 1)
- crates/app/src/settings.rs (`save` / `set_if_differs` bodies and their tests only; see amendment)

## Goal
Replace the walking-skeleton window with the v1 shell: a `Workspace` entity holding the open collection, active environment, `Engine`, settings and tabs; `RequestTab` entities with draft, saved snapshot and response; one background task draining engine events and routing them by `RequestId` through weak handles; the watcher wired to reload or flag tabs; explicit save; and the root view with sidebar | (tab bar, request pane, draggable response split). The `WISP_AUTOSEND_URL` hook moves here and keeps its exact output.

## Context
- crates/app/src/view.rs:1-226 — engine drain, body release, autosend and `WISP_TRACE`/`WISP_EVENT` lines to preserve
- crates/app/src/workspace.rs, tab.rs, ui/root.rs, actions.rs — frozen API from task 21
- crates/storage/src/collection.rs:54, 330 — `scope_for`, `open_collection`; crates/storage/src/environment.rs:62-151 — list/load env
- crates/storage/src/request.rs:91, 310 — `load_request`, `save_request`
- crates/storage/src/watcher.rs:24-80 — `Watcher::start`, `Changed` (relative paths; may be a directory; re-check disk)
- `wisp_storage::ops` (task 23) — `create_request` for saving scratch tabs
- crates/storage/src/history.rs:30-125 — `HistoryLog`, `HistoryEntry` (stores no secrets, headers or error text)
- `draft::on_disk_change` (task 24), `settings::load/save` (task 27), `theme::apply` (task 27) — call the frozen signatures; their behaviour lands in parallel
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/resizable.rs, src/tab/ — split and tab bar components
- scripts/measure/measure.sh:100-135 — how the hook is driven
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-28`.

## Acceptance
- [ ] `view.rs` is deleted; `main.rs` loads settings, applies the theme, calls `actions::bind_keys`, creates `Workspace` (opening `std::env::args().nth(1)` as a collection when given) and mounts `WorkspaceView` in `Root`. `WISP_TRACE` stages `app run start`, `before open_window`, `window opened`, `autosend submitted`, `first event received` are still printed under the hook.
- [ ] With `WISP_AUTOSEND_URL` set, the workspace opens a scratch tab with that URL, sends `GET` through `send`, and prints exactly `WISP_EVENT completed status=<n> size=<n>` or `WISP_EVENT failed kind=<Kind>` on stdout; `RENDERER=hardware scripts/measure/measure.sh` completes and reports idle RSS ≤ 120 MB, after-request RSS ≤ 128 MB and binary ≤ 20,971,520 B (figures pasted in the PR description).
- [ ] One detached task drains engine events; routing uses a `HashMap<RequestId, WeakEntity<RequestTab>>`; an event for a closed tab or superseded request releases any `Completed` body. A new response or `close_tab` releases the previous `BodyHandle` (test or `debug_assert` path citing `BodyStore::release`).
- [ ] `send` resolves via `wisp_core::resolve::resolve(&draft, &scope_for(path))`, returns `Err(ResolveError)` without submitting when it fails, and sets `Phase::Sending`; a second send on the same tab cancels the first. `cancel` submits `Command::Cancel`. `Headers`, `Progress`, `Completed`, `Failed`, `Cancelled` fill `ResponseState` and emit `TabEvent::ResponseChanged`.
- [ ] When a collection is open, each terminal event appends one `HistoryEntry` (template URL, not resolved; preview from at most the first 1 KiB of the body); no history is written for scratch tabs without a collection, so the measurement run writes nothing.
- [ ] `save` writes the draft with `save_request` (a scratch tab with a collection open is first given a file at the collection root by `wisp_storage::ops::create_request`), then `mark_saved`; without a collection it emits `WorkspaceEvent::Notice("Open a collection to save")`. Saves never happen implicitly.
- [ ] The watcher's `Changed` events are drained on the GPUI side: a path matching an open tab re-reads the file and applies `draft::on_disk_change` (`Reload` → `replace_draft` + new saved snapshot, `ShowBanner` → `DiskState::ChangedOnDisk`, `MarkDeleted` → `DeletedOnDisk`); `collection.toml`, `folder.toml`, request files and directories trigger one coalesced `reload_tree`; the active environment's files trigger an environment reload. A failing reload keeps the previous tree and sets `OpenCollection::error`.
- [ ] `WorkspaceView` renders sidebar | main with a draggable divider; main is tab bar over a draggable request/response split whose axis follows `Settings::split`; tab titles show `●` when dirty; a banner above the request pane reads "Changed on disk" with Reload / Keep mine buttons (or "Deleted on disk" with Close / Keep), bound to `ReloadFromDisk` / `KeepMine`. Every action in `actions.rs` has a handler here (palette and env switch open `PaletteView` as an overlay and close it on `DismissEvent`; `OpenCollection` uses `prompt_for_paths` for a directory; toggles call `update_settings` + `theme::apply`).
- [ ] `on_node_moved` rewrites matching tab paths (file or folder prefix) and `on_node_deleted` marks matching tabs `DeletedOnDisk`.
- [ ] Hand check (orchestrator, window frontmost), launched with a temp copy of `crates/storage/tests/fixtures/my-api` as the first argument: the layout shows sidebar, tab bar and request/response placeholders; ⌘T adds a scratch tab and ⌘W closes it; both dividers drag; `ToggleLayout` flips the response split axis; `ToggleTheme` cycles system/light/dark and writes only `theme` in `settings.toml`. Dirty, save and reload behaviour is hand-checked in task 35 once the editors exist.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- The contents of the sidebar, URL bar, request editors, response panes and palette (tasks 30–34); they stay as task 21's placeholders here.
- Pure logic in `draft.rs`, `settings.rs`, `theme.rs` (tasks 24, 27), except `settings::save` per the amendment below.
- Removing the `#[allow(dead_code)]` markers on modules this task does not own (task 35).
- A cookie jar, history UI, or reopening the last collection on launch.

### Amendment (2026-10-30, live system theme)
Decision (orchestrator): when `Settings::theme` is `System`, the app follows the OS appearance live. Task 27's `theme::apply` reads the appearance once and its signature is frozen, so the live follow lives in this task's root view (`crates/app/src/ui/root.rs`, already owned): subscribe with `Window::observe_window_appearance` (gpui-pre 0.3.10) and, on change, call `theme::apply(settings.theme, Some(window), cx)` again only while the current setting is `System`; changes are ignored while it is `Light` or `Dark`. The setting is read at callback time, so a `ToggleTheme` to or from `System` takes effect without re-subscribing. `theme.rs` is not changed.

Additional acceptance:
- [ ] `ui/root.rs` holds a `Window::observe_window_appearance` subscription whose callback calls `theme::apply(settings.theme, Some(window), cx)` only when the current setting is `ThemeSetting::System` (or the equivalent variant) and returns without applying otherwise.
- [ ] Hand check (orchestrator, window frontmost): with `theme = "system"` in `settings.toml`, switching macOS between Light and Dark in System Settings switches the window's theme without restart; with `theme = "dark"`, switching macOS appearance changes nothing.

### Amendment (2026-10-30, key strings in actions.rs)
Decision (orchestrator): `crates/app/src/actions.rs` (frozen in task 21) binds keys as `"secondary-enter"`, `"secondary-s"`, etc.; gpui-pre 0.3.10 rejects `secondary` as a modifier, so `actions::bind_keys` panics with `InvalidKeystrokeError` at launch, and this task requires `main` to call it. This task may change only the keystroke strings in `bind_keys`: `cmd-` on macOS and `ctrl-` elsewhere (e.g. via `cfg!(target_os = "macos")`). No change to the action types, their names, or anything else in `actions.rs`.

Additional acceptance:
- [ ] A unit test asserts every binding string in `bind_keys` parses (e.g. via gpui's `Keystroke::parse`) on the build platform.
- [ ] The app launches without panic: `RENDERER=hardware scripts/measure/measure.sh` reports status `measured`.

### Amendment (2026-10-30, opening another collection)
Decision (user): when `OpenCollection` replaces an open collection, all open tabs close. If any tab is dirty, a prompt (Discard / Cancel) is shown first; Cancel aborts the open and leaves the collection, tabs and drafts unchanged. In-flight requests from the old collection are cancelled, and their terminal events write no history into the new root.

Additional acceptance:
- [ ] A test (pure helper level is fine) shows that opening a new collection while all tabs are clean closes them, and that with a dirty tab the open requires confirmation (Cancel leaves tabs unchanged).
- [ ] Hand check (orchestrator): open collection A, open a request, ⌘O to collection B → no tabs from A remain.

### Amendment (2026-10-30, settings save writes only changed keys)
Decision (orchestrator): `settings::save` in `crates/app/src/settings.rs` writes a key that is absent from the file only when its value differs from `Settings::default()`; keys present in the file are updated only when they differ (as now). Changes are limited to `save` / `set_if_differs`; no signature change.

Additional acceptance:
- [ ] A test: with no `settings.toml`, saving settings equal to the defaults except `theme` writes a file containing only `theme`.
- [ ] Existing `settings` tests still pass.

## Review round 1 — findings
Before anything else, merge current `main` into the branch: the branch predates task 24 landing (`draft::on_disk_change` is a stub in its base).

- [ ] [P2] `crates/app/src/workspace.rs:436-445` (`send`) and `:585-597` (`append_history`): `send` resets only `phase` / `request_id` / `received`; `status`, `reason`, `version`, `headers`, `ttfb`, `total`, `size` and `error` keep the previous response's values, so a Failed/Cancelled send after a 200 records `status=Some(200)` and the old timings in `HistoryEntry` and shows them in `ResponseState`. Route: implementer — reset the whole response state on send. Input: send → 200, then send to an unreachable host → history entry has outcome Failed but status `Some(200)`.
- [ ] [P2] `open_collection` keeps old tabs (relative paths now point into the new root; ⌘S overwrites B's `users/get.toml` with A's request). Route: implementer, per amendment "opening another collection".
- [ ] [P2] `ToggleTheme` writes `split` / `pretty_limit` / `body_memory_limit` on first launch. Route: implementer, per amendment "settings save writes only changed keys".
- [ ] [STYLE] `crates/app/src/actions.rs:55`: `.unwrap()` in `bind_keys` with no adjacent comment proving infallibility; a misspelled `k("...")` lookup panics at launch, and `every_binding_string_parses` doesn't call `bind_keys`. Route: implementer — remove the unwrap (e.g. build bindings directly from static per-platform literals chosen with `cfg!`, which also removes the `Box::leak`) and make the test cover the strings `bind_keys` actually uses. Rule: `docs/conventions.md` Errors.
- [ ] [STYLE] `crates/app/src/actions.rs:25-28`: the doc comment for `key_strings` is attached to `leak` and still mentions "secondary". Route: implementer.
- [ ] [P3] `crates/app/src/workspace.rs:610-618` (`save`, scratch branch): the draft is renamed to "Untitled" before `create_request`, and a blank file is written before `save_request`; if `save_request` fails the draft is changed and an orphan `untitled.toml` remains. Route: implementer — rename/commit only after a successful save, and remove the created file if `save_request` fails.
- [NOTE] (optional) Autosend now resolves the URL, so `{{…}}` in `WISP_AUTOSEND_URL` fails as `InvalidUrl`; superseded/closed-tab terminal events write no history. Leave as is; note both in a doc comment.
