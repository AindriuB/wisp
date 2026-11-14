//! The workspace entity: engine, collection, tabs and settings.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    AppContext as _, Context, Entity, EntityId, EventEmitter, SharedString, Task, WeakEntity,
};
use wisp_core::auth::EngineAuth;
use wisp_core::body::{BodyHandle, BodyStore};
use wisp_core::cookies::{CookieJar, CookieJars};
use wisp_core::request::{Auth, Body, CollectionDefaults, Environment, PartValue, Request};
use wisp_core::resolve::{Location, Problem, ResolveError, Scope, resolve};
use wisp_core::{
    Command, Engine, EngineConfig, EngineError, Event, EventReceiver, PreparedRequest, RequestId,
};
use wisp_storage::collection::{
    CollectionId, assign_new_collection_id, ensure_collection_id, read_collection_id,
};
use wisp_storage::collection::{CollectionTree, Node, open_collection};
use wisp_storage::cookies::{CookieFile, StoredCookie};
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::list_environments;
use wisp_storage::history::{BODY_CAP, HistoryEntry, HistoryStore, NewEntry, Outcome, StoredEntry};
use wisp_storage::request::{load_request, save_request};
use wisp_storage::watcher::Watcher;

use crate::draft::{self, ReloadAction};
use crate::settings::{self, Settings};
use crate::tab::{
    AuthorizeState, DiskState, HistoryOrigin, Phase, RequestTab, ResponseState, TabEvent,
};
use crate::ui::cookie_jar::{self as cookie_ui, Debounce};
use crate::ui::history as history_ui;
use crate::vault::{self, EnvAction, EnvState, EnvStatus, PromptSpec};

/// Quiet period after the first watcher event before the batch is handled.
const WATCH_BATCH: Duration = Duration::from_millis(50);

/// At most this many collections are open at once.
pub const MAX_OPEN_COLLECTIONS: usize = 8;

/// An ordered set of tabs and which one is showing.
#[derive(Default)]
pub struct TabGroup {
    pub tabs: Vec<Entity<RequestTab>>,
    pub active: Option<usize>,
}

/// One open collection and everything that belongs to it alone.
pub struct OpenCollection {
    pub tree: CollectionTree,
    pub environments: Vec<String>,
    pub active_env: Option<Environment>,
    /// How the active environment's secrets loaded; never holds a value.
    pub env_status: Option<EnvStatus>,
    /// Bumped by every environment load so a late result is dropped.
    env_gen: u64,
    pub error: Option<String>,
    /// The collection's tabs; a tab belongs to exactly one group.
    pub group: TabGroup,
    /// Where the collection's id stands; resolved off the UI thread.
    coll_id: IdState,
    history: HistoryState,
    /// True once a history Notice was shown for this collection.
    history_notified: bool,
    watcher: Option<Watcher>,
    _watch_task: Option<Task<()>>,
}

impl OpenCollection {
    fn new(tree: CollectionTree, environments: Vec<String>) -> Self {
        Self {
            tree,
            environments,
            active_env: None,
            env_status: None,
            env_gen: 0,
            error: None,
            group: TabGroup::default(),
            coll_id: IdState::Reading,
            history: HistoryState::Unresolved,
            history_notified: false,
            watcher: None,
            _watch_task: None,
        }
    }

    /// The folder name, or the whole path when there is none.
    pub fn name(&self) -> String {
        self.tree.root.file_name().map_or_else(
            || self.tree.root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }
}

/// What opening a path does, given the roots already open (canonical).
#[derive(Debug, PartialEq, Eq)]
enum Admit {
    /// Already open at this index: activate it, start nothing.
    Existing(usize),
    /// At the cap: refuse, change nothing.
    Full,
    Add,
}

fn admit(open: &[&Path], root: &Path) -> Admit {
    if let Some(ix) = open.iter().position(|p| *p == root) {
        Admit::Existing(ix)
    } else if open.len() >= MAX_OPEN_COLLECTIONS {
        Admit::Full
    } else {
        Admit::Add
    }
}

/// When two open collections share an id: `(refused, kept)`. The one opened later is refused.
fn id_clash(ids: &[Option<&str>], ix: usize) -> Option<(usize, usize)> {
    let id = ids.get(ix).copied().flatten()?;
    let other = ids
        .iter()
        .enumerate()
        .find(|(j, o)| *j != ix && **o == Some(id))?
        .0;
    Some((ix.max(other), ix.min(other)))
}

/// What to do when a collection's id lands and clashes with another open one.
#[derive(Debug, PartialEq, Eq)]
struct ClashPlan {
    /// The collection to close: the one opened later.
    refused: usize,
    /// The collection that stays.
    kept: usize,
    /// The landing id belongs to the older collection: record it before closing the newer one.
    adopt_landing_first: bool,
}

fn plan_clash(ids: &[Option<&str>], landing: usize) -> Option<ClashPlan> {
    let (refused, kept) = id_clash(ids, landing)?;
    Some(ClashPlan {
        refused,
        kept,
        adopt_landing_first: refused != landing,
    })
}

/// What closing collection `ix` does, in order.
#[derive(Debug, PartialEq, Eq)]
struct ClosePlan {
    /// Indices of the collection's tabs whose in-flight work is cancelled and released.
    release_tabs: Vec<usize>,
    /// The cookie id whose jar is flushed before the collection goes.
    flush_id: Option<String>,
    /// The active collection afterwards.
    active_after: Option<usize>,
}

fn plan_close(
    active: Option<usize>,
    ix: usize,
    len: usize,
    tab_count: usize,
    known_id: Option<&str>,
) -> ClosePlan {
    ClosePlan {
        release_tabs: (0..tab_count).collect(),
        flush_id: known_id.map(str::to_owned),
        active_after: active_after_close(active, ix, len.saturating_sub(1)),
    }
}

/// After a confirmation prompt: the index to act on, only when the answer was yes and the
/// collection captured before the prompt is still open.
pub fn confirmed_open(confirmed: bool, ix: Option<usize>) -> Option<usize> {
    if confirmed { ix } else { None }
}

/// The active index once collection `closed` was removed, leaving `len` collections.
fn active_after_close(active: Option<usize>, closed: usize, len: usize) -> Option<usize> {
    match active {
        _ if len == 0 => None,
        Some(a) if a > closed => Some(a - 1),
        Some(a) if a < closed => Some(a),
        Some(_) => Some(closed.min(len - 1)),
        None => None,
    }
}

/// The tab's owner: a collection (by index) or the scratch group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Scratch,
    Collection(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    CollectionChanged,
    EnvironmentChanged,
    TabsChanged,
    ActiveTabChanged,
    Notice(SharedString),
    /// The open collection's history gained or lost entries.
    HistoryChanged,
    /// The open collection's cookie persistence state changed.
    CookiesChanged,
    /// Environment secrets state changed (load status, keychain probe).
    SecretsChanged,
    /// Ask the user to confirm; the view answers through `Workspace::answer_prompt`.
    Prompt(PromptSpec),
}

/// What was sent, kept for the history entry written at the terminal event.
struct Sent {
    /// The unresolved draft; the store redacts it.
    draft: Request,
    request: Option<String>,
    /// The root of the sending tab's collection; the history entry goes there.
    root: Option<PathBuf>,
}

/// Where history writing stands for the open collection.
enum HistoryState {
    /// The collection id is not resolved yet; the first append resolves it.
    Unresolved,
    Ready(HistoryStore),
    /// Writing is off; the reason is shown in the history panel.
    Off(String),
}

/// Serialises every collection-id resolve so concurrent first uses agree on one id.
static RESOLVE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read or create the collection id under the shared lock. Blocking; never on the UI thread.
pub(crate) fn resolve_collection_id(root: &Path) -> Result<CollectionId, String> {
    let _g = RESOLVE.lock().unwrap_or_else(|e| e.into_inner());
    ensure_collection_id(root).map_err(|e| e.to_string())
}

/// Give the collection a fresh id under the shared lock. Blocking; never on the UI thread.
pub(crate) fn assign_collection_id(root: &Path) -> Result<CollectionId, String> {
    let _g = RESOLVE.lock().unwrap_or_else(|e| e.into_inner());
    assign_new_collection_id(root).map_err(|e| e.to_string())
}

/// Read the collection id without writing, under the shared lock. Blocking.
fn read_id_state(root: &Path) -> IdState {
    let _g = RESOLVE.lock().unwrap_or_else(|e| e.into_inner());
    match read_collection_id(root) {
        Ok(Some(id)) => IdState::Known(id.to_string()),
        Ok(None) => IdState::Absent,
        Err(e) => IdState::failed(root, e.to_string()),
    }
}

/// Where the open collection's id stands. Resolved off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IdState {
    /// The background read is in flight.
    Reading,
    /// The collection has no id yet; the first Send or toggle-on creates it.
    Absent,
    Known(String),
    /// The id can be neither read nor created; the partition is the canonical root.
    Failed {
        partition: String,
        reason: String,
    },
}

impl IdState {
    /// Blocking: canonicalises the root.
    fn failed(root: &Path, reason: String) -> IdState {
        IdState::Failed {
            partition: cookie_ui::cookie_jar_partition(None, root),
            reason,
        }
    }
}

/// The open collection's cookie persistence, as the manager shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persistence {
    /// No collection, no data dir or no usable id: the reason.
    Unavailable(String),
    /// The file's state is still being read.
    Pending,
    Off,
    On,
    /// On, but a load or save failed; saving is paused until the toggle is turned off and on.
    Failed(String),
}

/// What the Cookies manager needs from the workspace.
pub struct CookiePanel {
    pub jar: Option<CookieJar>,
    pub name: Option<String>,
    pub persistence: Persistence,
}

/// Per-collection (by id) persistence state in this run. Present once the file was checked.
struct CookieStatus {
    /// The file has been checked (and loaded when it exists).
    loaded: bool,
    enabled: bool,
    /// The toggle was pressed in this run after the entry was created.
    toggled: bool,
    /// A load, save or enable error; saves are skipped while set.
    error: Option<String>,
    debounce: Debounce,
}

/// True when the id-wait `token` is still the tab's current one (and ends the wait).
fn claim_id_wait<K: std::hash::Hash + Eq>(waits: &mut HashMap<K, u64>, tab: K, token: u64) -> bool {
    if waits.get(&tab) == Some(&token) {
        waits.remove(&tab);
        true
    } else {
        false
    }
}

/// The quit-time save runs only when persistence is on and the jar changed since the last save.
fn quit_save_needed(saving: bool, dirty: bool) -> bool {
    saving && dirty
}

/// A queued cookie operation, as far as the quit plan cares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QuitOp {
    Save,
    Enable,
    Disable,
}

/// What quit does for one collection: the last queued toggle first, then a final save.
#[derive(PartialEq, Eq, Debug)]
struct QuitPlan {
    toggle: Option<QuitOp>,
    save: bool,
}

/// Plan the quit work for one collection. `saving_now` is whether saving is on and healthy;
/// `dirty` is whether the jar has changes not yet saved (or a save was still queued).
fn plan_quit(queued: &[QuitOp], saving_now: bool, dirty: bool) -> QuitPlan {
    let toggle = queued
        .iter()
        .rev()
        .copied()
        .find(|op| matches!(op, QuitOp::Enable | QuitOp::Disable));
    let saving = match toggle {
        Some(QuitOp::Enable) => true,
        Some(_) => false,
        None => saving_now,
    };
    QuitPlan {
        toggle,
        // Enable writes the current snapshot even when clean, like CookieJob::Enable.
        save: toggle == Some(QuitOp::Enable) || quit_save_needed(saving, dirty),
    }
}

/// Quit work for one collection, ready to run on a worker thread.
struct QuitWork {
    id: String,
    plan: QuitPlan,
    /// Snapshot for the toggle's enable and the final save.
    cookies: Vec<StoredCookie>,
}

/// Plan the quit work for the saving collections `ids` plus any with queued jobs.
fn quit_work(
    mut ids: Vec<String>,
    queued: &[CookieJob],
    files: &mut HashMap<String, CookieStatus>,
    jars: &CookieJars,
) -> Vec<QuitWork> {
    for job in queued {
        if !ids.iter().any(|i| i == job.id()) {
            ids.push(job.id().to_owned());
        }
    }
    let mut work = Vec::new();
    for id in ids {
        let ops: Vec<QuitOp> = queued
            .iter()
            .filter(|j| j.id() == id)
            .filter_map(|j| match j {
                CookieJob::Save { .. } => Some(QuitOp::Save),
                CookieJob::Enable { .. } => Some(QuitOp::Enable),
                CookieJob::Disable { .. } => Some(QuitOp::Disable),
                CookieJob::Load { .. } => None,
            })
            .collect();
        let jar = jars.jar(Some(&id));
        let generation = jar.generation();
        let Some(st) = files.get_mut(&id) else {
            continue;
        };
        st.debounce.observe(generation, Instant::now());
        let dirty = st.debounce.dirty() || ops.contains(&QuitOp::Save);
        let plan = plan_quit(&ops, st.saving(), dirty);
        if plan.toggle.is_none() && !plan.save {
            continue;
        }
        if plan.save {
            st.debounce.saved(generation);
        }
        let cookies = cookie_ui::to_stored(jar.snapshot(SystemTime::now()));
        work.push(QuitWork { id, plan, cookies });
    }
    work
}

impl CookieStatus {
    /// Saving is on and healthy: loaded, enabled and no error since the toggle.
    fn saving(&self) -> bool {
        self.loaded && self.enabled && self.error.is_none()
    }

    /// Apply a finished job's outcome. Returns the error to tell the user, if any. Merging
    /// loaded cookies into the jar is the caller's job.
    fn apply(&mut self, done: &CookieDone) -> Option<String> {
        match done {
            CookieDone::Loaded(Ok(None)) => {
                self.apply_loaded(false);
                None
            }
            CookieDone::Loaded(Ok(Some(_))) => {
                self.apply_loaded(true);
                None
            }
            CookieDone::Loaded(Err(e)) => {
                // The file is left alone until the toggle is turned off and on.
                self.apply_loaded(true);
                self.error = Some(e.clone());
                Some(e.clone())
            }
            CookieDone::Saved(Err(e)) => {
                self.error = Some(e.clone());
                Some(e.clone())
            }
            CookieDone::Enabled(Err(e)) => {
                self.enabled = false;
                Some(e.clone())
            }
            CookieDone::Disabled(Err(e)) => {
                self.enabled = true;
                self.error = Some(e.clone());
                Some(e.clone())
            }
            CookieDone::Saved(Ok(()))
            | CookieDone::Enabled(Ok(()))
            | CookieDone::Disabled(Ok(())) => None,
        }
    }

    /// Apply a finished load; `file_on` is whether the file was found enabled. A toggle
    /// pressed since the load was queued wins, so `enabled` is left to it.
    fn apply_loaded(&mut self, file_on: bool) {
        self.loaded = true;
        if !self.toggled {
            self.enabled = file_on;
        }
    }
}

/// One cookie-file operation. Run strictly one at a time, in order.
enum CookieJob {
    Load {
        id: String,
    },
    Save {
        id: String,
        cookies: Vec<StoredCookie>,
    },
    Enable {
        id: String,
        cookies: Vec<StoredCookie>,
    },
    Disable {
        id: String,
    },
}

/// What a finished cookie job reports.
enum CookieDone {
    Loaded(Result<Option<Vec<StoredCookie>>, String>),
    Saved(Result<(), String>),
    Enabled(Result<(), String>),
    Disabled(Result<(), String>),
}

impl CookieJob {
    fn id(&self) -> &str {
        match self {
            CookieJob::Load { id }
            | CookieJob::Save { id, .. }
            | CookieJob::Enable { id, .. }
            | CookieJob::Disable { id } => id,
        }
    }

    /// Blocking file work, run on the background executor.
    fn run(self, data: &DataDir) -> CookieDone {
        let Some(cid) = CollectionId::parse(self.id()) else {
            let e = || "invalid collection id".to_string();
            return match self {
                CookieJob::Load { .. } => CookieDone::Loaded(Err(e())),
                CookieJob::Save { .. } => CookieDone::Saved(Err(e())),
                CookieJob::Enable { .. } => CookieDone::Enabled(Err(e())),
                CookieJob::Disable { .. } => CookieDone::Disabled(Err(e())),
            };
        };
        let file = CookieFile::new(data, &cid);
        let now = SystemTime::now();
        let s = |e: wisp_storage::error::StorageError| e.to_string();
        match self {
            CookieJob::Load { .. } => CookieDone::Loaded(if file.is_enabled() {
                file.load(now).map(Some).map_err(s)
            } else {
                Ok(None)
            }),
            CookieJob::Save { cookies, .. } => {
                CookieDone::Saved(file.save(&cookies, now).map_err(s))
            }
            CookieJob::Enable { cookies, .. } => CookieDone::Enabled(
                file.enable()
                    .and_then(|()| file.save(&cookies, now))
                    .map_err(s),
            ),
            CookieJob::Disable { .. } => CookieDone::Disabled(file.disable().map_err(s)),
        }
    }
}

