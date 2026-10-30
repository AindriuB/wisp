# 21 — Freeze the app module layout, entity types and action names

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/Cargo.toml
- crates/app/src/main.rs (only the `mod` declarations added below; `view.rs` stays the mounted window)
- crates/app/src/actions.rs (new)
- crates/app/src/workspace.rs (new)
- crates/app/src/tab.rs (new)
- crates/app/src/draft.rs (new)
- crates/app/src/url_sync.rs (new)
- crates/app/src/highlight.rs (new)
- crates/app/src/palette.rs (new)
- crates/app/src/pretty.rs (new)
- crates/app/src/pager.rs (new)
- crates/app/src/cookies.rs (new)
- crates/app/src/settings.rs (new)
- crates/app/src/theme.rs (new)
- crates/app/src/ui/** (new)

## Goal
M2 needs about a dozen tasks writing `crates/app` at once. This task creates every M2 module as a compiling stub with its public types, fields and signatures frozen, so later tasks fill disjoint files without editing each other's. The walking-skeleton window (`view.rs`) stays mounted and unchanged, so `WISP_AUTOSEND_URL` measurement keeps working until task 28 swaps the root.

## Context
- crates/app/src/view.rs:1-226 — current engine wiring and autosend hook (left untouched here)
- crates/app/src/main.rs:1-26 — app init
- crates/core/src/request.rs:12-180 — `Request`, `KeyValue`, `Var`, `Auth`, `Body`, `RequestSettings`, `Environment`
- crates/core/src/model.rs:100-150 — `Event`, `Header`, `HttpVersion`, `EngineError`
- crates/storage/src/collection.rs:17-60 — `CollectionTree`, `Node`, `RequestMeta`, `scope_for`
- crates/storage/src/watcher.rs:17-30 — `Watcher`, `Changed`
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/command/ — palette component (`filterable(false)` + `on_query` lets the app rank items)
- docs/conventions.md#gpui, #dependencies, #errors
- Spec section 4 (`docs/plan/specs/2026-10-29-http-client-v1.md`)
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-21`.

## Acceptance
- [ ] `crates/app/Cargo.toml` adds `toml_edit.workspace = true` (already in the workspace and in the binary through `wisp-storage`; for `settings.rs`). No other dependency is added; root `Cargo.toml` and `Cargo.lock` gain no new package.
- [ ] `main.rs` declares the new modules; each stub module that is not yet reachable carries `#[allow(dead_code)]` on its `mod` line with the comment `// mounted by 28, allow removed by 35`. `view.rs` stays the window root.
- [ ] The items below exist with exactly these names, fields and signatures (stub bodies return defaults or `Ok(())`; no `todo!()`/`unimplemented!()`, no panics). Every enum below derives `Debug, Clone, PartialEq` (plus `Copy, Eq` where all fields allow). Pure modules (`draft`, `url_sync`, `highlight`, `palette`, `pretty`, `pager`, `cookies`, `settings`) import no `gpui` or `gpui_component` items.

```rust
// actions.rs
actions!(wisp, [SendRequest, CancelRequest, SaveRequest, NewRequest, CloseTab, NextTab, PrevTab,
    OpenPalette, SwitchEnvironment, OpenCollection, ReloadFromDisk, KeepMine, ToggleTheme, ToggleLayout]);
pub fn bind_keys(cx: &mut App); // secondary-enter Send, secondary-s Save, secondary-k Palette,
    // secondary-e SwitchEnvironment, secondary-t NewRequest, secondary-w CloseTab,
    // secondary-o OpenCollection, ctrl-tab NextTab, ctrl-shift-tab PrevTab, escape CancelRequest
    // ("secondary" is cmd on macOS and ctrl on Windows/Linux)

// tab.rs
pub enum RequestSubTab { Params, Path, Headers, Auth, Body, Settings }
pub enum ResponseSubTab { Body, Headers, Cookies }
pub enum BodyView { Pretty, Raw }
pub enum DiskState { InSync, ChangedOnDisk, DeletedOnDisk }
pub enum Phase { Idle, Sending, Done, Failed, Cancelled }
pub struct ResponseState {
    pub phase: Phase, pub request_id: Option<RequestId>,
    pub status: Option<u16>, pub reason: Option<String>, pub version: Option<HttpVersion>,
    pub headers: Vec<Header>, pub ttfb: Option<Duration>, pub total: Option<Duration>,
    pub size: Option<u64>, pub received: u64, pub body: Option<BodyHandle>,
    pub error: Option<EngineError>,
}                                   // derives Default (phase Idle)
pub enum TabEvent { DraftEdited, DraftReplaced, ResponseChanged, DiskStateChanged, Saved }
pub struct RequestTab {
    pub path: Option<PathBuf>,      // relative to the collection root; None = scratch
    pub saved: Option<Request>,     // snapshot as last loaded or saved
    pub draft: Request,
    pub revision: u64,              // bumped only by replace_draft
    pub disk: DiskState,
    pub response: ResponseState,
    pub request_tab: RequestSubTab, pub response_tab: ResponseSubTab, pub body_view: BodyView,
    pub load_error: Option<String>,
}
impl EventEmitter<TabEvent> for RequestTab {}
impl RequestTab {
    pub fn new_scratch(url: String) -> Self;
    pub fn from_saved(path: PathBuf, request: Request) -> Self;
    pub fn title(&self) -> SharedString;
    pub fn is_dirty(&self) -> bool;                                  // delegates to draft::is_dirty
    pub fn edit(&mut self, f: impl FnOnce(&mut Request), cx: &mut Context<Self>); // emits DraftEdited
    pub fn replace_draft(&mut self, request: Request, cx: &mut Context<Self>);   // emits DraftReplaced
    pub fn mark_saved(&mut self, path: PathBuf, cx: &mut Context<Self>);         // emits Saved
    pub fn set_disk_state(&mut self, state: DiskState, cx: &mut Context<Self>);
}

// workspace.rs
pub struct OpenCollection {
    pub tree: CollectionTree, pub environments: Vec<String>,
    pub active_env: Option<Environment>, pub error: Option<String>,
}
pub enum WorkspaceEvent { CollectionChanged, EnvironmentChanged, TabsChanged, ActiveTabChanged, Notice(SharedString) }
pub struct Workspace {
    pub engine: Engine, pub settings: Settings, pub settings_path: Option<PathBuf>,
    pub collection: Option<OpenCollection>, pub tabs: Vec<Entity<RequestTab>>,
    pub active: Option<usize>,
    // private fields (routes by RequestId, watcher, history, autosend id, empty defaults) are 28's
}
impl EventEmitter<WorkspaceEvent> for Workspace {}
impl Workspace {
    pub fn new(settings: Settings, settings_path: Option<PathBuf>, open: Option<PathBuf>, cx: &mut Context<Self>) -> Self;
    pub fn open_collection(&mut self, root: &Path, cx: &mut Context<Self>) -> anyhow::Result<()>;
    pub fn reload_tree(&mut self, cx: &mut Context<Self>);
    pub fn set_environment(&mut self, name: Option<&str>, cx: &mut Context<Self>) -> anyhow::Result<()>;
    pub fn open_request(&mut self, rel: &Path, cx: &mut Context<Self>);
    pub fn new_scratch_tab(&mut self, url: String, cx: &mut Context<Self>) -> Entity<RequestTab>;
    pub fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>);
    pub fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>);
    pub fn active_tab(&self) -> Option<Entity<RequestTab>>;
    pub fn scope_for(&self, rel: Option<&Path>) -> Scope<'_>;
    pub fn send(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) -> Result<RequestId, ResolveError>;
    pub fn cancel(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>);
    pub fn save(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) -> anyhow::Result<()>;
    pub fn bodies(&self) -> BodyStore;
    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Context<Self>);
    pub fn on_node_moved(&mut self, old_rel: &Path, new_rel: &Path, cx: &mut Context<Self>);
    pub fn on_node_deleted(&mut self, rel: &Path, cx: &mut Context<Self>);
}

// draft.rs (pure)
pub struct DirtySections { pub name: bool, pub method: bool, pub url: bool, pub params: bool,
    pub path: bool, pub headers: bool, pub auth: bool, pub body: bool, pub settings: bool }  // Default
pub enum ReloadAction { Reload, ShowBanner, MarkDeleted, Ignore }
pub fn blank_request() -> Request;
pub fn is_dirty(saved: Option<&Request>, draft: &Request) -> bool;
pub fn dirty_sections(saved: Option<&Request>, draft: &Request) -> DirtySections;
pub fn on_disk_change(dirty: bool, saved: Option<&Request>, disk: Option<Result<Request, String>>) -> ReloadAction;

// url_sync.rs (pure)
pub fn display_url(url: &str, params: &[KeyValue]) -> String;
pub fn apply_url_text(text: &str, params: &[KeyValue]) -> (String, Vec<KeyValue>);
pub fn sync_path_vars(url: &str, path: &[Var]) -> Vec<Var>;

// highlight.rs (pure)
pub enum VarState { Resolved, Dynamic, Unresolved, Error }
pub enum SpanKind { Text, Var { name: String, state: VarState }, PathParam { name: String, filled: bool }, Query }
pub struct Span { pub range: Range<usize>, pub kind: SpanKind }
pub fn url_spans(text: &str, request: &Request, scope: &Scope) -> Vec<Span>;
pub fn text_spans(text: &str, request: &Request, scope: &Scope) -> Vec<Span>;
pub fn hover_text(name: &str, request: &Request, scope: &Scope) -> String;
pub fn unresolved_names(text: &str, request: &Request, scope: &Scope) -> Vec<String>;

// palette.rs (pure)
pub enum CommandId { Send, Save, NewRequest, CloseTab, OpenCollection, SwitchEnvironment, ToggleTheme, ToggleLayout, ReloadFromDisk }
pub enum PaletteKind { Request(PathBuf), Environment(Option<String>), Command(CommandId) }
pub enum PaletteMode { All, Environments }
pub struct PaletteItem { pub label: String, pub detail: String, pub kind: PaletteKind }
pub struct Match { pub index: usize, pub score: i64, pub positions: Vec<usize> }
pub fn items(tree: Option<&CollectionTree>, environments: &[String], active_env: Option<&str>, mode: PaletteMode) -> Vec<PaletteItem>;
pub fn filter(items: &[PaletteItem], query: &str, limit: usize) -> Vec<Match>;

// pretty.rs (pure)
pub const PRETTY_LIMIT: u64 = 5 * 1024 * 1024;
pub enum ContentKind { Json, Xml, Html, Text, Binary }
pub struct PrettyError { pub offset: usize, pub message: String }
pub fn content_kind(headers: &[Header], sniff: &[u8]) -> ContentKind;
pub fn pretty_json(input: &[u8]) -> Result<String, PrettyError>;

// pager.rs (pure)
pub const PAGE_SIZE: usize = 64 * 1024;
pub const RAW_DISPLAY_CAP: u64 = 4 * 1024 * 1024;
pub struct Page { pub index: u64, pub offset: u64, pub len: usize }
pub fn page_count(body_len: u64) -> u64;
pub fn page(body_len: u64, index: u64) -> Option<Page>;
pub struct Utf8Decoder { /* carry bytes, private */ }   // Default
impl Utf8Decoder { pub fn push(&mut self, bytes: &[u8]) -> String; pub fn finish(&mut self) -> String; }

