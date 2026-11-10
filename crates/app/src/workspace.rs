//! The workspace entity: engine, collection, tabs and settings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui::{AppContext as _, Context, Entity, EventEmitter, SharedString, Task, WeakEntity};
use wisp_core::body::{BodyHandle, BodyStore};
use wisp_core::request::{CollectionDefaults, Environment, Request};
use wisp_core::resolve::{Location, Problem, ResolveError, Scope, resolve};
use wisp_core::{Command, Engine, EngineConfig, Event, EventReceiver, RequestId};
use wisp_storage::collection::{CollectionId, ensure_collection_id};
use wisp_storage::collection::{CollectionTree, open_collection};
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::{list_environments, load_environment};
use wisp_storage::history::{BODY_CAP, HistoryStore, NewEntry, Outcome};
use wisp_storage::request::{load_request, save_request};
use wisp_storage::watcher::Watcher;

use crate::draft::{self, ReloadAction};
use crate::settings::{self, Settings};
use crate::tab::{DiskState, Phase, RequestTab, ResponseState, TabEvent};

/// Quiet period after the first watcher event before the batch is handled.
const WATCH_BATCH: Duration = Duration::from_millis(50);

pub struct OpenCollection {
    pub tree: CollectionTree,
    pub environments: Vec<String>,
    pub active_env: Option<Environment>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    CollectionChanged,
    EnvironmentChanged,
    TabsChanged,
    ActiveTabChanged,
    Notice(SharedString),
}

/// What was sent, kept for the history entry written at the terminal event.
struct Sent {
    /// The unresolved draft; the store redacts it.
    draft: Request,
    request: Option<String>,
}

/// Where history writing stands for the open collection.
enum HistoryState {
    /// The collection id is not resolved yet; the first append resolves it.
    Unresolved,
    Ready(HistoryStore),
    Off,
}

pub struct Workspace {
    pub engine: Engine,
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub collection: Option<OpenCollection>,
    pub tabs: Vec<Entity<RequestTab>>,
    pub active: Option<usize>,
    routes: HashMap<RequestId, WeakEntity<RequestTab>>,
    sent: HashMap<RequestId, Sent>,
    data_dir: Option<DataDir>,
    history: HistoryState,
    /// True once a history Notice was shown for the open collection.
    history_notified: bool,
    autosend: Option<RequestId>,
    watcher: Option<Watcher>,
    _watch_task: Option<Task<()>>,
    empty_defaults: CollectionDefaults,
    /// True while a discard prompt is waiting for an answer.
    pub prompt_up: bool,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

/// Path of a tab after `old` was moved to `new`; `None` when it is not affected.
fn moved_path(tab: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    tab.strip_prefix(old).ok().map(|rest| {
        if rest.as_os_str().is_empty() {
            new.to_path_buf()
        } else {
            new.join(rest)
        }
    })
}

/// True when a changed path (relative to the collection root) is part of the tree.
fn affects_tree(path: &Path) -> bool {
    !path.starts_with("environments")
}

/// True when a changed path is one of the active environment's files.
fn affects_env(path: &Path, env: &str) -> bool {
    path.parent() == Some(Path::new("environments"))
        && path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f == format!("{env}.toml") || f == format!("{env}.secrets.toml"))
}

/// Replacing a collection discards every tab, so any dirty tab needs a confirmation.
fn open_needs_confirm(dirty: impl IntoIterator<Item = bool>) -> bool {
    dirty.into_iter().any(|d| d)
}

/// What closing one tab does.
#[derive(Debug, PartialEq, Eq)]
pub enum CloseStep {
    Close,
    Prompt,
    Nothing,
}

/// Decide a tab close from (tab dirty, a prompt already up).
pub fn close_step(dirty: bool, prompt_active: bool) -> CloseStep {
    match (dirty, prompt_active) {
        (false, _) => CloseStep::Close,
        (true, false) => CloseStep::Prompt,
        (true, true) => CloseStep::Nothing,
    }
}

impl Workspace {
    /// True when any tab holds unsaved changes.
    pub fn any_dirty(&self, cx: &gpui::App) -> bool {
        self.tabs.iter().any(|t| t.read(cx).is_dirty())
    }