/// Everything needed to reach the open collection's history off the UI thread.
pub struct HistoryJob {
    store: Option<HistoryStore>,
    root: PathBuf,
    data: DataDir,
}

impl HistoryJob {
    /// The store if the collection already has an id; never writes. Blocking.
    pub fn open_existing(self) -> Result<Option<HistoryStore>, String> {
        match self.store {
            Some(s) => Ok(Some(s)),
            None => Ok(read_collection_id(&self.root)
                .map_err(|e| e.to_string())?
                .map(|id| HistoryStore::open(&self.data, &id))),
        }
    }

    /// All entries, newest first; empty when the collection has no id. Blocking.
    pub fn list(self) -> Result<Vec<HistoryEntry>, String> {
        match self.open_existing()? {
            Some(s) => s.list().map_err(|e| e.to_string()),
            None => Ok(Vec::new()),
        }
    }

    /// Remove the history; a no-op when the collection has no id. Blocking.
    pub fn clear(self) -> Result<(), String> {
        match self.open_existing()? {
            Some(s) => s.clear().map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Load an entry's request and headers and register its stored body file as a
    /// read-only, file-backed body. The body bytes are not read. Blocking.
    pub fn load_entry(
        self,
        entry: &HistoryEntry,
        bodies: &BodyStore,
    ) -> Result<(StoredEntry, Option<BodyHandle>), String> {
        let store = self
            .open_existing()?
            .ok_or_else(|| "history entry not found".to_string())?;
        let stored = store
            .load_without_body(&entry.id)
            .map_err(|e| e.to_string())?;
        let handle = match store.body_path(&entry.id) {
            Some(path) if entry.body_stored => {
                let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
                (len > 0).then(|| bodies.open_file(path, len))
            }
            _ => None,
        };
        Ok((stored, handle))
    }

    /// Resolve the collection id when needed and open the store. Blocking.
    pub fn open(self) -> Result<HistoryStore, String> {
        match self.store {
            Some(s) => Ok(s),
            None => {
                let id = resolve_collection_id(&self.root)?;
                Ok(HistoryStore::open(&self.data, &id))
            }
        }
    }
}

/// A handle to `c`'s history, or the reason there is none.
fn history_job_of(c: &OpenCollection, data: Option<&DataDir>) -> Result<HistoryJob, String> {
    let store = match &c.history {
        HistoryState::Off(reason) => return Err(reason.clone()),
        HistoryState::Ready(s) => Some(s.clone()),
        HistoryState::Unresolved => None,
    };
    let Some(data) = data.cloned() else {
        return Err("the data directory is unknown".into());
    };
    Ok(HistoryJob {
        store,
        root: c.tree.root.clone(),
        data,
    })
}

/// The collection a sent tab's history entry goes to (by its root, never the active one)
/// and the job that writes it.
fn history_target(
    cols: &[OpenCollection],
    root: &Path,
    data: Option<&DataDir>,
) -> Option<(usize, Result<HistoryJob, String>)> {
    let ix = cols.iter().position(|c| c.tree.root == root)?;
    Some((ix, history_job_of(&cols[ix], data)))
}

/// Ids of the collections whose cookie file is on and healthy.
fn saving_ids_of(cols: &[OpenCollection], files: &HashMap<String, CookieStatus>) -> Vec<String> {
    cols.iter()
        .filter_map(|c| match &c.coll_id {
            IdState::Known(id) => Some(id),
            _ => None,
        })
        .filter(|id| files.get(*id).is_some_and(CookieStatus::saving))
        .cloned()
        .collect()
}

fn tree_has_request(nodes: &[Node], rel: &Path) -> bool {
    nodes.iter().any(|n| match n {
        Node::Request(m) => m.path == rel,
        Node::Folder { children, .. } => tree_has_request(children, rel),
        Node::Invalid { path, .. } => path == rel,
    })
}

/// A tab's own path wins. A from-history tab has none, so its entry's original
/// request path scopes the resolve, but only while the tab's URL is still the
/// stored one. Flags: the original path is gone; the URL was edited. Either way
/// only the collection defaults apply.
fn scope_path_for(
    tree: &CollectionTree,
    tab_path: Option<&Path>,
    origin: Option<&HistoryOrigin>,
    draft_url: &str,
) -> (Option<PathBuf>, bool, bool) {
    if let Some(p) = tab_path {
        return (Some(p.to_path_buf()), false, false);
    }
    let Some(origin) = origin else {
        return (None, false, false);
    };
    let Some(orig) = origin.request.as_deref() else {
        return (None, false, false);
    };
    if origin.url != draft_url {
        return (None, false, true);
    }
    let rel = PathBuf::from(orig);
    if tree_has_request(&tree.nodes, &rel) {
        (Some(rel), false, false)
    } else {
        (None, true, false)
    }
}

/// Make a stored (redacted) request safe to resend: nothing emptied by
/// redaction may reach the wire. Redacted request vars are removed, redacted
/// headers, params and form fields are disabled so the user can re-enable and
/// fill them, redacted auth becomes Inherit. Returns what was changed and the
/// redacted names this did not handle.
fn sanitize_redacted(req: &mut Request, redacted: &[String]) -> (Vec<String>, Vec<String>) {
    let mut adjustments = Vec::new();
    let mut left = Vec::new();
    let note = |a: &mut Vec<String>, t: String| {
        if !a.contains(&t) {
            a.push(t);
        }
    };
    for name in redacted {
        let mut handled = false;
        if name.starts_with("auth.") {
            req.auth = Auth::Inherit;
            note(&mut adjustments, "reset auth to inherit".into());
            handled = true;
        } else if let Some(n) = name.strip_prefix("vars.") {
            let before = req.vars.len();
            req.vars.retain(|v| !(v.name == n && v.value.is_empty()));
            if req.vars.len() != before {
                note(&mut adjustments, format!("removed var {n}"));
                handled = true;
            }
        } else if let Some(n) = name.strip_prefix("path.") {
            // Fail closed: drop the emptied path var so resolution reports it
            // missing and nothing is sent until the user re-enters it.
            let before = req.path.len();
            req.path.retain(|v| !(v.name == n && v.value.is_empty()));
            if req.path.len() != before {
                note(&mut adjustments, format!("removed path variable {n}"));
                handled = true;
            }
        } else if let Some(n) = name.strip_prefix("params.") {
            for p in req
                .params
                .iter_mut()
                .filter(|p| p.name == n && p.value.is_empty())
            {
                p.enabled = false;
                note(&mut adjustments, format!("disabled param {n}"));
                handled = true;
            }
        } else if let Some(n) = name.strip_prefix("form.") {
            match &mut req.body {
                Body::Form(fields) => {
                    for f in fields
                        .iter_mut()
                        .filter(|f| f.name == n && f.value.is_empty())
                    {
                        f.enabled = false;
                        note(&mut adjustments, format!("disabled form field {n}"));
                        handled = true;
                    }
                }
                Body::Multipart(parts) => {
                    for p in parts.iter_mut().filter(|p| {
                        p.name == n && matches!(&p.value, PartValue::Text(t) if t.is_empty())
                    }) {
                        p.enabled = false;
                        note(&mut adjustments, format!("disabled form field {n}"));
                        handled = true;
                    }
                }
                _ => {}
            }
        } else if let Some(n) = name.strip_prefix("query.") {
            if let Some(url) = remove_empty_query(&req.url, n) {
                req.url = url;
                note(&mut adjustments, format!("removed query {n} from the URL"));
                handled = true;
            }
        } else if name == "url.username" || name == "url.password" {
            if let Some(url) = strip_userinfo(&req.url) {
                req.url = url;
                note(&mut adjustments, "removed credentials from the URL".into());
                handled = true;
            }
        } else {
            for h in req
                .headers
                .iter_mut()
                .filter(|h| h.name == *name && h.value.is_empty())
            {
                h.enabled = false;
                note(&mut adjustments, format!("disabled header {name}"));
                handled = true;
            }
        }
        if !handled {
            left.push(name.clone());
        }
    }
    (adjustments, left)
}

/// The URL without the `name=` pair whose value was emptied, if there is one.
fn remove_empty_query(url: &str, name: &str) -> Option<String> {
    let (rest, fragment) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let (base, query) = rest.split_once('?')?;
    let target = format!("{name}=");
    let pairs: Vec<&str> = query.split('&').collect();
    if !pairs.contains(&target.as_str()) {
        return None;
    }
    let kept: Vec<&str> = pairs.into_iter().filter(|p| *p != target).collect();
    let mut out = base.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out.push_str(fragment);
    Some(out)
}

/// The URL without its `user:password@` part, if it has one.
fn strip_userinfo(url: &str) -> Option<String> {
    let start = url.find("://").map_or(0, |i| i + 3);
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    let at = url[start..end].rfind('@')?;
    Some(format!("{}{}", &url[..start], &url[start + at + 1..]))
}

/// Sanitise a stored request in place and describe where it came from.
fn sanitized_origin(entry: &HistoryEntry, request: &mut Request) -> HistoryOrigin {
    let (adjustments, redacted) = sanitize_redacted(request, &entry.redacted);
    HistoryOrigin {
        at_ms: entry.at,
        body_stored: entry.body_stored,
        body_truncated: entry.body_truncated,
        redacted,
        adjustments,
        request: entry.request.clone(),
        url: request.url.clone(),
        stored_body: request.body.clone(),
    }
}

/// Whether any of `fields` (dotted paths) names a JSON string leaf that is
/// `""`, or `None` when the text does not scan as JSON or nests past 128. Bare `{{ref}}` values are skipped, as history
/// masks them; arrays do not add to the path.
fn empty_json_leaves(text: &str, fields: &[&str]) -> Option<bool> {
    const MAX_DEPTH: usize = 128;
    struct Scan<'a> {
        b: &'a [u8],
        i: usize,
        fields: &'a [&'a str],
        found: bool,
    }
    impl Scan<'_> {
        fn ws(&mut self) {
            while self.b.get(self.i).is_some_and(u8::is_ascii_whitespace) {
                self.i += 1;
            }
        }
        fn eat(&mut self, c: u8) -> Option<()> {
            self.ws();
            (self.b.get(self.i) == Some(&c)).then(|| self.i += 1)
        }
        fn string(&mut self) -> Option<String> {
            self.eat(b'"')?;
            let start = self.i;
            while let Some(&c) = self.b.get(self.i) {
                match c {
                    b'\\' => self.i += 2,
                    b'"' => {
                        let s = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                        self.i += 1;
                        return Some(s);
                    }
                    _ => self.i += 1,
                }
            }
            None
        }
        fn value(&mut self, path: &mut String, depth: usize) -> Option<()> {
            if depth > MAX_DEPTH {
                return None;
            }
            self.ws();
            match *self.b.get(self.i)? {
                b'{' if self.b[self.i..].starts_with(b"{{") => {
                    let end = self.b[self.i..].windows(2).position(|w| w == b"}}")?;
                    self.i += end + 2;
                }
                b'{' => {
                    self.i += 1;
                    if self.eat(b'}').is_some() {
                        return Some(());
                    }
                    loop {
                        let key = self.string()?;
                        self.eat(b':')?;
                        let keep = path.len();
                        if !path.is_empty() {
                            path.push('.');
                        }
                        path.push_str(&key);
                        self.value(path, depth + 1)?;
                        path.truncate(keep);
                        if self.eat(b',').is_none() {
                            return self.eat(b'}');
                        }
                    }
                }
                b'[' => {
                    self.i += 1;
                    if self.eat(b']').is_some() {
                        return Some(());
                    }
                    loop {
                        self.value(path, depth + 1)?;
                        if self.eat(b',').is_none() {
                            return self.eat(b']');
                        }
                    }
                }
                b'"' => {
                    if self.string()?.is_empty() {
                        self.found |= self.fields.contains(&path.as_str());
                    }
                }
                _ => {
                    let start = self.i;
                    while self
                        .b
                        .get(self.i)
                        .is_some_and(|c| !c.is_ascii_whitespace() && !b",]}".contains(c))
                    {
                        self.i += 1;
                    }
                    if self.i == start {
                        return None;
                    }
                }
            }
            Some(())
        }
    }
    let mut s = Scan {
        b: text.as_bytes(),
        i: 0,
        fields,
        found: false,
    };
    s.value(&mut String::new(), 0)?;
    s.ws();
    (s.i == s.b.len()).then_some(s.found)
}

/// Why a from-history tab may not be sent: a redacted body value has not been
/// filled in since. A redacted JSON leaf is unedited while it is still `""`
/// (a removed or non-empty leaf counts as edited); a dropped body is unedited
/// while it is empty. A body that does not scan is refused. `None` when no
/// body was redacted.
fn body_redaction_refusal(draft: &Request, origin: &HistoryOrigin) -> Option<String> {
    let mut whole = false;
    let mut fields: Vec<&str> = Vec::new();
    for n in &origin.redacted {
        if n == "body" {
            whole = true;
        } else if let Some(f) = n.strip_prefix("body.") {
            fields.push(f);
        }
    }
    if !whole && fields.is_empty() {
        return None;
    }
    let text = match &draft.body {
        Body::Json(t) | Body::Text(t) | Body::Xml(t) => Some(t.content.as_str()),
        _ => None,
    };
    let unedited = if whole {
        text.is_none_or(|t| t.trim().is_empty())
            && matches!(
                &draft.body,
                Body::None | Body::Json(_) | Body::Text(_) | Body::Xml(_)
            )
    } else {
        match text.map(|t| empty_json_leaves(t, &fields)) {
            Some(Some(found)) => found,
            Some(None) => true,
            None => false,
        }
    };
    if !unedited {
        return None;
    }
    let what = if whole {
        "the body was not stored".to_string()
    } else {
        format!("{} was not stored", fields.join(", "))
    };
    Some(format!("Edit the request body before resending: {what}"))
}

/// A request ready to send, the scope path it resolved with, and the Resend
/// notice of a from-history tab.
struct SendPlan {
    prepared: PreparedRequest,
    scope_path: Option<PathBuf>,
    notice: Option<String>,
}

/// The `AuthorizeStart` for a resolved request, or `None` when its auth is not authorization
/// code. `client` is what Send would use, with `collection` set to the same partition; the
/// environment partitions the cached token.
fn authorize_command(prepared: PreparedRequest, collection: Option<String>) -> Option<Command> {
    let auth = prepared.auth?;
    if !matches!(auth, EngineAuth::OAuth2AuthCode { .. }) {
        return None;
    }
    let mut client = prepared.client;
    client.collection = collection;
    Some(Command::AuthorizeStart {
        auth,
        client,
        environment: prepared.environment,
        timeout: prepared.timeout,
    })
}

/// True when the app may hand `url` to the system browser: `https`, or `http` to a loopback host.
pub(crate) fn may_open_in_browser(url: &str) -> bool {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((h, tail)) if tail.is_empty() || tail.starts_with(':') => h,
            _ => return false,
        },
        None => authority.split(':').next().unwrap_or(""),
    };
    if scheme.eq_ignore_ascii_case("https") {
        !host.is_empty()
    } else if scheme.eq_ignore_ascii_case("http") {
        host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
    } else {
        false
    }
}

/// What to do with an auth event once its route and tab are looked up.
#[derive(Debug, PartialEq, Eq)]
struct AuthRouting {
    /// Apply the event to the routed tab.
    apply: bool,
    /// Forget the route for the event's id.
    remove_route: bool,
}

/// Route an auth event: apply it only to a live tab still waiting on this very id; drop
/// events of a closed tab or of a superseded authorization; terminal events end the route.
fn route_auth_event(
    has_route: bool,
    tab_alive: bool,
    tab_waiting: Option<RequestId>,
    id: RequestId,
    terminal: bool,
) -> AuthRouting {
    AuthRouting {
        apply: has_route && tab_alive && tab_waiting == Some(id),
        remove_route: has_route && (terminal || !tab_alive),
    }
}

/// Closing a tab: drop its auth routes and return the cancel for a flow it is waiting on.
fn close_auth<T: PartialEq>(
    routes: &mut HashMap<RequestId, T>,
    closed: &T,
    waiting: Option<RequestId>,
) -> Option<Command> {
    routes.retain(|_, w| w != closed);
    waiting.map(Command::AuthorizeCancel)
}

/// The tab state an `AuthFailed` error shows.
fn failure_state(error: &EngineError) -> AuthorizeState {
    if error.message == "cancelled" {
        AuthorizeState::Cancelled
    } else {
        AuthorizeState::Failed(error.message.clone())
    }
}

/// Resolve a draft for an authorize command, through the same path as Send. `Ok(None)` when the
/// resolved auth is not authorization code. Pure: everything it reads is passed in.
fn prepare_authorize(
    draft: &Request,
    tab_path: Option<&Path>,
    origin: Option<&HistoryOrigin>,
    tree: Option<&CollectionTree>,
    empty_defaults: &CollectionDefaults,
    env: Option<&Environment>,
    collection: Option<String>,
) -> Result<Option<Command>, ResolveError> {
    let plan = prepare_send(draft, tab_path, origin, tree, empty_defaults, env)?;
    Ok(authorize_command(plan.prepared, collection))
}