// cookies.rs (pure)
pub struct ResponseCookie { pub name: String, pub value: String, pub domain: Option<String>,
    pub path: Option<String>, pub expires: Option<String>, pub max_age: Option<i64>,
    pub secure: bool, pub http_only: bool, pub same_site: Option<String> }
pub fn parse_set_cookies(headers: &[Header]) -> Vec<ResponseCookie>;

// settings.rs (no gpui)
pub enum ThemeMode { System, Light, Dark }
pub enum SplitAxis { Vertical, Horizontal }
pub struct Settings { pub theme: ThemeMode, pub split: SplitAxis, pub pretty_limit: u64, pub body_memory_limit: u64 }
    // Default: System, Vertical, PRETTY_LIMIT, wisp_core::body::DEFAULT_MEMORY_LIMIT
pub fn location(exe_dir: &Path, portable_marker: bool, config_dir: Option<&Path>) -> Option<PathBuf>;
pub fn settings_path() -> Option<PathBuf>;
pub fn load(path: &Path) -> (Settings, Option<String>);
pub fn save(path: &Path, settings: &Settings) -> anyhow::Result<()>;

// theme.rs
pub fn apply(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App);

// ui/mod.rs: pub mod root; pub mod sidebar; pub mod kv; pub mod url_bar; pub mod request; pub mod response; pub mod palette;
// Each view below: a struct, `impl Render` drawing a labelled placeholder, and this constructor.
ui::root::WorkspaceView::new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self
ui::sidebar::Sidebar::new(workspace: Entity<Workspace>, window, cx) -> Self
ui::url_bar::UrlBar::new(workspace: Entity<Workspace>, tab: Entity<RequestTab>, window, cx) -> Self
ui::request::RequestPane::new(workspace, tab, window, cx) -> Self      // ui/request/mod.rs
ui::request::params::ParamsEditor / path::PathEditor / headers::HeadersEditor /
    settings::SettingsEditor / auth::AuthEditor / body::BodyEditor ::new(workspace, tab, window, cx) -> Self
