# 31 — Build the URL bar with variable highlighting and the Params, Path, Headers and Settings sub-tabs

**Repo:** .
**Depends on:** 21, 24, 25, 28, 29
**Owns:**
- crates/app/src/ui/url_bar.rs
- crates/app/src/ui/request/mod.rs
- crates/app/src/ui/request/params.rs
- crates/app/src/ui/request/path.rs
- crates/app/src/ui/request/headers.rs
- crates/app/src/ui/request/settings.rs

## Goal
The top of the request editor: method select, URL input, Send/Cancel, with `{{var}}` and `{name}` highlighted and the resolved value on hover; and the sub-tab strip with the Params, Path, Headers and Settings editors. URL text, `[[params]]` and `[path]` stay in sync both ways through `url_sync`, and the draft is updated on every keystroke.

## Context
- crates/app/src/ui/url_bar.rs, ui/request/** — frozen constructors from task 21 (`RequestPane` already mounts `AuthEditor` and `BodyEditor`; keep those two lines as they are)
- crates/app/src/tab.rs — `edit`, `replace_draft`, `TabEvent`, `request_tab` (task 28)
- crates/app/src/workspace.rs — `scope_for`, `send`, `cancel` (task 28)
- crates/app/src/url_sync.rs (task 24), crates/app/src/highlight.rs (task 25), crates/app/src/ui/kv.rs (task 29)
- crates/app/src/draft.rs — `dirty_sections` for per-sub-tab dirty dots (task 24)
- PLAN M0 note (06): the method dropdown has no chevron
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/input/input.rs:170-205 — `Input::token`/`on_token_hover` (atomic tokens only; see decision below)
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-31`.

## Acceptance
- [ ] The method select lists GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, shows a chevron, and writes `draft.method`.
- [ ] The URL input shows `url_sync::display_url(draft.url, draft.params)`; each edit calls `apply_url_text` and `sync_path_vars` inside one `tab.edit`, so the Params and Path tabs update live; edits in Params or Path update the URL input without moving its cursor when the input is not focused, and never echo back into a second edit (no feedback loop: one keystroke produces one `DraftEdited`).
- [ ] Highlighting: because gpui-component's `Input` supports only atomic tokens, a single-line strip directly under the URL input renders `highlight::url_spans` of the current text (resolved vars, dynamic vars, unresolved vars in the theme's danger colour, path params, query), and hovering a variable span shows `highlight::hover_text` in a tooltip. Recomputed on `DraftEdited`, `DraftReplaced` and `WorkspaceEvent::EnvironmentChanged`.
- [ ] The URL bar runs `resolve` on `DraftEdited`, `DraftReplaced` and `EnvironmentChanged` and lists any `ResolveError` problems under the strip in plain words (e.g. `{{orgId}} is not defined (URL)`); while problems exist the Send button is disabled. The button calls `Workspace::send` (⌘↵ is handled by the root view, task 28, through the same method, so a refused send there needs no extra UI); while `Phase::Sending` the button reads Cancel and calls `cancel`.
- [ ] Params tab: a `KvTable` with toggles bound to `draft.params`, annotated with `unresolved_names` per row. Path tab: one labelled input per `sync_path_vars` entry, with an empty-state line when the URL has no `{name}`; catch-all params are labelled `{*name}`. Headers tab: a `KvTable` for `draft.headers`, plus a read-only greyed list of inherited headers from the folder chain and collection (nearer wins, overridden ones struck through). Settings tab: timeout in ms (empty = default), follow-redirects and verify-TLS switches bound to `draft.settings`.
- [ ] The sub-tab strip shows a dot on each sub-tab whose `dirty_sections` flag is set and remembers the selected sub-tab in `tab.request_tab`.
- [ ] Every editor resyncs its inputs from the draft on `TabEvent::DraftReplaced` (reload from disk) and only then.
- [ ] Hand check (orchestrator, window frontmost) on the `my-api` fixture copy with `dev` active: typing `?a=1` in the URL adds a Params row; unchecking it removes `?a=1` from the URL; adding `{id}` to the path adds a Path input; an undefined `{{nope}}` is red, hover shows "not defined — blocks sending" and Send is refused; hovering a secret var shows `••••`.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- Auth and Body sub-tab contents (task 32).
- Editing request-level `vars` (no v1 UI; preserved on save).
- Inline-coloured text inside the input itself.