/// Resolve a draft for sending. Pure: everything it reads is passed in.
fn prepare_send(
    draft: &Request,
    tab_path: Option<&Path>,
    origin: Option<&HistoryOrigin>,
    tree: Option<&CollectionTree>,
    empty_defaults: &CollectionDefaults,
    env: Option<&Environment>,
) -> Result<SendPlan, ResolveError> {
    let (scope_path, original_missing, url_edited) = match tree {
        Some(t) => scope_path_for(t, tab_path, origin, &draft.url),
        None => (tab_path.map(Path::to_path_buf), false, false),
    };
    let scope = match tree {
        Some(t) => t.scope_for(scope_path.as_deref().unwrap_or(Path::new("")), env),
        None => Scope {
            collection: empty_defaults,
            folders: Vec::new(),
            environment: None,
        },
    };
    if let Some(message) = origin.and_then(|o| body_redaction_refusal(draft, o)) {
        return Err(ResolveError {
            problems: vec![Problem::Syntax {
                location: Location::Body,
                message,
            }],
        });
    }
    let prepared = resolve(draft, &scope)?;
    let notice = origin.map(|o| {
        history_ui::resend_notice(
            env.map(|e| e.name.as_str()),
            &o.adjustments,
            &o.redacted,
            original_missing,
            url_edited,
        )
    });
    Ok(SendPlan {
        prepared,
        scope_path,
        notice,
    })
}

/// The request path a history entry records: the scope path used to resolve,
/// so resending a from-history entry keeps its folder defaults.
fn recorded_request(scope_path: Option<&Path>) -> Option<String> {
    scope_path.map(|p| p.to_string_lossy().into_owned())
}

pub struct Workspace {
    pub engine: Engine,
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    /// The open collections, in order.
    pub collections: Vec<OpenCollection>,
    /// Index of the active collection; `None` shows the scratch group.
    active_collection: Option<usize>,
    /// Tabs created with no collection open.
    scratch: TabGroup,
    routes: HashMap<RequestId, WeakEntity<RequestTab>>,
    /// Interactive authorizations in flight, by engine id.
    auth_routes: HashMap<RequestId, WeakEntity<RequestTab>>,
    sent: HashMap<RequestId, Sent>,
    data_dir: Option<DataDir>,
    jars: CookieJars,
    /// Persistence state per collection id, once its file was checked in this run.
    cookie_files: HashMap<String, CookieStatus>,
    cookie_queue: VecDeque<CookieJob>,
    /// True while a cookie-file job runs: at most one at a time.
    cookie_busy: bool,
    /// Sends waiting for a collection id to be created, by tab, with their token.
    id_waits: HashMap<EntityId, u64>,
    next_wait: u64,
    cookie_ticker: Option<Task<()>>,
    _quit_sub: Option<gpui::Subscription>,
    autosend: Option<RequestId>,
    empty_defaults: CollectionDefaults,
    /// True while a discard prompt is waiting for an answer.
    pub prompt_up: bool,
    stores: vault::SharedStores,
    /// Workspace-wide, never reused: an environment load result carries the value it was given,
    /// so a late result for a closed collection cannot land on a reopened one.
    env_gen_counter: u64,
    refresh_store: Arc<vault::KeychainRefreshStore>,
    /// Banners for refresh tokens kept for this session only.
    pub token_banners: Vec<String>,
    /// The keychain probe: `None` until run, then `Ok(())` or the reason.
    pub keychain_probe: Option<Result<(), String>>,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

/// True when the collection's active environment loaded from the keychain: the only case in which
/// refresh tokens are persisted.
fn persist_refresh_for(c: &OpenCollection) -> bool {
    match (&c.active_env, &c.env_status) {
        (Some(env), Some(status)) => status.persist_refresh(&env.name),
        _ => false,
    }
}

/// Tell the refresh-token adapter whether the collection's active environment is file-mode.
fn sync_file_mode(store: &vault::KeychainRefreshStore, c: &OpenCollection) {
    if let (IdState::Known(id), Some(env)) = (&c.coll_id, &c.active_env) {
        let file = c
            .env_status
            .as_ref()
            .is_some_and(|s| s.is_file_mode(&env.name));
        store.set_file_mode(id, &env.name, file);
    }
}

/// Set an outgoing auth's refresh-token persistence from the collection's environment status,
/// and tell the adapter whether that environment is file-mode. Send and Authorize both call it.
fn mark_persist(
    store: &vault::KeychainRefreshStore,
    c: Option<&OpenCollection>,
    auth: &mut EngineAuth,
) {
    if let Some(c) = c {
        sync_file_mode(store, c);
    }
    auth.set_persist_refresh(c.is_some_and(persist_refresh_for));
}

/// After "Give this copy its own id": replace the cached id with the one now on disk, so
/// cookies, history and refresh tokens use it. The new id when it changed.
fn refresh_known_id(c: &mut OpenCollection) -> Option<String> {
    let IdState::Known(old) = &c.coll_id else {
        return None;
    };
    match read_id_state(&c.tree.root) {
        IdState::Known(new) if &new != old => {
            c.coll_id = IdState::Known(new.clone());
            Some(new)
        }
        _ => None,
    }
}

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
    /// The active collection, if one is.
    pub fn active_collection(&self) -> Option<&OpenCollection> {
        self.active_collection.and_then(|i| self.collections.get(i))
    }

    /// Index of the active collection.
    pub fn active_collection_ix(&self) -> Option<usize> {
        self.active_collection
    }

    /// The root of open collection `ix`.
    pub fn collection_root(&self, ix: usize) -> Option<PathBuf> {
        self.collections.get(ix).map(|c| c.tree.root.clone())
    }

    /// The tabs showing now: the active collection's, else the scratch group's.
    pub fn group(&self) -> &TabGroup {
        self.active_collection().map_or(&self.scratch, |c| &c.group)
    }

    fn group_mut(&mut self) -> &mut TabGroup {
        match self.active_collection {
            Some(i) if i < self.collections.len() => &mut self.collections[i].group,
            _ => &mut self.scratch,
        }
    }

    fn group_of_mut(&mut self, owner: Owner) -> &mut TabGroup {
        match owner {
            Owner::Collection(i) => &mut self.collections[i].group,
            Owner::Scratch => &mut self.scratch,
        }
    }

    /// The visible tabs.
    pub fn tabs(&self) -> &[Entity<RequestTab>] {
        &self.group().tabs
    }

    /// Index of the showing tab among `tabs()`.
    pub fn active_tab_ix(&self) -> Option<usize> {
        self.group().active
    }

    pub fn all_tabs(&self) -> impl Iterator<Item = &Entity<RequestTab>> {
        self.collections
            .iter()
            .flat_map(|c| c.group.tabs.iter())
            .chain(self.scratch.tabs.iter())
    }

    fn owner_of(&self, tab: &Entity<RequestTab>) -> Option<Owner> {
        if let Some(i) = self
            .collections
            .iter()
            .position(|c| c.group.tabs.contains(tab))
        {
            Some(Owner::Collection(i))
        } else {
            self.scratch.tabs.contains(tab).then_some(Owner::Scratch)
        }
    }

    fn root_of(&self, owner: Owner) -> Option<PathBuf> {
        match owner {
            Owner::Collection(i) => self.collections.get(i).map(|c| c.tree.root.clone()),
            Owner::Scratch => None,
        }
    }

    pub fn coll_ix(&self, root: &Path) -> Option<usize> {
        self.collections.iter().position(|c| c.tree.root == root)
    }

    /// True when any tab, in any collection, holds unsaved changes.
    pub fn any_dirty(&self, cx: &gpui::App) -> bool {
        self.all_tabs().any(|t| t.read(cx).is_dirty())
    }

    /// True when closing collection `ix` would discard unsaved changes.
    pub fn collection_dirty(&self, ix: usize, cx: &gpui::App) -> bool {
        self.collections
            .get(ix)
            .is_some_and(|c| c.group.tabs.iter().any(|t| t.read(cx).is_dirty()))
    }

    /// Close the tab `tab` if it is still open; the index may have moved.
    pub fn close_tab_entity(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        if let Some(owner) = self.owner_of(tab)
            && let Some(ix) = self.group_of_mut(owner).tabs.iter().position(|t| t == tab)
        {
            self.close_tab_in(owner, ix, cx);
        }
    }