ui::response::ResponsePane::new(workspace, tab, window, cx) -> Self    // ui/response/mod.rs
ui::response::body::BodyPane / headers::HeadersPane / cookies::CookiesPane ::new(workspace, tab, window, cx) -> Self
ui::palette::PaletteView::new(workspace, mode: PaletteMode, window, cx) -> Self  // impl EventEmitter<DismissEvent>
ui::kv::KvOptions { pub name_placeholder: SharedString, pub value_placeholder: SharedString, pub toggles: bool, pub mask_values: bool }
ui::kv::KvEvent::Changed(Vec<KeyValue>)
ui::kv::KvTable::new(rows: Vec<KeyValue>, options: KvOptions, window, cx) -> Self   // impl EventEmitter<KvEvent>
ui::kv::KvTable::set_rows(&mut self, rows: Vec<KeyValue>, window, cx); rows(&self) -> Vec<KeyValue>;
    set_annotations(&mut self, notes: Vec<Option<SharedString>>, cx)
```
- [ ] The stub `RequestPane` already constructs and renders all six sub-editors (Params, Path, Headers, Auth, Body, Settings) behind a sub-tab strip, and the stub `ResponsePane` constructs and renders `BodyPane`, `HeadersPane` and `CookiesPane`, so tasks 32 and 33 never edit a `mod.rs` they do not own.
- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
- [ ] `cargo test --workspace` passes with at least the 161 tests present at `49da271`.
- [ ] `RENDERER=hardware scripts/measure/measure.sh` still prints `WISP_EVENT completed` (the hook is untouched) and the binary stays at or below 20,971,520 B.

## Out of scope
- Any behaviour inside the stubs — every later task owns its file's behaviour.
- Mounting `WorkspaceView` or deleting `view.rs` (task 28).
- New dependencies beyond `toml_edit` for the app crate.
- Changes to `crates/core` or `crates/storage` (tasks 22, 23).