    /// Close the tab `tab` if it is still open; the index may have moved.
    pub fn close_tab_entity(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t == tab) {
            self.close_tab(ix, cx);
        }
    }

    pub fn new(
        settings: Settings,
        settings_path: Option<PathBuf>,
        open: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (engine, events) = Engine::start(EngineConfig {
            body_memory_limit: settings.body_memory_limit,
            ..EngineConfig::default()
        });
        let mut this = Self {
            engine,
            settings,
            settings_path,
            collection: None,
            tabs: Vec::new(),
            active: None,
            routes: HashMap::new(),
            sent: HashMap::new(),
            data_dir: DataDir::resolve(),
            history: HistoryState::Unresolved,
            history_notified: false,
            autosend: None,
            watcher: None,
            _watch_task: None,
            empty_defaults: CollectionDefaults::default(),
            prompt_up: false,
        };
        this.spawn_drain(events, cx);
        if let Some(root) = open
            && let Err(e) = this.open_collection(&root, cx)
        {
            eprintln!("wisp: cannot open collection {}: {e:#}", root.display());
        }
        // The URL goes through `send`, so a resolve failure prints its problems.
        // Terminal events of superseded or closed tabs write no history.
        if let Ok(url) = std::env::var("WISP_AUTOSEND_URL") {
            let tab = this.new_scratch_tab(url, cx);
            match this.send(&tab, cx) {
                Ok(id) => {
                    this.autosend = Some(id);
                    crate::trace("autosend submitted");
                }
                Err(e) => println!(
                    "WISP_EVENT failed kind=Resolve problems={}",
                    describe_problems(&e)
                ),
            }
        }
        this
    }

    /// One background task drains the event channel; the UI thread only awaits.
    fn spawn_drain(&self, rx: EventReceiver, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut first = true;
            while let Ok(event) = rx.recv().await {
                if std::mem::take(&mut first) {
                    crate::trace("first event received");
                }
                if this
                    .update(cx, |this, cx| this.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn open_collection(&mut self, root: &Path, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let tree = open_collection(root)?;
        // The new collection opened; only now drop the old one's tabs (their
        // relative paths would otherwise point into the new root).
        // In-flight requests are cancelled by `close_tab`; their terminal events
        // find no tab and write no history.
        while !self.tabs.is_empty() {
            self.close_tab(self.tabs.len() - 1, cx);
        }
        let environments = list_environments(&tree.root).unwrap_or_default();
        let (watcher, rx) = match Watcher::start(&tree.root) {
            Ok((w, rx)) => (Some(w), Some(rx)),
            Err(e) => {
                tracing::warn!(error = %e, "watcher unavailable");
                (None, None)
            }
        };
        self.history = HistoryState::Unresolved;
        self.history_notified = false;
        self.collection = Some(OpenCollection {
            tree,
            environments,
            active_env: None,
            error: None,
        });
        self.watcher = watcher;
        self._watch_task = rx.map(|rx| {
            cx.spawn(async move |this, cx| {
                while let Ok(first) = rx.recv().await {
                    cx.background_executor().timer(WATCH_BATCH).await;
                    let mut paths = vec![first.path];
                    while let Ok(more) = rx.try_recv() {
                        paths.push(more.path);
                    }
                    if this
                        .update(cx, |this, cx| this.on_disk_changes(&paths, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        cx.emit(WorkspaceEvent::CollectionChanged);
        cx.notify();
        Ok(())
    }

    /// True when opening another collection needs the user's confirmation.
    pub fn open_needs_confirm(&self, cx: &Context<Self>) -> bool {
        open_needs_confirm(self.tabs.iter().map(|t| t.read(cx).is_dirty()))
    }

    pub fn reload_tree(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.collection.as_mut() else {
            return;
        };
        match open_collection(&c.tree.root) {
            Ok(tree) => {
                c.tree = tree;
                c.error = None;
            }
            Err(e) => c.error = Some(e.to_string()),
        }
        if let Ok(names) = list_environments(&c.tree.root) {
            c.environments = names;
        }
        cx.emit(WorkspaceEvent::CollectionChanged);
        cx.notify();
    }

    pub fn set_environment(
        &mut self,
        name: Option<&str>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let Some(c) = self.collection.as_mut() else {
            return Ok(());
        };
        c.active_env = match name {
            Some(n) => Some(load_environment(&c.tree.root, n)?),
            None => None,
        };
        cx.emit(WorkspaceEvent::EnvironmentChanged);
        cx.notify();
        Ok(())
    }

    /// Handle a coalesced batch of watcher events.
    fn on_disk_changes(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
            return;
        };
        let mut paths: Vec<&PathBuf> = paths.iter().collect();
        paths.sort();
        paths.dedup();
        for p in &paths {
            if let Some(tab) = self
                .tabs
                .iter()
                .find(|t| t.read(cx).path.as_deref() == Some(p.as_path()))
                .cloned()
            {
                self.apply_disk_state(&tab, &root, cx);
            }
        }
        if paths.iter().any(|p| affects_tree(p)) {
            self.reload_tree(cx);
        }
        let env_name = self
            .collection
            .as_ref()
            .and_then(|c| c.active_env.as_ref())
            .map(|e| e.name.clone());
        if let Some(name) = env_name
            && paths.iter().any(|p| affects_env(p, &name))
        {
            let result = self.set_environment(Some(&name), cx);
            if let Err(e) = result {
                cx.emit(WorkspaceEvent::Notice(
                    format!("Environment reload failed: {e}").into(),
                ));
            }
        } else if paths.iter().any(|p| !affects_tree(p))
            && let Some(c) = self.collection.as_mut()
            && let Ok(names) = list_environments(&root)
        {
            c.environments = names;
            cx.emit(WorkspaceEvent::EnvironmentChanged);
        }
    }

    /// Re-read a tab's file and apply the disk-change decision.
    fn apply_disk_state(&mut self, tab: &Entity<RequestTab>, root: &Path, cx: &mut Context<Self>) {
        tab.update(cx, |t, cx| {
            let Some(rel) = t.path.clone() else { return };
            let abs = root.join(&rel);
            let disk = if abs.exists() {
                Some(load_request(&abs).map_err(|e| e.to_string()))
            } else {
                None
            };
            let loaded = disk.as_ref().and_then(|d| d.as_ref().ok()).cloned();
            match draft::on_disk_change(t.is_dirty(), t.saved.as_ref(), disk) {
                ReloadAction::Reload => {
                    if let Some(req) = loaded {
                        t.saved = Some(req.clone());
                        t.replace_draft(req, cx);
                        t.set_disk_state(DiskState::InSync, cx);
                    }
                }
                ReloadAction::ShowBanner => t.set_disk_state(DiskState::ChangedOnDisk, cx),
                ReloadAction::MarkDeleted => t.set_disk_state(DiskState::DeletedOnDisk, cx),
                ReloadAction::Ignore => {}
            }
        });
    }

    /// Discard the draft and take the file as it is on disk.
    pub fn reload_from_disk(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
            return;
        };
        let Some(rel) = tab.read(cx).path.clone() else {
            return;
        };
        match load_request(&root.join(&rel)) {
            Ok(req) => tab.update(cx, |t, cx| {
                t.saved = Some(req.clone());
                t.replace_draft(req, cx);
                t.set_disk_state(DiskState::InSync, cx);
            }),
            Err(e) => cx.emit(WorkspaceEvent::Notice(format!("Reload failed: {e}").into())),
        }
    }

    /// Keep the draft and stop showing the disk-change banner.
    pub fn keep_mine(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        tab.update(cx, |t, cx| t.set_disk_state(DiskState::InSync, cx));
    }

    pub fn open_request(&mut self, rel: &Path, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| t.read(cx).path.as_deref() == Some(rel))
        {
            self.activate_tab(ix, cx);
            return;
        }
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
            return;
        };
        match load_request(&root.join(rel)) {
            Ok(req) => {
                let tab = cx.new(|_| RequestTab::from_saved(rel.to_path_buf(), req));
                self.tabs.push(tab);
                self.active = Some(self.tabs.len() - 1);
                cx.emit(WorkspaceEvent::TabsChanged);
                cx.emit(WorkspaceEvent::ActiveTabChanged);
                cx.notify();
            }
            Err(e) => cx.emit(WorkspaceEvent::Notice(
                format!("Cannot open {}: {e}", rel.display()).into(),
            )),
        }
    }

    pub fn new_scratch_tab(&mut self, url: String, cx: &mut Context<Self>) -> Entity<RequestTab> {
        let tab = cx.new(|_| RequestTab::new_scratch(url));
        self.tabs.push(tab.clone());
        self.active = Some(self.tabs.len() - 1);
        cx.emit(WorkspaceEvent::TabsChanged);
        cx.emit(WorkspaceEvent::ActiveTabChanged);
        cx.notify();
        tab
    }

    pub fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() && self.active != Some(ix) {
            self.active = Some(ix);
            cx.emit(WorkspaceEvent::ActiveTabChanged);
            cx.notify();
        }
    }

    pub fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(ix);
        let (sending, body) = tab.update(cx, |t, _| {
            (t.response.phase == Phase::Sending, t.response.body.take())
        });
        if sending && let Some(id) = tab.read(cx).response.request_id {
            self.engine.submit(Command::Cancel(id));
        }
        if let Some(h) = body {
            self.release_body(&h);
        }
        let closed = tab.downgrade();
        self.routes.retain(|_, w| *w != closed);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > ix => Some(a - 1),
            Some(a) if a < ix => Some(a),
            _ => Some(ix.min(self.tabs.len() - 1)),
        };
        cx.emit(WorkspaceEvent::TabsChanged);
        cx.emit(WorkspaceEvent::ActiveTabChanged);
        cx.notify();
    }

    pub fn active_tab(&self) -> Option<Entity<RequestTab>> {
        self.active.and_then(|ix| self.tabs.get(ix).cloned())
    }

    pub fn scope_for(&self, rel: Option<&Path>) -> Scope<'_> {
        match &self.collection {
            Some(c) => c
                .tree
                .scope_for(rel.unwrap_or_else(|| Path::new("")), c.active_env.as_ref()),
            None => Scope {
                collection: &self.empty_defaults,
                folders: Vec::new(),
                environment: None,
            },
        }
    }

    pub fn send(
        &mut self,
        tab: &Entity<RequestTab>,
        cx: &mut Context<Self>,
    ) -> Result<RequestId, ResolveError> {
        let (draft, path, previous) = {
            let t = tab.read(cx);
            (
                t.draft.clone(),
                t.path.clone(),
                (t.response.phase == Phase::Sending)
                    .then_some(t.response.request_id)
                    .flatten(),
            )
        };
        let prepared = resolve(&draft, &self.scope_for(path.as_deref()))?;
        if let Some(old) = previous {
            self.engine.submit(Command::Cancel(old));
        }
        let id = self.engine.submit(Command::Send(prepared));
        self.routes.insert(id, tab.downgrade());
        self.sent.insert(
            id,
            Sent {
                request: path.map(|p| p.to_string_lossy().into_owned()),
                draft,
            },
        );
        // Start from a clean response so nothing of the previous one leaks into
        // this one's display or history entry; the old body goes back to the store.
        let old_body = tab.update(cx, |t, cx| {
            let old = t.response.body.take();
            t.response = ResponseState {
                phase: Phase::Sending,
                request_id: Some(id),
                ..ResponseState::default()
            };
            cx.emit(TabEvent::ResponseChanged);
            cx.notify();
            old
        });
        if let Some(h) = old_body {
            self.release_body(&h);
        }
        Ok(id)
    }

    pub fn cancel(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        let t = tab.read(cx);
        if t.response.phase == Phase::Sending
            && let Some(id) = t.response.request_id
        {
            self.engine.submit(Command::Cancel(id));
        }
    }

    /// Release a body through `BodyStore::release`; the handle must then be unreadable.
    fn release_body(&self, handle: &BodyHandle) {
        let bodies = self.engine.bodies();
        bodies.release(handle);
        debug_assert!(
            bodies.read(handle, 0, 1).is_err(),
            "body still readable after BodyStore::release"
        );
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let id = match &event {
            Event::Started { id, .. }
            | Event::Headers { id, .. }
            | Event::Progress { id, .. }
            | Event::Completed { id, .. }
            | Event::Failed { id, .. }
            | Event::Cancelled { id } => *id,
        };
        let terminal = matches!(
            event,
            Event::Completed { .. } | Event::Failed { .. } | Event::Cancelled { .. }
        );
        let tab = self.routes.get(&id).and_then(WeakEntity::upgrade);
        let current = tab
            .as_ref()
            .is_some_and(|t| t.read(cx).response.request_id == Some(id));
        if terminal {
            self.routes.remove(&id);
        }
        let sent = if terminal {
            self.sent.remove(&id)
        } else {
            None
        };
        let Some(tab) = tab.filter(|_| current) else {
            // Closed tab or superseded request: free any body that arrives.
            if let Event::Completed { body, .. } = &event {
                self.release_body(body);
            }
            return;
        };
        let bodies = self.engine.bodies();
        let mut old_body = None;
        let mut stored_body: Option<Vec<u8>> = None;
        let autosend_line = tab.update(cx, |t, cx| {
            let mut line = None;
            let is_auto = self.autosend == Some(id);
            match &event {
                Event::Started { .. } => {}
                Event::Headers {
                    status,
                    reason,
                    version,
                    headers,
                    ttfb,
                    ..
                } => {
                    old_body = t.response.body.take();
                    t.response.status = Some(*status);
                    t.response.reason = reason.clone();
                    t.response.version = Some(*version);
                    t.response.headers = headers.clone();
                    t.response.ttfb = Some(*ttfb);
                    t.response.total = None;
                    t.response.size = None;
                    t.response.error = None;
                }
                Event::Progress { bytes, .. } => t.response.received = *bytes,
                Event::Completed {
                    total, size, body, ..
                } => {
                    old_body = t.response.body.take();
                    // Read now: the handle is released when the next request replaces it.
                    stored_body = bodies.read(body, 0, BODY_CAP + 1).ok();
                    t.response.phase = Phase::Done;
                    t.response.total = Some(*total);
                    t.response.size = Some(*size);
                    t.response.received = *size;
                    t.response.body = Some(*body);
                    if is_auto {
                        line = Some(format!(
                            "WISP_EVENT completed status={} size={size}",
                            t.response.status.unwrap_or(0)
                        ));
                    }
                }
                Event::Failed { error, .. } => {
                    old_body = t.response.body.take();
                    t.response.phase = Phase::Failed;
                    t.response.error = Some(error.clone());
                    if is_auto {
                        line = Some(format!("WISP_EVENT failed kind={:?}", error.kind));
                    }
                }
                Event::Cancelled { .. } => t.response.phase = Phase::Cancelled,
            }
            cx.emit(TabEvent::ResponseChanged);
            cx.notify();
            line
        });
        if let Some(h) = old_body {
            self.release_body(&h);
        }
        if let Some(line) = autosend_line {
            println!("{line}");
        }
        if terminal && let Some(sent) = sent {
            self.append_history(&tab, sent, &event, stored_body, cx);
        }
    }

    /// One entry per terminal event, only while a collection is open. The store I/O runs on
    /// the background executor.
    fn append_history(
        &mut self,
        tab: &Entity<RequestTab>,
        sent: Sent,
        event: &Event,
        body: Option<Vec<u8>>,
        cx: &mut Context<Self>,
    ) {
        let Some(c) = &self.collection else { return };
        let root = c.tree.root.clone();
        let task_root = root.clone();
        let store = match &self.history {
            HistoryState::Off => return,
            HistoryState::Ready(s) => Some(s.clone()),
            HistoryState::Unresolved => None,
        };
        let Some(data) = self.data_dir.clone() else {
            self.history_off("the data directory is unknown".into(), cx);
            return;
        };
        let r = &tab.read(cx).response;
        let outcome = match event {
            Event::Completed { .. } => Outcome::Completed,
            Event::Failed { error, .. } => Outcome::Failed { kind: error.kind },
            _ => Outcome::Cancelled,
        };
        let new = NewEntry {
            at: SystemTime::now(),
            request_path: sent.request,
            request: sent.draft,
            outcome,
            status: r.status,
            ttfb: r.ttfb,
            total: r.total,
            size: r.size,
            response_headers: r.headers.clone(),
        };
        cx.spawn(async move |this, cx| {
            let work = cx.background_executor().spawn(async move {
                let store = match store {
                    Some(s) => s,
                    None => {
                        // Serialised so concurrent first appends agree on one id.
                        static RESOLVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
                        let _g = RESOLVE.lock().unwrap_or_else(|e| e.into_inner());
                        let id: CollectionId =
                            ensure_collection_id(&root).map_err(|e| (true, e.to_string()))?;
                        HistoryStore::open(&data, &id)
                    }
                };
                store
                    .append(new, body.as_deref())
                    .map(|_| store)
                    .map_err(|e| (false, e.to_string()))
            });
            let result = work.await;
            this.update(cx, |this, cx| {
                // The collection changed while this was in flight: its result is stale.
                if this.collection.as_ref().map(|c| &c.tree.root) != Some(&task_root) {
                    return;
                }
                match result {
                    Ok(store) => {
                        if matches!(this.history, HistoryState::Unresolved) {
                            this.history = HistoryState::Ready(store);
                        }
                    }
                    Err((true, reason)) => this.history_off(reason, cx),
                    Err((false, reason)) => {
                        tracing::warn!(error = %reason, "history append failed");
                        this.history_notice(format!("History entry dropped: {reason}"), cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn history_off(&mut self, reason: String, cx: &mut Context<Self>) {
        self.history = HistoryState::Off;
        self.history_notice(format!("History is off: {reason}"), cx);
    }

    /// Show a history Notice, once per collection.
    fn history_notice(&mut self, text: String, cx: &mut Context<Self>) {
        if !std::mem::replace(&mut self.history_notified, true) {
            cx.emit(WorkspaceEvent::Notice(text.into()));
        }
    }

    pub fn save(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
            cx.emit(WorkspaceEvent::Notice("Open a collection to save".into()));
            return Ok(());
        };
        let mut draft = tab.read(cx).draft.clone();
        let (rel, created) = match tab.read(cx).path.clone() {
            Some(rel) => (rel, false),
            None => {
                if draft.name.is_empty() {
                    draft.name = "Untitled".into();
                }
                let rel = wisp_storage::ops::create_request(&root, Path::new(""), &draft.name)?;
                (rel, true)
            }
        };
        if let Err(e) = save_request(&root.join(&rel), &draft) {
            if created {
                let _ = std::fs::remove_file(root.join(&rel));
            }
            return Err(e.into());
        }
        // Only a successful save touches the tab.
        let name = draft.name;
        tab.update(cx, |t, cx| {
            if t.draft.name.is_empty() {
                t.edit(|r| r.name = name, cx);
            }
            t.mark_saved(rel, cx)
        });
        cx.notify();
        Ok(())
    }

    pub fn bodies(&self) -> BodyStore {
        self.engine.bodies()
    }

    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        f(&mut self.settings);
        if let Some(path) = &self.settings_path
            && let Err(e) = settings::save(path, &self.settings)
        {
            cx.emit(WorkspaceEvent::Notice(
                format!("Could not save settings: {e}").into(),
            ));
        }
        cx.notify();
    }

    pub fn on_node_moved(&mut self, old_rel: &Path, new_rel: &Path, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.update(cx, |t, cx| {
                if let Some(p) = t
                    .path
                    .as_deref()
                    .and_then(|p| moved_path(p, old_rel, new_rel))
                {
                    t.path = Some(p);
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    pub fn on_node_deleted(&mut self, rel: &Path, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.update(cx, |t, cx| {
                if t.path.as_deref().is_some_and(|p| p.starts_with(rel)) {
                    t.set_disk_state(DiskState::DeletedOnDisk, cx);
                }
            });
        }
    }
}

/// Names only, never values or syntax messages (conventions: logging).
fn describe_problems(e: &ResolveError) -> String {
    fn loc(l: &Location) -> String {
        match l {
            Location::Url => "Url".into(),
            Location::PathParam(n) => format!("PathParam:{}", n.escape_debug()),
            Location::Param(n) => format!("Param:{}", n.escape_debug()),
            Location::Header(n) => format!("Header:{}", n.escape_debug()),
            Location::Auth => "Auth".into(),
            Location::Body => "Body".into(),
            Location::Var(n) => format!("Var:{}", n.escape_debug()),
        }
    }
    e.problems
        .iter()
        .map(|p| match p {
            Problem::UnresolvedVar { name, .. } => {
                format!("UnresolvedVar({})", name.escape_debug())
            }
            Problem::MissingPathParam { name } => {
                format!("MissingPathParam({})", name.escape_debug())
            }
            Problem::Syntax { location, .. } => format!("Syntax({})", loc(location)),
            Problem::VarCycle { name } => format!("VarCycle({})", name.escape_debug()),
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_step_clean_closes() {
        assert_eq!(close_step(false, false), CloseStep::Close);
        assert_eq!(close_step(false, true), CloseStep::Close);
    }

    #[test]
    fn close_step_dirty_prompts() {
        assert_eq!(close_step(true, false), CloseStep::Prompt);
    }

    #[test]
    fn close_step_dirty_with_prompt_up_does_nothing() {
        assert_eq!(close_step(true, true), CloseStep::Nothing);
    }

    #[test]
    fn describe_problems_names_all_variants_without_values() {
        let e = ResolveError {
            problems: vec![
                Problem::UnresolvedVar {
                    name: "token".into(),
                    location: Location::Url,
                },
                Problem::MissingPathParam { name: "id".into() },
                Problem::Syntax {
                    location: Location::Url,
                    message: "SECRETMARK".into(),
                },
                Problem::VarCycle { name: "a".into() },
            ],
        };
        let out = describe_problems(&e);
        assert_eq!(
            out,
            "UnresolvedVar(token),MissingPathParam(id),Syntax(Url),VarCycle(a)"
        );
        assert!(!out.contains("SECRETMARK"));
    }

    #[test]
    fn describe_problems_escapes_newlines_in_names() {
        let e = ResolveError {
            problems: vec![Problem::UnresolvedVar {
                name: "a\nWISP_EVENT completed".into(),
                location: Location::Url,
            }],
        };
        let out = describe_problems(&e);
        assert!(!out.contains('\n'));
        assert_eq!(out, "UnresolvedVar(a\\nWISP_EVENT completed)");
    }

    #[test]
    fn open_confirmation_only_for_dirty_tabs() {
        assert!(!open_needs_confirm([]));
        assert!(!open_needs_confirm([false, false]));
        assert!(open_needs_confirm([false, true]));
    }

    #[test]
    fn moved_file_and_folder_prefix() {
        let (old, new) = (Path::new("a/x.toml"), Path::new("b/x.toml"));
        assert_eq!(
            moved_path(Path::new("a/x.toml"), old, new),
            Some(new.into())
        );
        assert_eq!(moved_path(Path::new("a/y.toml"), old, new), None);
        let (old, new) = (Path::new("a"), Path::new("c/a"));
        assert_eq!(
            moved_path(Path::new("a/sub/x.toml"), old, new),
            Some(PathBuf::from("c/a/sub/x.toml"))
        );
        assert_eq!(moved_path(Path::new("ab/x.toml"), old, new), None);
    }

    #[test]
    fn env_and_tree_classification() {
        assert!(affects_env(Path::new("environments/dev.toml"), "dev"));
        assert!(affects_env(
            Path::new("environments/dev.secrets.toml"),
            "dev"
        ));
        assert!(!affects_env(Path::new("environments/prod.toml"), "dev"));
        assert!(!affects_tree(Path::new("environments/dev.toml")));
        assert!(affects_tree(Path::new("folder.toml")));
        assert!(affects_tree(Path::new("users")));
    }
}