    pub fn new(
        settings: Settings,
        settings_path: Option<PathBuf>,
        open: Option<PathBuf>,
        stores: vault::SharedStores,
        cx: &mut Context<Self>,
    ) -> Self {
        let jars = CookieJars::default();
        let refresh_store = Arc::new(vault::KeychainRefreshStore::new(stores.clone()));
        let (engine, events) = Engine::start(EngineConfig {
            body_memory_limit: settings.body_memory_limit,
            cookies: jars.clone(),
            refresh_tokens: refresh_store.clone(),
            ..EngineConfig::default()
        });
        let mut this = Self {
            engine,
            settings,
            settings_path,
            collections: Vec::new(),
            active_collection: None,
            scratch: TabGroup::default(),
            routes: HashMap::new(),
            auth_routes: HashMap::new(),
            sent: HashMap::new(),
            data_dir: DataDir::resolve(),
            jars,
            cookie_files: HashMap::new(),
            cookie_queue: VecDeque::new(),
            cookie_busy: false,
            id_waits: HashMap::new(),
            next_wait: 0,
            cookie_ticker: None,
            _quit_sub: None,
            autosend: None,
            empty_defaults: CollectionDefaults::default(),
            prompt_up: false,
            stores,
            env_gen_counter: 0,
            refresh_store,
            token_banners: Vec::new(),
            keychain_probe: None,
        };
        this.spawn_drain(events, cx);
        // Flush on quit. gpui's shutdown blocks the main thread, so nothing spawned on the
        // foreground runs: take the snapshot here and save it on a worker thread.
        this._quit_sub = Some(cx.on_app_quit(|this, cx| {
            let Some((work, data)) = this.take_quit_work() else {
                return Task::ready(());
            };
            cx.background_executor().spawn(async move {
                let now = SystemTime::now();
                for w in work {
                    let Some(cid) = CollectionId::parse(&w.id) else {
                        continue;
                    };
                    let file = CookieFile::new(&data, &cid);
                    match w.plan.toggle {
                        Some(QuitOp::Enable) => {
                            let _ = file.enable();
                        }
                        Some(QuitOp::Disable) => {
                            let _ = file.disable();
                        }
                        _ => {}
                    }
                    if w.plan.save {
                        // `save` re-checks the enabled flag under the file lock.
                        let _ = file.save(&w.cookies, now);
                    }
                }
            })
        }));
        this.restore_collections(open, cx);
        // The URL goes through `send`, so a resolve failure prints its problems.
        // Terminal events of superseded or closed tabs write no history.
        if let Ok(url) = std::env::var("WISP_AUTOSEND_URL") {
            let tab = this.new_scratch_tab(url, cx);
            match this.send(&tab, cx) {
                Ok(Some(id)) => {
                    this.autosend = Some(id);
                    crate::trace("autosend submitted");
                }
                Ok(None) => {}
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

    /// Open the remembered collections and the argv one at startup. Refusals and failures are
    /// shown as one notice once the window exists; missing paths are dropped from the list.
    fn restore_collections(&mut self, argv: Option<PathBuf>, cx: &mut Context<Self>) {
        let argv = argv.map(|p| p.canonicalize().unwrap_or(p));
        let plan = settings::plan_restore(
            &self.settings.open_collections,
            self.settings.active_collection.as_deref(),
            argv,
            Path::exists,
        );
        let mut notes: Vec<String> = Vec::new();
        if !plan.dropped.is_empty() {
            let names: Vec<String> = plan
                .dropped
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            notes.push(format!(
                "Collection not found, removed from the open list: {}",
                names.join(", ")
            ));
        }
        let mut active = None;
        for path in &plan.open {
            match self.add_collection(path, cx) {
                Ok(ix) => {
                    if plan.active.as_deref() == Some(path.as_path()) {
                        active = Some(ix);
                    }
                }
                Err(e) => {
                    eprintln!("wisp: cannot open collection {}: {e:#}", path.display());
                    notes.push(format!("Cannot open {}: {e:#}", path.display()));
                }
            }
        }
        // An argv path that was refused leaves `active` unset: keep the remembered one.
        let remembered = self
            .settings
            .active_collection
            .as_deref()
            .and_then(|r| self.coll_ix(r));
        if let Some(ix) = active
            .or(remembered)
            .or((!self.collections.is_empty()).then_some(0))
        {
            self.active_collection = Some(ix);
        }
        let roots = self.open_roots();
        let active_root = self.active_collection().map(|c| c.tree.root.clone());
        if roots != self.settings.open_collections || active_root != self.settings.active_collection
        {
            self.persist_collections(cx);
        }
        if !notes.is_empty() {
            let text = notes.join("; ");
            // The window subscribes after construction; emit once it has.
            cx.spawn(async move |this, cx| {
                this.update(cx, |_, cx| cx.emit(WorkspaceEvent::Notice(text.into())))
                    .ok();
            })
            .detach();
        }
    }

    fn open_roots(&self) -> Vec<PathBuf> {
        self.collections
            .iter()
            .map(|c| c.tree.root.clone())
            .collect()
    }

    /// Remember the open list and the active collection in settings.toml.
    fn persist_collections(&mut self, cx: &mut Context<Self>) {
        let roots = self.open_roots();
        let active = self.active_collection().map(|c| c.tree.root.clone());
        self.update_settings(
            |s| {
                s.open_collections = roots;
                s.active_collection = active;
            },
            cx,
        );
    }

    /// Add a collection without activating it. An open path (after canonicalising) returns
    /// the existing index and starts nothing.
    fn add_collection(&mut self, root: &Path, cx: &mut Context<Self>) -> anyhow::Result<usize> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let open: Vec<&Path> = self
            .collections
            .iter()
            .map(|c| c.tree.root.as_path())
            .collect();
        match admit(&open, &root) {
            Admit::Existing(ix) => return Ok(ix),
            Admit::Full => anyhow::bail!(
                "At most {MAX_OPEN_COLLECTIONS} collections can be open; close one first"
            ),
            Admit::Add => {}
        }
        let tree = open_collection(&root)?;
        let environments = list_environments(&tree.root).unwrap_or_default();
        let mut c = OpenCollection::new(tree, environments);
        // gpui tests run a deterministic scheduler; a real watcher thread would schedule
        // foreground work from outside it and trip its determinism check.
        let started = if cfg!(test) {
            None
        } else {
            Some(Watcher::start(&c.tree.root))
        };
        let rx = match started {
            None => None,
            Some(Ok((w, rx))) => {
                c.watcher = Some(w);
                Some(rx)
            }
            Some(Err(e)) => {
                tracing::warn!(error = %e, "watcher unavailable");
                None
            }
        };
        let tree_root = c.tree.root.clone();
        c._watch_task = rx.map(|rx| {
            let root = tree_root.clone();
            cx.spawn(async move |this, cx| {
                while let Ok(first) = rx.recv().await {
                    cx.background_executor().timer(WATCH_BATCH).await;
                    let mut paths = vec![first.path];
                    while let Ok(more) = rx.try_recv() {
                        paths.push(more.path);
                    }
                    if this
                        .update(cx, |this, cx| this.on_disk_changes(&root, &paths, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        self.collections.push(c);
        self.start_id_read(tree_root, cx);
        Ok(self.collections.len() - 1)
    }

    /// Open a collection and make it active; an open one is just activated.
    pub fn open_collection(&mut self, root: &Path, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let ix = self.add_collection(root, cx)?;
        self.activate_collection(ix, cx);
        Ok(())
    }

    /// Make collection `ix` the active one and remember it.
    pub fn activate_collection(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.collections.len() {
            return;
        }
        self.active_collection = Some(ix);
        self.persist_collections(cx);
        self.emit_view_changed(cx);
    }

    /// The tree, environment picker, tabs, history and cookies all follow the active collection.
    fn emit_view_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(WorkspaceEvent::CollectionChanged);
        cx.emit(WorkspaceEvent::EnvironmentChanged);
        cx.emit(WorkspaceEvent::TabsChanged);
        cx.emit(WorkspaceEvent::ActiveTabChanged);
        cx.emit(WorkspaceEvent::HistoryChanged);
        cx.emit(WorkspaceEvent::CookiesChanged);
        cx.notify();
    }

    /// Close collection `ix`: its tabs are cancelled and dropped, its watcher stops and its
    /// jar is saved when persistence is on and the jar is dirty.
    pub fn close_collection(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.collections.len() {
            return;
        }
        let known = match &self.collections[ix].coll_id {
            IdState::Known(id) => Some(id.as_str()),
            _ => None,
        };
        let plan = plan_close(
            self.active_collection,
            ix,
            self.collections.len(),
            self.collections[ix].group.tabs.len(),
            known,
        );
        let tabs = std::mem::take(&mut self.collections[ix].group.tabs);
        for &t in &plan.release_tabs {
            self.release_tab(&tabs[t], cx);
        }
        if plan.flush_id.is_some() {
            self.flush_cookies_for(ix, cx);
        }
        self.collections.remove(ix);
        self.active_collection = plan.active_after;
        self.persist_collections(cx);
        self.emit_view_changed(cx);
    }

    /// Re-read the tree of the collection at `root`, if it is still open.
    pub fn reload_tree_of(&mut self, root: &Path, cx: &mut Context<Self>) {
        if let Some(ix) = self.coll_ix(root) {
            self.reload_tree_at(ix, cx);
        }
    }

    pub fn reload_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.active_collection {
            self.reload_tree_at(ix, cx);
        }
    }

    fn reload_tree_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(c) = self.collections.get_mut(ix) else {
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
        if self.active_collection == Some(ix) {
            cx.emit(WorkspaceEvent::CollectionChanged);
        }
        cx.notify();
    }

    pub fn set_environment(
        &mut self,
        name: Option<&str>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        match self.active_collection {
            Some(ix) => self.set_environment_at(ix, name, cx),
            None => Ok(()),
        }
    }

    fn set_environment_at(
        &mut self,
        ix: usize,
        name: Option<&str>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.env_gen_counter += 1;
        let generation = self.env_gen_counter;
        let Some(c) = self.collections.get_mut(ix) else {
            return Ok(());
        };
        match name {
            Some(n) => self.load_env_at(ix, n.to_string(), true, cx),
            None => {
                c.env_gen = generation;
                c.active_env = None;
                c.env_status = None;
                if self.active_collection == Some(ix) {
                    cx.emit(WorkspaceEvent::EnvironmentChanged);
                    cx.emit(WorkspaceEvent::SecretsChanged);
                }
                cx.notify();
            }
        }
        Ok(())
    }

    /// Load environment `name` of collection `ix` through the fail-closed loader on a background
    /// task. `switching` shows an empty placeholder meanwhile, so no send uses the previous
    /// environment; a reload keeps the old one until the result lands.
    fn load_env_at(&mut self, ix: usize, name: String, switching: bool, cx: &mut Context<Self>) {
        self.env_gen_counter += 1;
        let generation = self.env_gen_counter;
        let Some(c) = self.collections.get_mut(ix) else {
            return;
        };
        c.env_gen = generation;
        let previous = (c.active_env.clone(), c.env_status.clone());
        if switching {
            c.active_env = Some(Environment {
                name: name.clone(),
                vars: Vec::new(),
                secrets: Vec::new(),
            });
        }
        c.env_status = Some(EnvStatus {
            env: name.clone(),
            state: EnvState::Loading,
        });
        let root = c.tree.root.clone();
        let data = self.data_dir.clone();
        let stores = self.stores.clone();
        if self.active_collection == Some(ix) {
            cx.emit(WorkspaceEvent::EnvironmentChanged);
            cx.emit(WorkspaceEvent::SecretsChanged);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let job_root = root.clone();
            let job_name = name.clone();
            let result = cx
                .background_executor()
                .spawn(
                    async move { vault::load_job(&job_root, &job_name, data.as_ref(), &*stores) },
                )
                .await;
            this.update(cx, |w, cx| {
                w.finish_env_load(&root, generation, name, switching, previous, result, cx)
            })
            .ok();
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_env_load(
        &mut self,
        root: &Path,
        generation: u64,
        name: String,
        switching: bool,
        previous: (Option<Environment>, Option<EnvStatus>),
        result: vault::LoadResult,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.coll_ix(root) else {
            return;
        };
        let c = &mut self.collections[ix];
        if c.env_gen != generation {
            return;
        }
        let state = result.state();
        match result.loaded {
            Ok(loaded) => {
                c.active_env = Some(loaded.env);
                c.env_status = Some(EnvStatus { env: name, state });
            }
            Err(e) => {
                if switching {
                    c.active_env = previous.0;
                    c.env_status = previous.1;
                    cx.emit(WorkspaceEvent::Notice(
                        format!("Cannot switch environment: {e}").into(),
                    ));
                } else {
                    c.env_status = Some(EnvStatus { env: name, state });
                    cx.emit(WorkspaceEvent::Notice(
                        format!("Environment reload failed: {e}").into(),
                    ));
                }
            }
        }
        if self.active_collection == Some(ix) {
            cx.emit(WorkspaceEvent::EnvironmentChanged);
            cx.emit(WorkspaceEvent::SecretsChanged);
        }
        cx.notify();
    }

    /// True when the active collection's open root may share its keychain secrets.
    pub fn can_share_secrets(&self) -> bool {
        self.active_collection().is_some_and(|c| {
            matches!(
                c.env_status.as_ref().map(|s| &s.state),
                Some(EnvState::Loaded {
                    can_share: true,
                    ..
                })
            )
        })
    }

    /// Run the keychain probe off the UI thread; the panel disables keychain actions on failure.
    pub fn probe_keychain(&mut self, cx: &mut Context<Self>) {
        let stores = self.stores.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match stores.get(wisp_storage::vault::StoreKind::Keychain) {
                        Some(s) => s.probe().map_err(|e| e.to_string()),
                        None => Err("the keychain is not available".to_string()),
                    }
                })
                .await;
            this.update(cx, |w, cx| {
                w.keychain_probe = Some(result);
                cx.emit(WorkspaceEvent::SecretsChanged);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// An action the user asked for: confirm first when it has a confirmation, else run it.
    pub fn request_env_action(&mut self, action: EnvAction, cx: &mut Context<Self>) {
        let Some(c) = self.active_collection() else {
            return;
        };
        let Some(env) = c.active_env.as_ref().map(|e| e.name.clone()) else {
            cx.emit(WorkspaceEvent::Notice("Select an environment first".into()));
            return;
        };
        let root = c.tree.root.clone();
        match vault::first_prompt(&action, &env, &root) {
            Some(spec) => cx.emit(WorkspaceEvent::Prompt(spec)),
            None => self.run_env_action(root, env, action, cx),
        }
    }

    /// The share action after the folder picker returned `new_root`: only from the open root, and
    /// only when it passed the binding check.
    pub fn request_share(&mut self, new_root: PathBuf, cx: &mut Context<Self>) {
        if !self.can_share_secrets() {
            cx.emit(WorkspaceEvent::Notice(
                "Not shared: keychain secrets are not enabled for this folder".into(),
            ));
            return;
        }
        self.request_env_action(EnvAction::Share { new_root }, cx);
    }

    /// The answer to a prompt: run what the pressed button stands for.
    pub fn answer_prompt(&mut self, spec: &PromptSpec, button: usize, cx: &mut Context<Self>) {
        if let Some(action) = spec.answer(button) {
            self.run_env_action(spec.root.clone(), spec.env.clone(), action, cx);
        }
    }

    /// Run a confirmed action on a background task, then show its outcome and reload.
    pub fn run_env_action(
        &mut self,
        root: PathBuf,
        env: String,
        action: EnvAction,
        cx: &mut Context<Self>,
    ) {
        let data = self.data_dir.clone();
        let stores = self.stores.clone();
        let refresh_store = self.refresh_store.clone();
        let coll_id = self
            .coll_ix(&root)
            .and_then(|ix| match &self.collections[ix].coll_id {
                IdState::Known(id) => Some(id.clone()),
                _ => None,
            });
        cx.spawn(async move |this, cx| {
            let (r, e, a) = (root.clone(), env.clone(), action.clone());
            let (outcome, forget_err) = cx
                .background_executor()
                .spawn(async move {
                    let outcome = vault::run(
                        &a,
                        &vault::OpEnv {
                            root: &r,
                            env: &e,
                            data: data.as_ref(),
                            stores: &*stores,
                        },
                    );
                    // The moved environment's stored refresh tokens go too, off the UI thread.
                    let forget_err = match (&a, &outcome, &coll_id) {
                        (EnvAction::MoveToFile { .. }, vault::Outcome::Done(_), Some(id)) => {
                            refresh_store.forget_environment(id, &e).err()
                        }
                        _ => None,
                    };
                    (outcome, forget_err)
                })
                .await;
            this.update(cx, |w, cx| {
                w.finish_env_action(root, env, action, outcome, forget_err, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_env_action(
        &mut self,
        root: PathBuf,
        env: String,
        action: EnvAction,
        outcome: vault::Outcome,
        forget_err: Option<wisp_core::auth::oauth2::RefreshStoreError>,
        cx: &mut Context<Self>,
    ) {
        if let Some(spec) = vault::prompt_for(&outcome, &action, &env, &root) {
            cx.emit(WorkspaceEvent::Prompt(spec));
            return;
        }
        let done = matches!(outcome, vault::Outcome::Done(_));
        if let vault::Outcome::Done(t) | vault::Outcome::Notice(t) = outcome {
            cx.emit(WorkspaceEvent::Notice(t.into()));
        }
        if done && let Some(ix) = self.coll_ix(&root) {
            match action {
                EnvAction::NewId => {
                    if let Some(id) = refresh_known_id(&mut self.collections[ix]) {
                        self.on_id_known(id, cx);
                        cx.emit(WorkspaceEvent::CookiesChanged);
                    }
                }
                EnvAction::MoveToFile { .. } => {
                    if let Some(e) = &forget_err {
                        cx.emit(WorkspaceEvent::Notice(
                            format!("Moved to file; stored refresh tokens were not deleted: {e}")
                                .into(),
                        ));
                    }
                }
                _ => {}
            }
        }
        // Reload so the status and the names reflect what happened (or did not).
        if let Some(ix) = self.coll_ix(&root) {
            let active = self.collections[ix]
                .active_env
                .as_ref()
                .map(|e| e.name.clone());
            if active.as_deref() == Some(env.as_str()) {
                self.load_env_at(ix, env, false, cx);
            }
        }
    }

    fn take_token_banners(&mut self, cx: &mut Context<Self>) {
        let new = self.refresh_store.take_banners();
        if !new.is_empty() {
            self.token_banners.extend(new);
            cx.emit(WorkspaceEvent::SecretsChanged);
            cx.notify();
        }
    }

    /// Handle a coalesced batch of watcher events for the collection at `root`.
    fn on_disk_changes(&mut self, root: &Path, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(ix) = self.coll_ix(root) else {
            return;
        };
        let mut paths: Vec<&PathBuf> = paths.iter().collect();
        paths.sort();
        paths.dedup();
        for p in &paths {
            if let Some(tab) = self.collections[ix]
                .group
                .tabs
                .iter()
                .find(|t| t.read(cx).path.as_deref() == Some(p.as_path()))
                .cloned()
            {
                self.apply_disk_state(&tab, root, cx);
            }
        }
        if paths.iter().any(|p| affects_tree(p)) {
            self.reload_tree_at(ix, cx);
        }
        let env_name = self.collections[ix]
            .active_env
            .as_ref()
            .map(|e| e.name.clone());
        let active = self.active_collection == Some(ix);
        if let Some(name) = env_name
            && paths.iter().any(|p| affects_env(p, &name))
        {
            self.load_env_at(ix, name, false, cx);
        } else if paths.iter().any(|p| !affects_tree(p))
            && let Ok(names) = list_environments(root)
        {
            self.collections[ix].environments = names;
            if active {
                cx.emit(WorkspaceEvent::EnvironmentChanged);
            }
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
        let Some(root) = self.owner_of(tab).and_then(|o| self.root_of(o)) else {
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
            .tabs()
            .iter()
            .position(|t| t.read(cx).path.as_deref() == Some(rel))
        {
            self.activate_tab(ix, cx);
            return;
        }
        let Some(root) = self.active_collection().map(|c| c.tree.root.clone()) else {
            return;
        };
        match load_request(&root.join(rel)) {
            Ok(req) => {
                let tab = cx.new(|_| RequestTab::from_saved(rel.to_path_buf(), req));
                self.push_tab(tab, cx);
            }
            Err(e) => cx.emit(WorkspaceEvent::Notice(
                format!("Cannot open {}: {e}", rel.display()).into(),
            )),
        }
    }

    /// Add `tab` to the showing group and make it the active tab.
    fn push_tab(&mut self, tab: Entity<RequestTab>, cx: &mut Context<Self>) {
        let g = self.group_mut();
        g.tabs.push(tab);
        g.active = Some(g.tabs.len() - 1);
        cx.emit(WorkspaceEvent::TabsChanged);
        cx.emit(WorkspaceEvent::ActiveTabChanged);
        cx.notify();
    }

    pub fn new_scratch_tab(&mut self, url: String, cx: &mut Context<Self>) -> Entity<RequestTab> {
        let tab = cx.new(|_| RequestTab::new_scratch(url));
        self.push_tab(tab.clone(), cx);
        tab
    }

    pub fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        let g = self.group_mut();
        if ix < g.tabs.len() && g.active != Some(ix) {
            g.active = Some(ix);
            cx.emit(WorkspaceEvent::ActiveTabChanged);
            cx.notify();
        }
    }

    fn close_tab_in(&mut self, owner: Owner, ix: usize, cx: &mut Context<Self>) {
        let g = self.group_of_mut(owner);
        if ix >= g.tabs.len() {
            return;
        }
        let tab = g.tabs.remove(ix);
        g.active = match g.active {
            _ if g.tabs.is_empty() => None,
            Some(a) if a > ix => Some(a - 1),
            Some(a) if a < ix => Some(a),
            _ => Some(ix.min(g.tabs.len() - 1)),
        };
        self.release_tab(&tab, cx);
        cx.emit(WorkspaceEvent::TabsChanged);
        cx.emit(WorkspaceEvent::ActiveTabChanged);
        cx.notify();
    }

    /// Everything a tab owns outside itself: its in-flight send and authorization are
    /// cancelled, its routes dropped and its body released.
    fn release_tab(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
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
        // A tab closed while waiting for the browser stops the flow and frees the loopback port.
        let waiting = tab.read(cx).authorize.waiting_id();
        if let Some(cmd) = close_auth(&mut self.auth_routes, &closed, waiting) {
            self.engine.submit(cmd);
        }
    }

    pub fn active_tab(&self) -> Option<Entity<RequestTab>> {
        let g = self.group();
        g.active.and_then(|ix| g.tabs.get(ix).cloned())
    }

    pub fn scope_for(&self, rel: Option<&Path>) -> Scope<'_> {
        match self.active_collection() {
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

    /// Send the tab's request. `Ok(None)` means the submit waits for the collection id to be
    /// created off the UI thread (the first Send in a collection that has none).
    pub fn send(
        &mut self,
        tab: &Entity<RequestTab>,
        cx: &mut Context<Self>,
    ) -> Result<Option<RequestId>, ResolveError> {
        let (draft, path, previous, origin, pending) = {
            let t = tab.read(cx);
            (
                t.draft.clone(),
                t.path.clone(),
                (t.response.phase == Phase::Sending)
                    .then_some(t.response.request_id)
                    .flatten(),
                t.from_history.clone(),
                t.response.phase == Phase::Sending && t.response.request_id.is_none(),
            )
        };
        // A send already waiting for the collection id is not doubled.
        if pending {
            return Ok(None);
        }
        let root = self.owner_of(tab).and_then(|o| self.root_of(o));
        let c = root
            .as_deref()
            .and_then(|r| self.coll_ix(r))
            .map(|i| &self.collections[i]);
        let plan = prepare_send(
            &draft,
            path.as_deref(),
            origin.as_ref(),
            c.map(|c| &c.tree),
            &self.empty_defaults,
            c.and_then(|c| c.active_env.as_ref()),
        )
        .inspect_err(|e| {
            // A refused body is explained here; callers skip their own notice.
            if let Some(message) = refused_body_message(e) {
                cx.emit(WorkspaceEvent::Notice(message.to_string().into()));
            }
        })?;
        let mut plan = plan;
        if let Some(auth) = plan.prepared.auth.as_mut() {
            mark_persist(&self.refresh_store, c, auth);
        }
        if let Some(old) = previous {
            self.engine.submit(Command::Cancel(old));
        }
        if let Some(partition) = self.partition_for(root.as_deref()) {
            return Ok(Some(
                self.submit_plan(tab, draft, plan, partition, root, cx),
            ));
        }
        // No id yet: create it off the UI thread, then submit.
        self.show_sending(tab, None, cx);
        self.next_wait += 1;
        let token = self.next_wait;
        self.id_waits.insert(tab.entity_id(), token);
        let tab = tab.downgrade();
        self.resolve_id_then(root.clone(), cx, move |this, partition, cx| {
            let Some(tab) = tab.upgrade() else {
                return;
            };
            // Cancelled (or replaced) while the id was created: the send is dropped.
            if !claim_id_wait(&mut this.id_waits, tab.entity_id(), token) {
                return;
            }
            let Some(partition) = partition else {
                return;
            };
            this.submit_plan(&tab, draft, plan, partition, root, cx);
        });
        Ok(None)
    }

    fn submit_plan(
        &mut self,
        tab: &Entity<RequestTab>,
        draft: Request,
        mut plan: SendPlan,
        partition: Option<String>,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> RequestId {
        plan.prepared.client.collection = partition;
        let scope_path = plan.scope_path;
        let id = self.engine.submit(Command::Send(plan.prepared));
        if let Some(notice) = plan.notice {
            cx.emit(WorkspaceEvent::Notice(notice.into()));
        }
        self.routes.insert(id, tab.downgrade());
        self.sent.insert(
            id,
            Sent {
                request: recorded_request(scope_path.as_deref()),
                draft,
                root,
            },
        );
        self.show_sending(tab, Some(id), cx);
        id
    }

    /// Start the tab from a clean response so nothing of the previous one leaks into this
    /// one's display or history entry; the old body goes back to the store.
    fn show_sending(
        &mut self,
        tab: &Entity<RequestTab>,
        id: Option<RequestId>,
        cx: &mut Context<Self>,
    ) {
        let old_body = tab.update(cx, |t, cx| {
            let old = t.response.body.take();
            t.response = ResponseState {
                phase: Phase::Sending,
                request_id: id,
                ..ResponseState::default()
            };
            cx.emit(TabEvent::ResponseChanged);
            cx.notify();
            old
        });
        if let Some(h) = old_body {
            self.release_body(&h);
        }
    }

    /// Start the interactive authorization for `tab`. Resolves like Send; an unresolved variable
    /// returns the error and submits nothing.
    pub fn authorize(
        &mut self,
        tab: &Entity<RequestTab>,
        cx: &mut Context<Self>,
    ) -> Result<(), ResolveError> {
        let (draft, path, origin, waiting) = {
            let t = tab.read(cx);
            (
                t.draft.clone(),
                t.path.clone(),
                t.from_history.clone(),
                t.authorize.waiting_id(),
            )
        };
        let root = self.owner_of(tab).and_then(|o| self.root_of(o));
        let c = root
            .as_deref()
            .and_then(|r| self.coll_ix(r))
            .map(|i| &self.collections[i]);
        let partition = self.partition_for(root.as_deref());
        let command = prepare_authorize(
            &draft,
            path.as_deref(),
            origin.as_ref(),
            c.map(|c| &c.tree),
            &self.empty_defaults,
            c.and_then(|c| c.active_env.as_ref()),
            partition.clone().flatten(),
        )?;
        let Some(mut command) = command else {
            return Ok(());
        };
        if let Command::AuthorizeStart { auth, .. } = &mut command {
            mark_persist(&self.refresh_store, c, auth);
        }
        if partition.is_none() {
            // No id yet: create it off the UI thread, then start the flow.
            let tab = tab.downgrade();
            self.resolve_id_then(root, cx, move |this, partition, cx| {
                if let (Some(_), Some(tab)) = (partition, tab.upgrade()) {
                    let _ = this.authorize(&tab, cx);
                }
            });
            return Ok(());
        }
        if let Some(old) = waiting {
            self.engine.submit(Command::AuthorizeCancel(old));
            self.auth_routes.remove(&old);
        }
        let id = self.engine.submit(command);
        self.auth_routes.insert(id, tab.downgrade());
        tab.update(cx, |t, cx| {
            t.set_authorize(AuthorizeState::Waiting { id, url: None }, cx)
        });
        Ok(())
    }

    /// Cancel the tab's waiting authorization; the engine answers with `AuthFailed`.
    pub fn authorize_cancel(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        if let Some(id) = tab.read(cx).authorize.waiting_id() {
            self.engine.submit(Command::AuthorizeCancel(id));
        }
    }

    fn on_auth_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let (id, terminal) = match &event {
            Event::AuthUrl { id, .. } => (*id, false),
            Event::AuthCompleted { id } | Event::AuthFailed { id, .. } => (*id, true),
            _ => return,
        };
        let route = self.auth_routes.get(&id).cloned();
        let tab = route.as_ref().and_then(|w| w.upgrade());
        let waiting = tab.as_ref().and_then(|t| t.read(cx).authorize.waiting_id());
        let routing = route_auth_event(route.is_some(), tab.is_some(), waiting, id, terminal);
        if routing.remove_route {
            self.auth_routes.remove(&id);
        }
        let Some(tab) = tab.filter(|_| routing.apply) else {
            return;
        };
        match event {
            Event::AuthUrl { url, .. } => {
                if may_open_in_browser(&url) {
                    cx.open_url(&url);
                } else {
                    cx.emit(WorkspaceEvent::Notice(
                        "Not opening the authorization URL in the browser: only https, or http \
                         on this machine, is allowed. Copy it to open it yourself."
                            .into(),
                    ));
                }
                tab.update(cx, |t, cx| {
                    t.set_authorize(AuthorizeState::Waiting { id, url: Some(url) }, cx)
                });
            }
            Event::AuthCompleted { .. } => {
                tab.update(cx, |t, cx| t.set_authorize(AuthorizeState::Authorized, cx));
            }
            Event::AuthFailed { error, .. } => {
                let state = failure_state(&error);
                tab.update(cx, |t, cx| t.set_authorize(state, cx));
            }
            _ => {}
        }
    }

    pub fn cancel(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) {
        let t = tab.read(cx);
        if t.response.phase == Phase::Sending {
            match t.response.request_id {
                Some(id) => {
                    self.engine.submit(Command::Cancel(id));
                }
                None => {
                    // Still waiting for the collection id: drop the pending submit.
                    if self.id_waits.remove(&tab.entity_id()).is_some() {
                        tab.update(cx, |t, cx| {
                            t.response.phase = Phase::Cancelled;
                            cx.emit(TabEvent::ResponseChanged);
                            cx.notify();
                        });
                    }
                }
            }
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
        self.take_token_banners(cx);
        let id = match &event {
            Event::Started { id, .. }
            | Event::Headers { id, .. }
            | Event::Progress { id, .. }
            | Event::Completed { id, .. }
            | Event::Failed { id, .. }
            | Event::Cancelled { id } => *id,
            Event::AuthUrl { .. } | Event::AuthCompleted { .. } | Event::AuthFailed { .. } => {
                self.on_auth_event(event, cx);
                return;
            }
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
                    t.response.dns = None;
                    t.response.connect = None;
                    t.response.size = None;
                    t.response.error = None;
                }
                Event::Progress { bytes, .. } => t.response.received = *bytes,
                Event::Completed {
                    total,
                    size,
                    body,
                    timings,
                    ..
                } => {
                    old_body = t.response.body.take();
                    // Read now: the handle is released when the next request replaces it.
                    stored_body = bodies.read(body, 0, BODY_CAP + 1).ok();
                    t.response.phase = Phase::Done;
                    t.response.total = Some(*total);
                    t.response.dns = timings.dns;
                    t.response.connect = timings.connect;
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
                Event::AuthUrl { .. } | Event::AuthCompleted { .. } | Event::AuthFailed { .. } => {}
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
        // The entry belongs to the collection the tab was sent from, not the active one.
        let Some(task_root) = sent.root.clone() else {
            return;
        };
        let Some((cix, job)) =
            history_target(&self.collections, &task_root, self.data_dir.as_ref())
        else {
            return;
        };
        let job = match job {
            Ok(j) => j,
            Err(reason) => {
                if matches!(self.collections[cix].history, HistoryState::Unresolved) {
                    self.history_off(&task_root, reason, cx);
                }
                return;
            }
        };
        let body = history_ui::body_to_store(self.settings.history_bodies, body);
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
            dns: r.dns,
            connect: r.connect,
            size: r.size,
            response_headers: r.headers.clone(),
        };
        cx.spawn(async move |this, cx| {
            let work = cx.background_executor().spawn(async move {
                let store = job.open().map_err(|e| (true, e))?;
                store
                    .append(new, body.as_deref())
                    .map(|_| store)
                    .map_err(|e| (false, e.to_string()))
            });
            let result = work.await;
            this.update(cx, |this, cx| {
                // The collection was closed while this was in flight: its result is stale.
                let Some(cix) = this.coll_ix(&task_root) else {
                    return;
                };
                match result {
                    Ok(store) => {
                        if matches!(this.collections[cix].history, HistoryState::Unresolved) {
                            this.collections[cix].history = HistoryState::Ready(store);
                        }
                        cx.emit(WorkspaceEvent::HistoryChanged);
                    }
                    Err((true, reason)) => this.history_off(&task_root, reason, cx),
                    Err((false, reason)) => {
                        tracing::warn!(error = %reason, "history append failed");
                        this.history_notice(
                            &task_root,
                            format!("History entry dropped: {reason}"),
                            cx,
                        );
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn history_off(&mut self, root: &Path, reason: String, cx: &mut Context<Self>) {
        if let Some(ix) = self.coll_ix(root) {
            self.collections[ix].history = HistoryState::Off(reason.clone());
        }
        self.history_notice(root, format!("History is off: {reason}"), cx);
    }

    /// A handle to the active collection's history, or the reason there is none.
    pub fn history_job(&self) -> Result<HistoryJob, String> {
        let Some(c) = self.active_collection() else {
            return Err("Open a collection to see its history".into());
        };
        history_job_of(c, self.data_dir.as_ref())
    }

    /// Open a history entry in a new tab. The stored request is read on the
    /// background executor; the body is shown from its stored file, not copied.
    pub fn open_history_entry(&mut self, entry: HistoryEntry, cx: &mut Context<Self>) {
        let job = match self.history_job() {
            Ok(j) => j,
            Err(reason) => {
                cx.emit(WorkspaceEvent::Notice(reason.into()));
                return;
            }
        };
        let Some(root) = self.active_collection().map(|c| c.tree.root.clone()) else {
            return;
        };
        let bodies = self.engine.bodies();
        let entry_for_job = entry.clone();
        cx.spawn(async move |this, cx| {
            let work = cx
                .background_executor()
                .spawn(async move { job.load_entry(&entry_for_job, &bodies) });
            let result = work.await;
            this.update(cx, |this, cx| {
                let still_open = this.coll_ix(&root);
                match result {
                    Ok((_, handle)) if still_open.is_none() => {
                        if let Some(h) = handle {
                            this.release_body(&h);
                        }
                    }
                    Ok((mut stored, handle)) => {
                        let cix = still_open.unwrap_or_default();
                        let origin = sanitized_origin(&entry, &mut stored.request);
                        let response =
                            history_ui::stored_response(&entry, stored.response_headers, handle);
                        let tab =
                            cx.new(|_| RequestTab::from_history(stored.request, response, origin));
                        let g = this.group_of_mut(Owner::Collection(cix));
                        g.tabs.push(tab);
                        g.active = Some(g.tabs.len() - 1);
                        cx.emit(WorkspaceEvent::TabsChanged);
                        cx.emit(WorkspaceEvent::ActiveTabChanged);
                        cx.notify();
                    }
                    Err(e) if still_open.is_some() => cx.emit(WorkspaceEvent::Notice(
                        format!("Cannot open history entry: {e}").into(),
                    )),
                    Err(_) => {}
                }
            })
            .ok();
        })
        .detach();
    }

    /// Remove the history of the collection at `root`, if it is still open; the store I/O runs
    /// on the background executor.
    pub fn clear_history(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let Some(ix) = self.coll_ix(&root) else {
            return;
        };
        let job = match history_job_of(&self.collections[ix], self.data_dir.as_ref()) {
            Ok(j) => j,
            Err(reason) => {
                cx.emit(WorkspaceEvent::Notice(reason.into()));
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let work = cx.background_executor().spawn(async move { job.clear() });
            let result = work.await;
            this.update(cx, |this, cx| {
                if this.coll_ix(&root).is_none() {
                    return;
                }
                match result {
                    Ok(()) => {
                        cx.emit(WorkspaceEvent::Notice("History cleared".into()));
                        cx.emit(WorkspaceEvent::HistoryChanged);
                    }
                    Err(e) => cx.emit(WorkspaceEvent::Notice(
                        format!("Cannot clear history: {e}").into(),
                    )),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Show a history Notice, once per collection.
    fn history_notice(&mut self, root: &Path, text: String, cx: &mut Context<Self>) {
        let Some(ix) = self.coll_ix(root) else {
            return;
        };
        if !std::mem::replace(&mut self.collections[ix].history_notified, true) {
            cx.emit(WorkspaceEvent::Notice(text.into()));
        }
    }

    pub fn save(&mut self, tab: &Entity<RequestTab>, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let Some(root) = self.owner_of(tab).and_then(|o| self.root_of(o)) else {
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
        for tab in &self.group().tabs {
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

    pub fn on_node_deleted(&mut self, root: &Path, rel: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self.coll_ix(root) else {
            return;
        };
        for tab in &self.collections[ix].group.tabs {
            tab.update(cx, |t, cx| {
                if t.path.as_deref().is_some_and(|p| p.starts_with(rel)) {
                    t.set_disk_state(DiskState::DeletedOnDisk, cx);
                }
            });
        }
    }
}

/// The notice `Workspace::send` already showed for a refused redacted body, if
/// this error is that refusal. Callers must not replace it with their own.
pub(crate) fn refused_body_message(e: &ResolveError) -> Option<&str> {
    match e.problems.as_slice() {
        [
            Problem::Syntax {
                location: Location::Body,
                message,
            },
        ] if message.starts_with("Edit the request body") => Some(message),
        _ => None,
    }
}

/// Names only, never values or syntax messages (conventions: logging).
pub(crate) fn describe_problems(e: &ResolveError) -> String {
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

/// Cookie persistence and the collection id behind it. Every file operation is a background
/// job and runs one at a time; the UI thread only reads the cached id and the in-memory jar.
impl Workspace {
    /// `Some(partition)` once known (`Some(None)`: no collection), `None` while the id still
    /// has to be read or created. A root that is no longer open counts as no collection.
    fn partition_for(&self, root: Option<&Path>) -> Option<Option<String>> {
        let Some(c) = root
            .and_then(|r| self.coll_ix(r))
            .map(|i| &self.collections[i])
        else {
            return Some(None);
        };
        match &c.coll_id {
            IdState::Known(id) => Some(Some(id.clone())),
            IdState::Failed { partition, .. } => Some(Some(partition.clone())),
            IdState::Reading | IdState::Absent => None,
        }
    }

    fn partition_now(&self) -> Option<Option<String>> {
        self.partition_for(self.active_collection().map(|c| c.tree.root.as_path()))
    }

    /// The active collection's id, once known.
    fn known_id(&self) -> Option<String> {
        match self.active_collection().map(|c| &c.coll_id) {
            Some(IdState::Known(id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// The open collection's jar (or the no-collection jar); `None` while its id is unknown.
    fn current_jar(&self) -> Option<CookieJar> {
        Some(match self.partition_now()? {
            None => self.jars.jar(None),
            Some(p) => self.jars.jar(Some(&p)),
        })
    }

    /// Why cookies cannot be saved for the open collection, if so.
    fn cookie_unavailable(&self) -> Option<String> {
        let Some(c) = self.active_collection() else {
            return Some("open a collection first".into());
        };
        if self.data_dir.is_none() {
            return Some("the data directory is unknown".into());
        }
        match &c.coll_id {
            IdState::Failed { reason, .. } => Some(reason.clone()),
            _ => None,
        }
    }

    /// What the Cookies manager shows.
    pub fn cookie_panel(&self) -> CookiePanel {
        let name = self.active_collection().map(OpenCollection::name);
        let persistence = if let Some(reason) = self.cookie_unavailable() {
            Persistence::Unavailable(reason)
        } else if matches!(
            self.active_collection().map(|c| &c.coll_id),
            Some(IdState::Absent)
        ) {
            Persistence::Off
        } else {
            match self.known_id().and_then(|id| self.cookie_files.get(&id)) {
                Some(st) if st.loaded => match &st.error {
                    Some(e) => Persistence::Failed(e.clone()),
                    None if st.enabled => Persistence::On,
                    None => Persistence::Off,
                },
                _ => Persistence::Pending,
            }
        };
        CookiePanel {
            jar: self.current_jar(),
            name,
            persistence,
        }
    }

    /// Read the open collection's id on the background executor; never writes.
    fn start_id_read(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let r = root.clone();
            let state = cx
                .background_executor()
                .spawn(async move { read_id_state(&r) })
                .await;
            this.update(cx, |this, cx| this.apply_id(&root, state, true, cx))
                .ok();
        })
        .detach();
    }

    /// Record a resolved id for `root` if that collection is still open. A read result only
    /// lands while the read is the latest word; a known id is never replaced.
    fn apply_id(&mut self, root: &Path, state: IdState, from_read: bool, cx: &mut Context<Self>) {
        let Some(ix) = self.coll_ix(root) else {
            return;
        };
        let c = &self.collections[ix];
        if matches!(c.coll_id, IdState::Known(_))
            || (from_read && !matches!(c.coll_id, IdState::Reading))
        {
            return;
        }
        // Two open collections never share an id: the later one is refused, before anything
        // is loaded or written for it.
        if let IdState::Known(id) = &state {
            let mut ids: Vec<Option<&str>> = self
                .collections
                .iter()
                .map(|c| match &c.coll_id {
                    IdState::Known(i) => Some(i.as_str()),
                    _ => None,
                })
                .collect();
            ids[ix] = Some(id.as_str());
            if let Some(ClashPlan {
                refused,
                kept,
                adopt_landing_first,
            }) = plan_clash(&ids, ix)
            {
                let text = format!(
                    "\"{}\" has the same collection id as the open \"{}\" ({}); not opened. \
                     Change the id in its collection.toml to open both.",
                    self.collections[refused].name(),
                    self.collections[kept].name(),
                    self.collections[kept].tree.root.display(),
                );
                if !adopt_landing_first {
                    self.close_collection(ix, cx);
                } else {
                    // The newer one finished reading first; the older one's id just landed.
                    self.collections[ix].coll_id = state;
                    self.close_collection(refused, cx);
                    if let Some(ix) = self.coll_ix(root) {
                        let IdState::Known(id) = self.collections[ix].coll_id.clone() else {
                            return;
                        };
                        self.on_id_known(id, cx);
                    }
                }
                cx.emit(WorkspaceEvent::Notice(text.into()));
                return;
            }
        }
        self.collections[ix].coll_id = state.clone();
        match state {
            IdState::Known(id) => self.on_id_known(id, cx),
            IdState::Failed { reason, .. } => self.cookies_not_saved(&reason, cx),
            IdState::Reading | IdState::Absent => {}
        }
        cx.emit(WorkspaceEvent::CookiesChanged);
        cx.notify();
    }

    /// Run `then` once the open collection's partition is known, creating the id on the
    /// background executor when there is none. `None` when the collection changed meanwhile.
    fn resolve_id_then(
        &mut self,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Option<Option<String>>, &mut Context<Self>) + 'static,
    ) {
        if let Some(p) = self.partition_for(root.as_deref()) {
            then(self, Some(p), cx);
            return;
        }
        let Some(root) = root else {
            then(self, None, cx);
            return;
        };
        cx.spawn(async move |this, cx| {
            let r = root.clone();
            let state = cx
                .background_executor()
                .spawn(async move {
                    match resolve_collection_id(&r) {
                        Ok(id) => IdState::Known(id.to_string()),
                        Err(e) => IdState::failed(&r, e),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.apply_id(&root, state, false, cx);
                let p = if this.coll_ix(&root).is_some() {
                    this.partition_for(Some(&root))
                } else {
                    None
                };
                then(this, p, cx);
            })
            .ok();
        })
        .detach();
    }

    /// The first time a collection's id is known in this run, check and load its cookie file.
    fn on_id_known(&mut self, id: String, cx: &mut Context<Self>) {
        if self.data_dir.is_some() && !self.cookie_files.contains_key(&id) {
            let generation = self.jars.jar(Some(&id)).generation();
            self.cookie_files.insert(
                id.clone(),
                CookieStatus {
                    loaded: false,
                    enabled: false,
                    toggled: false,
                    error: None,
                    debounce: Debounce::new(generation),
                },
            );
            self.enqueue_cookie_job(CookieJob::Load { id }, cx);
        }
        self.ensure_cookie_ticker(cx);
    }

    fn cookies_not_saved(&mut self, reason: &str, cx: &mut Context<Self>) {
        tracing::warn!(error = %reason, "cookies are not saved");
        cx.emit(WorkspaceEvent::Notice(
            format!("Cookies are not saved: {reason}").into(),
        ));
    }

    fn enqueue_cookie_job(&mut self, job: CookieJob, cx: &mut Context<Self>) {
        self.cookie_queue.push_back(job);
        self.pump_cookies(cx);
    }

    /// Quit work: drain the queue, and per collection plan the last queued toggle and a
    /// final save of the snapshot. Marks saved generations. Covers every open collection.
    fn take_quit_work(&mut self) -> Option<(Vec<QuitWork>, DataDir)> {
        let data = self.data_dir.clone()?;
        let queued: Vec<CookieJob> = self.cookie_queue.drain(..).collect();
        let work = quit_work(
            self.saving_ids(),
            &queued,
            &mut self.cookie_files,
            &self.jars,
        );
        (!work.is_empty()).then_some((work, data))
    }

    fn pump_cookies(&mut self, cx: &mut Context<Self>) {
        if self.cookie_busy {
            return;
        }
        let Some(data) = self.data_dir.clone() else {
            self.cookie_queue.clear();
            return;
        };
        let Some(job) = self.cookie_queue.pop_front() else {
            return;
        };
        self.cookie_busy = true;
        let id = job.id().to_owned();
        cx.spawn(async move |this, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { job.run(&data) })
                .await;
            this.update(cx, |this, cx| {
                this.cookie_busy = false;
                this.cookie_done(&id, done, cx);
                this.pump_cookies(cx);
            })
            .ok();
        })
        .detach();
    }

    fn cookie_done(&mut self, id: &str, done: CookieDone, cx: &mut Context<Self>) {
        let jar = self.jars.jar(Some(id));
        let mut failure = None;
        if let Some(st) = self.cookie_files.get_mut(id) {
            failure = st.apply(&done);
            if let CookieDone::Loaded(Ok(Some(stored))) = done {
                // Cookies set while the load ran are newer and win.
                let mut records = cookie_ui::to_records(stored);
                records.extend(jar.snapshot(SystemTime::now()));
                jar.load(records);
            }
        }
        if let Some(e) = failure {
            self.cookies_not_saved(&e, cx);
        }
        self.ensure_cookie_ticker(cx);
        cx.emit(WorkspaceEvent::CookiesChanged);
        cx.notify();
    }

    /// Hand the jar's persistent cookies to a save job and mark this generation as saved.
    fn queue_cookie_save(&mut self, id: &str, cx: &mut Context<Self>) {
        let jar = self.jars.jar(Some(id));
        let generation = jar.generation();
        let cookies = cookie_ui::to_stored(jar.snapshot(SystemTime::now()));
        if let Some(st) = self.cookie_files.get_mut(id) {
            st.debounce.saved(generation);
        }
        self.enqueue_cookie_job(
            CookieJob::Save {
                id: id.to_owned(),
                cookies,
            },
            cx,
        );
    }

    /// Ids of the open collections whose file is on and healthy, so changes are saved.
    fn saving_ids(&self) -> Vec<String> {
        saving_ids_of(&self.collections, &self.cookie_files)
    }

    /// Save collection `ix`'s jar now if it changed since the last save.
    fn flush_cookies_for(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(IdState::Known(id)) = self.collections.get(ix).map(|c| c.coll_id.clone()) else {
            return;
        };
        if !self.cookie_files.get(&id).is_some_and(CookieStatus::saving) {
            return;
        }
        let generation = self.jars.jar(Some(&id)).generation();
        let dirty = self.cookie_files.get_mut(&id).is_some_and(|st| {
            st.debounce.observe(generation, Instant::now());
            st.debounce.dirty()
        });
        if dirty {
            self.queue_cookie_save(&id, cx);
        }
    }

    /// Watch the open collections' jars while persistence is on. The timer stops itself when
    /// there is nothing to watch.
    fn ensure_cookie_ticker(&mut self, cx: &mut Context<Self>) {
        if self.cookie_ticker.is_some() || self.saving_ids().is_empty() {
            return;
        }
        self.cookie_ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep = this.update(cx, |t, cx| {
                    let keep = t.cookie_tick(Instant::now(), cx);
                    if !keep {
                        t.cookie_ticker = None;
                    }
                    keep
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn cookie_tick(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        let ids = self.saving_ids();
        if ids.is_empty() {
            return false;
        }
        for id in ids {
            let generation = self.jars.jar(Some(&id)).generation();
            let due = self.cookie_files.get_mut(&id).is_some_and(|st| {
                st.debounce.observe(generation, now);
                st.debounce.due(now)
            });
            if due {
                self.queue_cookie_save(&id, cx);
            }
        }
        true
    }

    /// Turn "Keep cookies between sessions" on or off for the open collection. The in-memory
    /// jar is untouched. On creates the collection id if there is none.
    pub fn set_cookie_persistence(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(reason) = self.cookie_unavailable() {
            self.cookies_not_saved(&reason, cx);
            return;
        }
        let root = self.active_collection().map(|c| c.tree.root.clone());
        self.resolve_id_then(root, cx, move |this, partition, cx| {
            if partition.is_none() {
                return;
            }
            let Some(id) = this.known_id() else {
                if let Some(reason) = this.cookie_unavailable() {
                    this.cookies_not_saved(&reason, cx);
                }
                return;
            };
            let jar = this.jars.jar(Some(&id));
            let generation = jar.generation();
            let st = this.cookie_files.entry(id.clone()).or_insert(CookieStatus {
                loaded: false,
                enabled: false,
                toggled: false,
                error: None,
                debounce: Debounce::new(generation),
            });
            st.error = None;
            st.toggled = true;
            if on {
                st.enabled = true;
                st.debounce.saved(generation);
                let cookies = cookie_ui::to_stored(jar.snapshot(SystemTime::now()));
                this.enqueue_cookie_job(CookieJob::Enable { id, cookies }, cx);
            } else {
                st.enabled = false;
                this.enqueue_cookie_job(CookieJob::Disable { id }, cx);
            }
            this.ensure_cookie_ticker(cx);
            cx.emit(WorkspaceEvent::CookiesChanged);
            cx.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::Method;
    use wisp_core::request::{KeyValue, TextBody, Var};

    #[test]
    fn quit_save_only_when_saving_and_dirty() {
        assert!(quit_save_needed(true, true));
        assert!(!quit_save_needed(true, false));
        assert!(!quit_save_needed(false, true));
        assert!(!quit_save_needed(false, false));
    }

    #[test]
    fn quit_plan_disable_after_save_drops_the_save() {
        let p = plan_quit(&[QuitOp::Save, QuitOp::Disable], true, true);
        assert_eq!(
            p,
            QuitPlan {
                toggle: Some(QuitOp::Disable),
                save: false
            }
        );
    }

    #[test]
    fn quit_plan_last_toggle_wins_then_saves() {
        let p = plan_quit(&[QuitOp::Disable, QuitOp::Enable], false, true);
        assert_eq!(
            p,
            QuitPlan {
                toggle: Some(QuitOp::Enable),
                save: true
            }
        );
    }

    #[test]
    fn quit_plan_enable_forces_save_even_when_clean() {
        let want = QuitPlan {
            toggle: Some(QuitOp::Enable),
            save: true,
        };
        assert_eq!(plan_quit(&[QuitOp::Enable], false, false), want);
        assert_eq!(
            plan_quit(&[QuitOp::Disable, QuitOp::Enable], false, false),
            want
        );
    }

    #[test]
    fn quit_plan_empty_saves_when_dirty_and_enabled() {
        assert!(plan_quit(&[], true, true).save);
        assert!(!plan_quit(&[], true, false).save);
        assert!(!plan_quit(&[], false, true).save);
        assert_eq!(plan_quit(&[], true, true).toggle, None);
    }

    fn healthy() -> CookieStatus {
        let mut st = status();
        st.loaded = true;
        st.enabled = true;
        st
    }

    #[test]
    fn apply_loaded_outcomes() {
        let mut st = status();
        assert_eq!(st.apply(&CookieDone::Loaded(Ok(None))), None);
        assert!(st.loaded && !st.enabled && !st.saving());
        let mut st = status();
        assert_eq!(st.apply(&CookieDone::Loaded(Ok(Some(vec![])))), None);
        assert!(st.loaded && st.enabled && st.saving());
        let mut st = status();
        assert_eq!(
            st.apply(&CookieDone::Loaded(Err("bad".into()))),
            Some("bad".into())
        );
        assert!(st.loaded && st.enabled && st.error.is_some() && !st.saving());
    }

    #[test]
    fn apply_load_does_not_override_a_toggle() {
        let mut st = status();
        st.toggled = true;
        st.enabled = true;
        st.apply(&CookieDone::Loaded(Ok(None)));
        assert!(st.enabled);
        let mut st = status();
        st.toggled = true;
        st.apply(&CookieDone::Loaded(Err("bad".into())));
        assert!(!st.enabled && st.loaded);
    }

    #[test]
    fn apply_enable_and_disable_outcomes() {
        let mut st = healthy();
        assert_eq!(st.apply(&CookieDone::Enabled(Ok(()))), None);
        assert!(st.saving());
        assert_eq!(
            st.apply(&CookieDone::Enabled(Err("e".into()))),
            Some("e".into())
        );
        assert!(!st.enabled && !st.saving());
        let mut st = healthy();
        assert_eq!(st.apply(&CookieDone::Disabled(Ok(()))), None);
        assert!(st.saving());
        assert_eq!(
            st.apply(&CookieDone::Disabled(Err("d".into()))),
            Some("d".into())
        );
        assert!(st.enabled && st.error.is_some() && !st.saving());
    }

    #[test]
    fn save_error_pauses_saving_until_toggled() {
        let mut st = healthy();
        assert_eq!(st.apply(&CookieDone::Saved(Ok(()))), None);
        assert!(st.saving());
        assert_eq!(
            st.apply(&CookieDone::Saved(Err("s".into()))),
            Some("s".into())
        );
        assert!(!st.saving());
        // Toggle off then on clears the error (as set_cookie_persistence does).
        st.error = None;
        st.toggled = true;
        st.enabled = false;
        assert!(!st.saving());
        st.enabled = true;
        assert!(st.saving());
    }

    fn status() -> CookieStatus {
        CookieStatus {
            loaded: false,
            enabled: false,
            toggled: false,
            error: None,
            debounce: Debounce::new(0),
        }
    }

    #[test]
    fn toggle_on_wins_over_a_load_that_found_no_file() {
        let mut st = status();
        // Entry created by the toggle, which then turned persistence on.
        st.toggled = true;
        st.enabled = true;
        st.apply_loaded(false);
        assert!(st.loaded && st.enabled);
        st.debounce.observe(1, Instant::now());
        assert!(st.debounce.dirty());
    }

    #[test]
    fn load_decides_enabled_when_not_toggled() {
        let mut st = status();
        st.apply_loaded(false);
        assert!(!st.enabled);
        st.apply_loaded(true);
        assert!(st.enabled);
    }

    #[test]
    fn cancel_drops_a_send_waiting_for_the_id() {
        let mut waits: HashMap<u32, u64> = HashMap::new();
        waits.insert(1, 7);
        // Cancel removes it; the late resolve then submits nothing.
        waits.remove(&1);
        assert!(!claim_id_wait(&mut waits, 1, 7));
        // A replaced wait is also stale; the current one claims once.
        waits.insert(1, 8);
        assert!(!claim_id_wait(&mut waits, 1, 7));
        assert!(claim_id_wait(&mut waits, 1, 8));
        assert!(!claim_id_wait(&mut waits, 1, 8));
    }

    #[test]
    fn auth_event_for_the_starting_tab_is_routed() {
        let r = route_auth_event(true, true, Some(RequestId(5)), RequestId(5), false);
        assert_eq!(
            r,
            AuthRouting {
                apply: true,
                remove_route: false
            }
        );
    }

    #[test]
    fn auth_event_with_a_stale_id_is_dropped() {
        // The tab started a second authorization (id 6); the first one's event (5) is stale.
        let r = route_auth_event(true, true, Some(RequestId(6)), RequestId(5), false);
        assert!(!r.apply);
        let r = route_auth_event(true, true, Some(RequestId(6)), RequestId(5), true);
        assert_eq!(
            r,
            AuthRouting {
                apply: false,
                remove_route: true
            }
        );
    }

    #[test]
    fn auth_event_for_a_closed_tab_is_dropped_and_its_route_removed() {
        let r = route_auth_event(true, false, None, RequestId(5), false);
        assert_eq!(
            r,
            AuthRouting {
                apply: false,
                remove_route: true
            }
        );
        let r = route_auth_event(false, false, None, RequestId(5), false);
        assert_eq!(
            r,
            AuthRouting {
                apply: false,
                remove_route: false
            }
        );
    }

    #[test]
    fn terminal_auth_events_remove_the_route() {
        let r = route_auth_event(true, true, Some(RequestId(5)), RequestId(5), true);
        assert_eq!(
            r,
            AuthRouting {
                apply: true,
                remove_route: true
            }
        );
    }

    #[test]
    fn closing_a_waiting_tab_cancels_its_flow_and_removes_the_route() {
        let mut routes = HashMap::from([(RequestId(5), 1u32), (RequestId(9), 2u32)]);
        let cmd = close_auth(&mut routes, &1, Some(RequestId(5)));
        assert!(matches!(cmd, Some(Command::AuthorizeCancel(RequestId(5)))));
        assert!(!routes.contains_key(&RequestId(5)));
        assert!(routes.contains_key(&RequestId(9)));
        assert!(close_auth(&mut routes, &2, None).is_none());
        assert!(routes.is_empty());
    }

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
    fn refused_body_message_matches_only_the_refusal() {
        let refusal = ResolveError {
            problems: vec![Problem::Syntax {
                location: Location::Body,
                message: "Edit the request body before resending: x".into(),
            }],
        };
        assert_eq!(
            refused_body_message(&refusal),
            Some("Edit the request body before resending: x")
        );
        let other = ResolveError {
            problems: vec![Problem::Syntax {
                location: Location::Body,
                message: "bad".into(),
            }],
        };
        assert_eq!(refused_body_message(&other), None);
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
    fn collections_admit_activates_an_open_path_and_caps_at_eight() {
        let open: Vec<&Path> = vec![Path::new("/a"), Path::new("/b")];
        assert_eq!(admit(&open, Path::new("/b")), Admit::Existing(1));
        assert_eq!(admit(&open, Path::new("/c")), Admit::Add);
        let names: Vec<PathBuf> = (0..MAX_OPEN_COLLECTIONS)
            .map(|i| PathBuf::from(format!("/c{i}")))
            .collect();
        let full: Vec<&Path> = names.iter().map(PathBuf::as_path).collect();
        assert_eq!(admit(&full, Path::new("/ninth")), Admit::Full);
        // An open one is still activated at the cap.
        assert_eq!(admit(&full, Path::new("/c3")), Admit::Existing(3));
    }

    #[test]
    fn collections_same_id_at_another_path_refuses_the_later_one() {
        let ids = [Some("x"), Some("y"), Some("x")];
        assert_eq!(id_clash(&ids, 2), Some((2, 0)));
        assert_eq!(id_clash(&ids, 0), Some((2, 0)));
        assert_eq!(id_clash(&ids, 1), None);
        assert_eq!(id_clash(&[None, Some("x")], 1), None);
    }

    #[test]
    fn collections_closing_keeps_the_active_one_in_range() {
        assert_eq!(active_after_close(Some(2), 0, 2), Some(1));
        assert_eq!(active_after_close(Some(0), 2, 2), Some(0));
        assert_eq!(active_after_close(Some(2), 2, 2), Some(1));
        assert_eq!(active_after_close(Some(0), 0, 0), None);
    }

    #[test]
    fn collections_clash_plan_covers_both_landing_orders() {
        // The original (0) lands first: the clone, still reading, is not yet known.
        assert_eq!(plan_clash(&[Some("x"), None], 0), None);
        // The clone (1) lands: it is refused, nothing to adopt first.
        assert_eq!(
            plan_clash(&[Some("x"), Some("x")], 1),
            Some(ClashPlan {
                refused: 1,
                kept: 0,
                adopt_landing_first: false
            })
        );
        // The clone's id landed first; then the original's lands: record it, close the clone.
        assert_eq!(
            plan_clash(&[Some("x"), Some("x")], 0),
            Some(ClashPlan {
                refused: 1,
                kept: 0,
                adopt_landing_first: true
            })
        );
        assert_eq!(plan_clash(&[Some("x"), Some("y")], 1), None);
    }

    #[test]
    fn collections_close_plan_releases_every_tab_and_flushes_the_jar() {
        let p = plan_close(Some(1), 1, 3, 2, Some("abc"));
        assert_eq!(p.release_tabs, vec![0, 1]);
        assert_eq!(p.flush_id.as_deref(), Some("abc"));
        assert_eq!(p.active_after, Some(1));
        // No known id: nothing to flush. Closing the only collection leaves none active.
        let p = plan_close(Some(0), 0, 1, 0, None);
        assert!(p.release_tabs.is_empty());
        assert_eq!(p.flush_id, None);
        assert_eq!(p.active_after, None);
        // A collection before the active one shifts it down.
        assert_eq!(plan_close(Some(2), 0, 3, 1, None).active_after, Some(1));
    }

    #[test]
    fn collections_prompt_answer_acts_only_on_a_confirmed_open_collection() {
        // Dirty close or delete: cancelled does nothing, confirmed acts, a closed one is skipped.
        assert_eq!(confirmed_open(false, Some(1)), None);
        assert_eq!(confirmed_open(true, Some(1)), Some(1));
        assert_eq!(confirmed_open(true, None), None);
    }

    fn make_collection(dir: &Path, name: &str) -> OpenCollection {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("collection.toml"), "format = 1\n").unwrap();
        OpenCollection::new(open_collection(&root).unwrap(), Vec::new())
    }

    #[test]
    fn new_id_refreshes_the_cached_collection_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = make_collection(dir.path(), "a");
        let old = resolve_collection_id(&c.tree.root).unwrap().to_string();
        c.coll_id = IdState::Known(old.clone());
        assert_eq!(refresh_known_id(&mut c), None);
        let new = assign_collection_id(&c.tree.root).unwrap().to_string();
        assert_ne!(new, old);
        assert_eq!(refresh_known_id(&mut c), Some(new.clone()));
        assert_eq!(c.coll_id, IdState::Known(new));
    }

    #[test]
    fn collections_history_goes_to_the_sending_collection() {
        let dir = tempfile::tempdir().unwrap();
        let a = make_collection(dir.path(), "a");
        let b = make_collection(dir.path(), "b");
        let data = DataDir::at(dir.path().join("data"));
        // `a` is active, but the tab was sent from `b`: the job is `b`'s.
        let cols = [a, b];
        let sent = Sent {
            draft: Request::default(),
            request: None,
            root: Some(cols[1].tree.root.clone()),
        };
        let (ix, job) = history_target(&cols, sent.root.as_deref().unwrap(), Some(&data)).unwrap();
        assert_eq!(ix, 1);
        let job = job.unwrap();
        assert_eq!(job.root, cols[1].tree.root);
        assert_ne!(job.root, cols[0].tree.root);
        assert!(history_target(&cols, &dir.path().join("gone"), Some(&data)).is_none());
        job.open().unwrap();
        // Only `b` got an id: the entry's store was resolved for its collection.
        assert!(read_collection_id(&cols[1].tree.root).unwrap().is_some());
        assert!(read_collection_id(&cols[0].tree.root).unwrap().is_none());
    }

    #[test]
    fn collections_quit_saves_every_open_collection_with_saving_on() {
        let dir = tempfile::tempdir().unwrap();
        let jars = CookieJars::default();
        let mut files = HashMap::new();
        let mut cols = Vec::new();
        for id in ["id-a", "id-b", "id-off"] {
            let mut st = healthy();
            st.enabled = id != "id-off";
            st.debounce = Debounce::new(jars.jar(Some(id)).generation());
            files.insert(id.to_string(), st);
            // A change after the last save makes the jar dirty.
            jars.jar(Some(id)).clear();
            let mut c = make_collection(dir.path(), id);
            c.coll_id = IdState::Known(id.to_string());
            cols.push(c);
        }
        // `id-a` is first (the active one in a real workspace); `id-b` must still be saved.
        let saving = saving_ids_of(&cols, &files);
        assert_eq!(saving, ["id-a", "id-b"]);
        let work = quit_work(saving, &[], &mut files, &jars);
        let got: Vec<&str> = work.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(got, ["id-a", "id-b"]);
        assert!(work.iter().all(|w| w.plan.save));
        // A second pass finds both clean.
        let again = quit_work(vec!["id-a".into(), "id-b".into()], &[], &mut files, &jars);
        assert!(again.is_empty());
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

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    const URL: &str = "http://example.test/";

    fn origin(request: Option<&str>) -> HistoryOrigin {
        HistoryOrigin {
            at_ms: 0,
            body_stored: false,
            body_truncated: false,
            redacted: Vec::new(),
            adjustments: Vec::new(),
            request: request.map(String::from),
            url: URL.into(),
            stored_body: Body::None,
        }
    }

    fn env(name: &str, token: &str) -> Environment {
        Environment {
            name: name.into(),
            vars: vec![Var {
                name: "token".into(),
                value: token.into(),
            }],
            secrets: Vec::new(),
        }
    }

    fn auth_collection() -> (tempfile::TempDir, CollectionTree) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(r, "collection.toml", "format = 1\n");
        write(
            r,
            "users/folder.toml",
            "[auth]\ntype = \"bearer\"\ntoken = \"{{token}}\"\n",
        );
        write(
            r,
            "users/get.toml",
            "name = \"Get\"\nmethod = \"GET\"\nurl = \"http://example.test/\"\n",
        );
        let tree = open_collection(r).unwrap();
        (d, tree)
    }

    fn plan(
        tree: &CollectionTree,
        draft: &Request,
        o: &HistoryOrigin,
        e: &Environment,
    ) -> SendPlan {
        prepare_send(
            draft,
            None,
            Some(o),
            Some(tree),
            &CollectionDefaults::default(),
            Some(e),
        )
        .unwrap()
    }

    fn draft() -> Request {
        Request {
            url: URL.into(),
            ..Request::default()
        }
    }

    fn header<'a>(p: &'a SendPlan, name: &str) -> Option<&'a str> {
        p.prepared
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    fn entry_with(redacted: &[&str]) -> HistoryEntry {
        HistoryEntry {
            id: "e1".into(),
            at: 5,
            request: Some("users/get.toml".into()),
            method: Method::Get,
            url: URL.into(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb_ms: None,
            total_ms: None,
            dns_ms: None,
            connect_ms: None,
            size: None,
            body_stored: false,
            body_truncated: false,
            redacted: redacted.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn body_origin(body: Body, redacted: &[&str]) -> HistoryOrigin {
        let mut o = origin(None);
        o.stored_body = body;
        o.redacted = redacted.iter().map(|s| s.to_string()).collect();
        o
    }

    fn try_send(draft: &Request, o: &HistoryOrigin) -> Result<SendPlan, ResolveError> {
        prepare_send(
            draft,
            None,
            Some(o),
            None,
            &CollectionDefaults::default(),
            None,
        )
    }

    fn refusal(e: ResolveError) -> String {
        match e.problems.as_slice() {
            [Problem::Syntax { message, .. }] => message.clone(),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn json(text: &str) -> Body {
        Body::Json(TextBody {
            content: text.into(),
            ..TextBody::default()
        })
    }

    #[test]
    fn redacted_json_leaf_body_is_refused_until_edited() {
        let stored = json(r#"{"name":"x","webhook_secret":""}"#);
        let o = body_origin(stored.clone(), &["body.webhook_secret"]);
        let mut d = draft();
        d.body = stored;
        let msg = refusal(try_send(&d, &o).err().expect("refused"));
        assert_eq!(
            msg,
            "Edit the request body before resending: webhook_secret was not stored"
        );
        d.body = json(r#"{"name":"x","webhook_secret":"s"}"#);
        assert!(try_send(&d, &o).is_ok());
    }

    #[test]
    fn deeply_nested_body_is_refused_without_overflow() {
        let o = body_origin(json("{}"), &["body.s"]);
        let mut d = draft();
        d.body = json(&format!("{}1{}", "[".repeat(200), "]".repeat(200)));
        assert!(try_send(&d, &o).is_err());
        d.body = json(&"{\"a\":".repeat(50_000));
        assert!(try_send(&d, &o).is_err());
    }

    #[test]
    fn long_key_with_many_empty_leaves_is_fast() {
        let o = body_origin(json("{}"), &["body.s"]);
        let mut d = draft();
        let mut t = format!("{{\"{}\":{{", "k".repeat(400_000));
        for i in 0..100_000 {
            t.push_str(&format!("\"{i}\":\"\","));
        }
        t.push_str("\"z\":\"\"}}");
        d.body = json(&t);
        let start = std::time::Instant::now();
        assert!(try_send(&d, &o).is_ok());
        assert!(start.elapsed().as_secs_f32() < 1.0);
    }

    #[test]
    fn dropped_body_is_refused_until_edited() {
        let stored = Body::Xml(TextBody::default());
        let o = body_origin(stored.clone(), &["body"]);
        let mut d = draft();
        d.body = stored;
        let msg = refusal(try_send(&d, &o).err().expect("refused"));
        assert!(msg.ends_with("the body was not stored"));
        d.body = Body::Xml(TextBody {
            content: "<a/>".into(),
            ..TextBody::default()
        });
        assert!(try_send(&d, &o).is_ok());
    }

    #[test]
    fn redaction_refusal_follows_the_leaf_not_the_text() {
        let o = body_origin(json("{}"), &["body.webhook_secret"]);
        let mut d = draft();
        let cases = [
            ("{\n  \"name\": \"x\",\n  \"webhook_secret\": \"\"\n}", true),
            (" {\"name\":\"x\",\"webhook_secret\":\"\"}\n", true),
            ("{\"a\":{{id}},\"webhook_secret\":\"\"}", true),
            ("{\"webhook_secret\":", true),
            ("{\"webhook_secret\":\"s\"}", false),
            ("{\"webhook_secret\":\"{{secret}}\"}", false),
            ("{\"webhook_secret\":{{secret}}}", false),
            ("{\"name\":\"x\"}", false),
        ];
        for (body, refused) in cases {
            d.body = json(body);
            let got = try_send(&d, &o).err().is_some_and(|e| {
                matches!(e.problems.as_slice(),
                    [Problem::Syntax { message, .. }] if message.starts_with("Edit the request body"))
            });
            assert_eq!(got, refused, "{body}");
        }
    }

    #[test]
    fn dropped_body_whitespace_is_still_refused() {
        let o = body_origin(Body::Xml(TextBody::default()), &["body"]);
        let mut d = draft();
        d.body = Body::Xml(TextBody {
            content: " \n".into(),
            ..TextBody::default()
        });
        assert!(try_send(&d, &o).is_err());
    }

    #[test]
    fn entry_without_body_redaction_is_not_refused() {
        let o = body_origin(Body::None, &["Authorization"]);
        assert!(try_send(&draft(), &o).is_ok());
    }

    #[test]
    fn opening_an_entry_sanitises_it() {
        let mut req = draft();
        req.headers = vec![KeyValue {
            name: "Authorization".into(),
            value: String::new(),
            enabled: true,
        }];
        req.body = json("{}");
        let o = sanitized_origin(&entry_with(&["Authorization", "body.k"]), &mut req);
        assert!(!req.headers[0].enabled);
        assert_eq!(o.adjustments, vec!["disabled header Authorization"]);
        assert_eq!(o.redacted, vec!["body.k"]);
        assert_eq!(o.stored_body, json("{}"));
        assert_eq!(o.request.as_deref(), Some("users/get.toml"));
    }

    #[test]
    fn folder_bearer_token_follows_the_environment() {
        let (_d, tree) = auth_collection();
        let o = origin(Some("users/get.toml"));
        let dev = plan(&tree, &draft(), &o, &env("dev", "dev-tok"));
        let prod = plan(&tree, &draft(), &o, &env("prod", "prod-tok"));
        assert_eq!(header(&dev, "authorization"), Some("Bearer dev-tok"));
        assert_eq!(header(&prod, "authorization"), Some("Bearer prod-tok"));
        assert!(dev.notice.unwrap().contains("(dev)"));
        assert!(prod.notice.unwrap().contains("(prod)"));
    }

    #[test]
    fn edited_url_drops_the_folder_scope() {
        let (_d, tree) = auth_collection();
        let mut d = draft();
        d.url = "http://other.test/".into();
        let p = plan(
            &tree,
            &d,
            &origin(Some("users/get.toml")),
            &env("prod", "prod-tok"),
        );
        assert_eq!(header(&p, "authorization"), None);
        assert_eq!(p.scope_path, None);
        assert!(p.notice.unwrap().contains("URL was edited"));
    }

    #[test]
    fn missing_original_falls_back_to_collection_defaults() {
        let (d, _) = auth_collection();
        std::fs::remove_file(d.path().join("users/get.toml")).unwrap();
        let tree = open_collection(d.path()).unwrap();
        let p = plan(
            &tree,
            &draft(),
            &origin(Some("users/get.toml")),
            &env("dev", "t"),
        );
        assert_eq!(header(&p, "authorization"), None);
        assert!(p.notice.unwrap().contains("original request was not found"));
        // A tab with its own path is unaffected.
        let own = scope_path_for(&tree, Some(Path::new("a.toml")), Some(&origin(None)), URL);
        assert_eq!(own, (Some(PathBuf::from("a.toml")), false, false));
    }

    #[test]
    fn resend_chain_keeps_scope() {
        let (_d, tree) = auth_collection();
        let first = plan(
            &tree,
            &draft(),
            &origin(Some("users/get.toml")),
            &env("dev", "t"),
        );
        let recorded = recorded_request(first.scope_path.as_deref());
        assert_eq!(recorded.as_deref(), Some("users/get.toml"));
        let again = plan(
            &tree,
            &draft(),
            &origin(recorded.as_deref()),
            &env("prod", "p"),
        );
        assert_eq!(header(&again, "authorization"), Some("Bearer p"));
    }

    fn kv(name: &str, value: &str) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }

    #[test]
    fn redacted_fields_never_send_an_empty_value_or_override_the_environment() {
        let (_d, tree) = auth_collection();
        // As stored: secrets emptied, the names listed.
        let mut req = draft();
        req.vars = vec![Var {
            name: "token".into(),
            value: String::new(),
        }];
        req.auth = Auth::Bearer {
            token: String::new(),
        };
        req.headers = vec![
            kv("Authorization", ""),
            kv("X-Api-Key", ""),
            kv("Accept", "*/*"),
        ];
        req.params = vec![kv("api_key", "")];
        req.body = Body::Form(vec![kv("password", ""), kv("user", "bob")]);
        req.url = "http://example.test/?secret=&a=1".into();
        let redacted: Vec<String> = [
            "auth.token",
            "vars.token",
            "Authorization",
            "X-Api-Key",
            "params.api_key",
            "form.password",
            "query.secret",
        ]
        .map(String::from)
        .to_vec();
        let (adjustments, left) = sanitize_redacted(&mut req, &redacted);
        assert!(left.is_empty(), "{left:?}");
        assert!(req.vars.is_empty());
        assert_eq!(req.auth, Auth::Inherit);
        assert!(!req.headers[0].enabled && !req.headers[1].enabled && req.headers[2].enabled);
        assert!(!req.params[0].enabled);
        assert_eq!(req.url, "http://example.test/?a=1");
        let Body::Form(f) = &req.body else { panic!() };
        assert!(!f[0].enabled && f[1].enabled);
        assert!(adjustments.contains(&"disabled header Authorization".to_string()));
        assert!(adjustments.contains(&"reset auth to inherit".to_string()));

        let mut o = origin(Some("users/get.toml"));
        o.url = req.url.clone();
        o.adjustments = adjustments;
        let p = plan(&tree, &req, &o, &env("prod", "prod-tok"));
        // The environment's token is used, not an emptied var, and the folder's auth applies.
        assert_eq!(header(&p, "authorization"), Some("Bearer prod-tok"));
        assert_eq!(header(&p, "x-api-key"), None);
        assert_eq!(header(&p, "accept"), Some("*/*"));
        assert!(p.notice.unwrap().contains("disabled header X-Api-Key"));
    }

    #[test]
    fn redacted_path_var_is_dropped_and_the_resend_is_refused() {
        let mut d = draft();
        d.method = Method::Delete;
        d.url = "http://example.test/sessions/{token}".into();
        d.path = vec![Var {
            name: "token".into(),
            value: String::new(),
        }];
        // The resend path sanitises the stored request, then sends the result.
        let (adj, left) = sanitize_redacted(&mut d, &["path.token".into()]);
        assert!(d.path.is_empty() && left.is_empty() && adj.len() == 1);
        let o = body_origin(Body::None, &[]);
        let e = try_send(&d, &o).err().expect("must not be sent");
        assert!(
            e.problems
                .iter()
                .any(|p| matches!(p, Problem::MissingPathParam { name } if name == "token")),
            "{:?}",
            e.problems
        );
    }

    #[test]
    fn userinfo_is_stripped_from_a_redacted_url() {
        let mut req = draft();
        req.url = "http://@example.test/x".into();
        let (adj, left) = sanitize_redacted(&mut req, &["url.password".into()]);
        assert_eq!(req.url, "http://example.test/x");
        assert!(left.is_empty() && adj.len() == 1);
    }

    fn history_fixture(body: &[u8]) -> (tempfile::TempDir, DataDir, CollectionId, HistoryEntry) {
        let d = tempfile::tempdir().unwrap();
        let data = DataDir::at(d.path().join("data"));
        let id = CollectionId::parse("abcdef0123456789").unwrap();
        let store = HistoryStore::open(&data, &id);
        let new = NewEntry {
            at: SystemTime::now(),
            request_path: None,
            request: Request::default(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb: None,
            total: None,
            dns: None,
            connect: None,
            size: Some(body.len() as u64),
            response_headers: Vec::new(),
        };
        let entry = store.append(new, Some(body)).unwrap();
        (d, data, id, entry)
    }

    #[test]
    fn open_history_entry_body_is_file_backed() {
        let (d, data, id, entry) = history_fixture(b"hello history");
        let root = d.path().join("coll");
        write(
            &root,
            "collection.toml",
            &format!("format = 1\nid = \"{}\"\n", id.as_str()),
        );
        let job = HistoryJob {
            store: None,
            root,
            data: data.clone(),
        };
        let bodies = BodyStore::new(1024);
        let (_, handle) = job.load_entry(&entry, &bodies).unwrap();
        let handle = handle.expect("stored body");
        assert_eq!(handle.len, 13);
        assert!(!bodies.is_spilled(&handle));
        assert_eq!(bodies.read(&handle, 0, 100).unwrap(), b"hello history");
        let path = HistoryStore::open(&data, &id).body_path(&entry.id).unwrap();
        // Borrowed, not copied: changing the file changes what the handle reads.
        std::fs::write(&path, b"HELLO HISTORY").unwrap();
        assert_eq!(bodies.read(&handle, 0, 100).unwrap(), b"HELLO HISTORY");
        bodies.release(&handle);
        assert!(path.exists());
    }

    #[test]
    fn history_read_only_use_mints_no_id() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("coll");
        let text = "# mine\nformat = 1\n";
        write(&root, "collection.toml", text);
        let job = || HistoryJob {
            store: None,
            root: root.clone(),
            data: DataDir::at(d.path().join("data")),
        };
        assert!(job().list().unwrap().is_empty());
        job().clear().unwrap();
        let entry = history_fixture(b"x").3;
        let bodies = BodyStore::new(1024);
        assert!(job().load_entry(&entry, &bodies).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("collection.toml")).unwrap(),
            text
        );
    }

    fn oauth_code_draft(secret: &str) -> Request {
        let mut r = crate::draft::blank_request();
        r.url = "https://api.example/x".into();
        r.auth = Auth::OAuth2AuthCode {
            auth_url: "https://idp/a".into(),
            token_url: "https://idp/t".into(),
            client_id: "cid".into(),
            client_secret: secret.into(),
            scope: "r".into(),
            client_auth: wisp_core::request::ClientAuth::Basic,
            redirect_port: 0,
        };
        r
    }

    fn oauth_prepare(
        draft: &Request,
        tree: Option<&CollectionTree>,
        env: Option<&Environment>,
        collection: Option<&str>,
    ) -> Result<Option<Command>, ResolveError> {
        prepare_authorize(
            draft,
            None,
            None,
            tree,
            &CollectionDefaults::default(),
            env,
            collection.map(str::to_string),
        )
    }

    #[test]
    fn oauth_authorize_command_carries_environment_and_client() {
        let draft = oauth_code_draft("s");
        let (_dir, tree) = auth_collection();
        let without = oauth_prepare(&draft, Some(&tree), None, Some("col-1"))
            .unwrap()
            .unwrap();
        let e = env("prod", "t");
        let with = oauth_prepare(&draft, Some(&tree), Some(&e), Some("col-1"))
            .unwrap()
            .unwrap();
        let send = prepare_send(
            &draft,
            None,
            None,
            None,
            &CollectionDefaults::default(),
            None,
        )
        .unwrap()
        .prepared;
        for (cmd, expect_env) in [(without, None), (with, Some("prod".to_string()))] {
            let Command::AuthorizeStart {
                client,
                environment,
                auth,
                ..
            } = cmd
            else {
                panic!("not an AuthorizeStart")
            };
            assert_eq!(environment, expect_env);
            let mut want = send.client.clone();
            want.collection = Some("col-1".into());
            assert!(client == want);
            assert!(matches!(auth, EngineAuth::OAuth2AuthCode { .. }));
        }
        // A scratch tab has no collection.
        let Command::AuthorizeStart { client, .. } =
            oauth_prepare(&draft, None, None, None).unwrap().unwrap()
        else {
            panic!()
        };
        assert_eq!(client.collection, None);
    }

    #[test]
    fn oauth_authorize_blocks_on_unresolved_variables_and_other_kinds() {
        let draft = oauth_code_draft("{{missing}}");
        let err = oauth_prepare(&draft, None, None, None).unwrap_err();
        assert!(describe_problems(&err).contains("UnresolvedVar(missing)"));
        // Client credentials has no interactive step.
        let mut cc = crate::draft::blank_request();
        cc.auth = Auth::OAuth2ClientCredentials {
            token_url: "https://idp/t".into(),
            client_id: "c".into(),
            client_secret: String::new(),
            scope: String::new(),
            client_auth: wisp_core::request::ClientAuth::Basic,
        };
        cc.url = "https://api.example/x".into();
        assert!(oauth_prepare(&cc, None, None, None).unwrap().is_none());
    }

    #[test]
    fn oauth_browser_open_allows_only_https_and_loopback_http() {
        for ok in [
            "https://idp.example/authorize?x=1",
            "http://127.0.0.1:8123/authorize",
            "http://localhost/authorize",
            "http://[::1]:9/a",
        ] {
            assert!(may_open_in_browser(ok), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "http://idp.example/a",
            "http://127.0.0.1@evil.example/a",
            "http://localhost.evil.example/a",
            "ftp://127.0.0.1/a",
            "https://",
            "https://a b/",
            "",
        ] {
            assert!(!may_open_in_browser(bad), "{bad}");
        }
    }

    #[test]
    fn oauth_failure_state_shows_cancelled_for_cancel() {
        let e = |m: &str| EngineError {
            kind: wisp_core::ErrorKind::Auth,
            message: m.into(),
        };
        assert_eq!(failure_state(&e("cancelled")), AuthorizeState::Cancelled);
        assert_eq!(
            failure_state(&e("state mismatch")),
            AuthorizeState::Failed("state mismatch".into())
        );
    }
}

/// Workspace-level environment wiring over a `MemoryStore` and a temp data dir. The OS keychain
/// is never touched: the stores are injected.
#[cfg(test)]
mod env_wiring_tests {
    use super::*;
    use gpui::TestAppContext;
    use wisp_core::auth::oauth2::{Grant, RefreshTokenStore, TokenKey};
    use wisp_core::request::ClientAuth;
    use wisp_storage::vault::{MemoryStore, SecretStore, SecretValue, bind, secret_account};

    const ID: &str = "abcdef0123456789";

    struct Fx {
        _dir: tempfile::TempDir,
        root: PathBuf,
        store: Arc<MemoryStore>,
        ws: Entity<Workspace>,
    }

    /// A bound collection with a keychain-mode `dev`, a file-mode `file` and a broken `bad`.
    fn fixture(cx: &mut TestAppContext) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::at(dir.path().join("data"));
        let root = dir.path().join("coll");
        std::fs::create_dir_all(root.join("environments")).unwrap();
        std::fs::write(
            root.join("collection.toml"),
            format!("format = 1\nid = \"{ID}\"\n"),
        )
        .unwrap();
        let envs = root.join("environments");
        std::fs::write(
            envs.join("dev.toml"),
            "secrets = \"keychain\"\nsecret_names = [\"token\"]\n[vars]\nhost = \"h\"\n",
        )
        .unwrap();
        std::fs::write(envs.join("file.toml"), "[vars]\nhost = \"f\"\n").unwrap();
        std::fs::write(envs.join("bad.toml"), "this is = [not valid").unwrap();
        let root = root.canonicalize().unwrap();
        bind(&data, &CollectionId::parse(ID).unwrap(), &root).unwrap();
        let store = Arc::new(MemoryStore::new());
        store
            .set(
                &secret_account(&CollectionId::parse(ID).unwrap(), "dev", "token"),
                &SecretValue::new("kc-secret"),
            )
            .unwrap();
        let shared: vault::SharedStores = store.clone();
        let ws = cx.new(|cx| Workspace::new(Settings::default(), None, None, shared, cx));
        ws.update(cx, |w, _| w.data_dir = Some(data));
        Fx {
            _dir: dir,
            root,
            store,
            ws,
        }
    }

    fn open(fx: &Fx, cx: &mut TestAppContext) {
        let root = fx.root.clone();
        fx.ws
            .update(cx, |w, cx| w.open_collection(&root, cx).unwrap());
        cx.run_until_parked();
    }

    fn env_name(fx: &Fx, cx: &mut TestAppContext) -> Option<String> {
        fx.ws.read_with(cx, |w, _| {
            w.collections[0].active_env.as_ref().map(|e| e.name.clone())
        })
    }

    fn status_env(fx: &Fx, cx: &mut TestAppContext) -> Option<String> {
        fx.ws.read_with(cx, |w, _| {
            w.collections[0].env_status.as_ref().map(|s| s.env.clone())
        })
    }

    fn switch(fx: &Fx, name: &str, cx: &mut TestAppContext) {
        fx.ws
            .update(cx, |w, cx| w.set_environment(Some(name), cx).unwrap());
        cx.run_until_parked();
    }

    fn token_key(env: &str) -> TokenKey {
        TokenKey {
            grant: Grant::AuthCode,
            token_url: "https://idp.test/token".into(),
            client_id: "client".into(),
            scope: "read".into(),
            collection: Some(ID.into()),
            environment: Some(env.into()),
        }
    }

    #[gpui::test]
    fn stale_load_generation_is_dropped(cx: &mut TestAppContext) {
        let fx = fixture(cx);
        open(&fx, cx);
        switch(&fx, "file", cx);
        assert_eq!(env_name(&fx, cx).as_deref(), Some("file"));
        // A result from an older load of `dev` arrives late.
        let (root, data) = fx.ws.read_with(cx, |w, _| {
            (w.collections[0].tree.root.clone(), w.data_dir.clone())
        });
        let late = vault::load_job(&root, "dev", data.as_ref(), &*fx.store);
        fx.ws.update(cx, |w, cx| {
            let stale = w.collections[0].env_gen - 1;
            w.finish_env_load(&root, stale, "dev".into(), true, (None, None), late, cx);
        });
        assert_eq!(env_name(&fx, cx).as_deref(), Some("file"));
        assert_eq!(status_env(&fx, cx).as_deref(), Some("file"));
    }

    #[gpui::test]
    fn reopened_collection_ignores_the_old_instances_late_load(cx: &mut TestAppContext) {
        let fx = fixture(cx);
        open(&fx, cx);
        switch(&fx, "dev", cx);
        let old_gen = fx.ws.read_with(cx, |w, _| w.collections[0].env_gen);
        // Close and reopen; the new instance loads once, so a per-collection counter would
        // hand it the old instance's generation.
        fx.ws.update(cx, |w, cx| w.close_collection(0, cx));
        open(&fx, cx);
        switch(&fx, "file", cx);
        let (root, data) = fx.ws.read_with(cx, |w, _| {
            (w.collections[0].tree.root.clone(), w.data_dir.clone())
        });
        let late = vault::load_job(&root, "dev", data.as_ref(), &*fx.store);
        fx.ws.update(cx, |w, cx| {
            assert_ne!(w.collections[0].env_gen, old_gen);
            w.finish_env_load(&root, old_gen, "dev".into(), true, (None, None), late, cx);
        });
        assert_eq!(env_name(&fx, cx).as_deref(), Some("file"));
        assert_eq!(status_env(&fx, cx).as_deref(), Some("file"));
    }

    #[gpui::test]
    fn failed_switch_restores_the_previous_environment(cx: &mut TestAppContext) {
        let fx = fixture(cx);
        open(&fx, cx);
        switch(&fx, "dev", cx);
        assert_eq!(env_name(&fx, cx).as_deref(), Some("dev"));
        switch(&fx, "bad", cx);
        assert_eq!(env_name(&fx, cx).as_deref(), Some("dev"));
        assert_eq!(status_env(&fx, cx).as_deref(), Some("dev"));
        // The restored status is the loaded keychain one, not a placeholder.
        assert!(
            fx.ws
                .read_with(cx, |w, _| persist_refresh_for(&w.collections[0]))
        );
    }

    #[gpui::test]
    fn send_and_authorize_set_persist_from_environment_status(cx: &mut TestAppContext) {
        fn code() -> EngineAuth {
            EngineAuth::OAuth2AuthCode {
                auth_url: "a".into(),
                token_url: "t".into(),
                client_id: "c".into(),
                client_secret: String::new(),
                scope: String::new(),
                client_auth: ClientAuth::Body,
                redirect_port: 0,
                persist_refresh: false,
            }
        }
        fn persists(a: &EngineAuth) -> bool {
            matches!(
                a,
                EngineAuth::OAuth2AuthCode {
                    persist_refresh: true,
                    ..
                }
            )
        }
        let fx = fixture(cx);
        open(&fx, cx);
        let run = |cx: &mut TestAppContext| {
            fx.ws.read_with(cx, |w, _| {
                let mut a = code();
                mark_persist(&w.refresh_store, w.collections.first(), &mut a);
                persists(&a)
            })
        };
        // No environment: memory only.
        assert!(!run(cx));
        switch(&fx, "dev", cx);
        assert!(run(cx), "keychain environment persists");
        switch(&fx, "file", cx);
        assert!(!run(cx), "file environment does not persist");
        // The adapter was told `file` is file-mode, so a save for it is refused.
        let saved = fx
            .ws
            .read_with(cx, |w, _| w.refresh_store.save(&token_key("file"), "rt"));
        assert!(saved.is_err());
        // No collection: memory only.
        fx.ws.read_with(cx, |w, _| {
            let mut a = code();
            mark_persist(&w.refresh_store, None, &mut a);
            assert!(!persists(&a));
        });
    }

    #[gpui::test]
    fn move_to_file_forgets_stored_refresh_tokens(cx: &mut TestAppContext) {
        let fx = fixture(cx);
        open(&fx, cx);
        switch(&fx, "dev", cx);
        fx.ws.read_with(cx, |w, _| {
            w.refresh_store.save(&token_key("dev"), "rt-1").unwrap();
        });
        let has_refresh = |fx: &Fx| fx.store.accounts().iter().any(|a| a.contains("/oauth2/"));
        assert!(has_refresh(&fx));
        let root = fx.root.clone();
        fx.ws.update(cx, |w, cx| {
            w.run_env_action(
                root,
                "dev".into(),
                EnvAction::MoveToFile { allow_drop: true },
                cx,
            )
        });
        cx.run_until_parked();
        assert!(!has_refresh(&fx), "{:?}", fx.store.accounts());
        assert!(fx.root.join("environments/dev.secrets.toml").exists());
    }
}
