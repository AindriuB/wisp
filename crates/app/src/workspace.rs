//! The workspace entity: engine, collection, tabs and settings.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gpui::{AppContext as _, Context, Entity, EventEmitter, SharedString, Task, WeakEntity};
use wisp_core::auth::EngineAuth;
use wisp_core::body::{BodyHandle, BodyStore};
use wisp_core::cookies::{CookieJar, CookieJars};
use wisp_core::request::{Auth, Body, CollectionDefaults, Environment, PartValue, Request};
use wisp_core::resolve::{Location, Problem, ResolveError, Scope, resolve};
use wisp_core::{
    Command, Engine, EngineConfig, EngineError, Event, EventReceiver, PreparedRequest, RequestId,
};
use wisp_storage::collection::{CollectionId, ensure_collection_id, read_collection_id};
use wisp_storage::collection::{CollectionTree, Node, open_collection};
use wisp_storage::cookies::{CookieFile, StoredCookie};
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::{list_environments, load_environment};
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
    /// The open collection's history gained or lost entries.
    HistoryChanged,
    /// The open collection's cookie persistence state changed.
    CookiesChanged,
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
    /// Writing is off; the reason is shown in the history panel.
    Off(String),
}

/// Serialises every collection-id resolve so concurrent first uses agree on one id.
static RESOLVE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read or create the collection id under the shared lock. Blocking; never on the UI thread.
fn resolve_collection_id(root: &Path) -> Result<CollectionId, String> {
    let _g = RESOLVE.lock().unwrap_or_else(|e| e.into_inner());
    ensure_collection_id(root).map_err(|e| e.to_string())
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
    /// A load, save or enable error; saves are skipped while set.
    error: Option<String>,
    debounce: Debounce,
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
    pub collection: Option<OpenCollection>,
    pub tabs: Vec<Entity<RequestTab>>,
    pub active: Option<usize>,
    routes: HashMap<RequestId, WeakEntity<RequestTab>>,
    /// Interactive authorizations in flight, by engine id.
    auth_routes: HashMap<RequestId, WeakEntity<RequestTab>>,
    sent: HashMap<RequestId, Sent>,
    data_dir: Option<DataDir>,
    history: HistoryState,
    /// True once a history Notice was shown for the open collection.
    history_notified: bool,
    jars: CookieJars,
    /// The open collection's id, read and resolved off the UI thread.
    coll_id: IdState,
    /// Persistence state per collection id, once its file was checked in this run.
    cookie_files: HashMap<String, CookieStatus>,
    cookie_queue: VecDeque<CookieJob>,
    /// True while a cookie-file job runs: at most one at a time.
    cookie_busy: bool,
    cookie_ticker: Option<Task<()>>,
    _quit_sub: Option<gpui::Subscription>,
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
        let jars = CookieJars::default();
        let (engine, events) = Engine::start(EngineConfig {
            body_memory_limit: settings.body_memory_limit,
            cookies: jars.clone(),
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
            auth_routes: HashMap::new(),
            sent: HashMap::new(),
            data_dir: DataDir::resolve(),
            history: HistoryState::Unresolved,
            history_notified: false,
            jars,
            coll_id: IdState::Absent,
            cookie_files: HashMap::new(),
            cookie_queue: VecDeque::new(),
            cookie_busy: false,
            cookie_ticker: None,
            _quit_sub: None,
            autosend: None,
            watcher: None,
            _watch_task: None,
            empty_defaults: CollectionDefaults::default(),
            prompt_up: false,
        };
        this.spawn_drain(events, cx);
        // Flush on quit: queue a final save, then wait (bounded by gpui's shutdown timeout)
        // for the cookie-file queue to drain.
        this._quit_sub = Some(cx.on_app_quit(|this, cx| {
            this.flush_cookies(cx);
            cx.spawn(async move |this, cx| {
                for _ in 0..150 {
                    if this.update(cx, |t, _| t.cookie_idle()).unwrap_or(true) {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(20))
                        .await;
                }
            })
        }));
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
        // Flush the old collection's jar before the new one is loaded; no jar is cleared.
        self.flush_cookies(cx);
        self.history = HistoryState::Unresolved;
        self.history_notified = false;
        self.coll_id = IdState::Reading;
        self.start_id_read(tree.root.clone(), cx);
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
        // An open history panel reloads for the new collection.
        cx.emit(WorkspaceEvent::HistoryChanged);
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
        // A tab closed while waiting for the browser stops the flow and frees the loopback port.
        let waiting = tab.read(cx).authorize.waiting_id();
        if let Some(cmd) = close_auth(&mut self.auth_routes, &closed, waiting) {
            self.engine.submit(cmd);
        }
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
        let plan = prepare_send(
            &draft,
            path.as_deref(),
            origin.as_ref(),
            self.collection.as_ref().map(|c| &c.tree),
            &self.empty_defaults,
            self.collection.as_ref().and_then(|c| c.active_env.as_ref()),
        )
        .inspect_err(|e| {
            // A refused body is explained here; callers skip their own notice.
            if let Some(message) = refused_body_message(e) {
                cx.emit(WorkspaceEvent::Notice(message.to_string().into()));
            }
        })?;
        if let Some(old) = previous {
            self.engine.submit(Command::Cancel(old));
        }
        if let Some(partition) = self.partition_now() {
            return Ok(Some(self.submit_plan(tab, draft, plan, partition, cx)));
        }
        // No id yet: create it off the UI thread, then submit.
        self.show_sending(tab, None, cx);
        let tab = tab.downgrade();
        self.resolve_id_then(cx, move |this, partition, cx| {
            let (Some(partition), Some(tab)) = (partition, tab.upgrade()) else {
                return;
            };
            this.submit_plan(&tab, draft, plan, partition, cx);
        });
        Ok(None)
    }

    fn submit_plan(
        &mut self,
        tab: &Entity<RequestTab>,
        draft: Request,
        mut plan: SendPlan,
        partition: Option<String>,
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
        let partition = self.partition_now();
        let command = prepare_authorize(
            &draft,
            path.as_deref(),
            origin.as_ref(),
            self.collection.as_ref().map(|c| &c.tree),
            &self.empty_defaults,
            self.collection.as_ref().and_then(|c| c.active_env.as_ref()),
            partition.clone().flatten(),
        )?;
        let Some(command) = command else {
            return Ok(());
        };
        if partition.is_none() {
            // No id yet: create it off the UI thread, then start the flow.
            let tab = tab.downgrade();
            self.resolve_id_then(cx, move |this, partition, cx| {
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
        let Some(c) = &self.collection else { return };
        let task_root = c.tree.root.clone();
        let job = match self.history_job() {
            Ok(j) => j,
            Err(reason) => {
                if matches!(self.history, HistoryState::Unresolved) {
                    self.history_off(reason, cx);
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
                // The collection changed while this was in flight: its result is stale.
                if this.collection.as_ref().map(|c| &c.tree.root) != Some(&task_root) {
                    return;
                }
                match result {
                    Ok(store) => {
                        if matches!(this.history, HistoryState::Unresolved) {
                            this.history = HistoryState::Ready(store);
                        }
                        cx.emit(WorkspaceEvent::HistoryChanged);
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
        self.history = HistoryState::Off(reason.clone());
        self.history_notice(format!("History is off: {reason}"), cx);
    }

    /// A handle to the open collection's history, or the reason there is none.
    pub fn history_job(&self) -> Result<HistoryJob, String> {
        let Some(c) = &self.collection else {
            return Err("Open a collection to see its history".into());
        };
        let store = match &self.history {
            HistoryState::Off(reason) => return Err(reason.clone()),
            HistoryState::Ready(s) => Some(s.clone()),
            HistoryState::Unresolved => None,
        };
        let Some(data) = self.data_dir.clone() else {
            return Err("the data directory is unknown".into());
        };
        Ok(HistoryJob {
            store,
            root: c.tree.root.clone(),
            data,
        })
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
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
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
                let still_open = this.collection.as_ref().map(|c| &c.tree.root) == Some(&root);
                match result {
                    Ok((_, handle)) if !still_open => {
                        if let Some(h) = handle {
                            this.release_body(&h);
                        }
                    }
                    Ok((mut stored, handle)) => {
                        let origin = sanitized_origin(&entry, &mut stored.request);
                        let response =
                            history_ui::stored_response(&entry, stored.response_headers, handle);
                        let tab =
                            cx.new(|_| RequestTab::from_history(stored.request, response, origin));
                        this.tabs.push(tab);
                        this.active = Some(this.tabs.len() - 1);
                        cx.emit(WorkspaceEvent::TabsChanged);
                        cx.emit(WorkspaceEvent::ActiveTabChanged);
                        cx.notify();
                    }
                    Err(e) if still_open => cx.emit(WorkspaceEvent::Notice(
                        format!("Cannot open history entry: {e}").into(),
                    )),
                    Err(_) => {}
                }
            })
            .ok();
        })
        .detach();
    }

    /// Remove the open collection's history; the store I/O runs on the background executor.
    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        let job = match self.history_job() {
            Ok(j) => j,
            Err(reason) => {
                cx.emit(WorkspaceEvent::Notice(reason.into()));
                return;
            }
        };
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let work = cx.background_executor().spawn(async move { job.clear() });
            let result = work.await;
            this.update(cx, |this, cx| {
                if this.collection.as_ref().map(|c| &c.tree.root) != Some(&root) {
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
    /// has to be read or created.
    fn partition_now(&self) -> Option<Option<String>> {
        if self.collection.is_none() {
            return Some(None);
        }
        match &self.coll_id {
            IdState::Known(id) => Some(Some(id.clone())),
            IdState::Failed { partition, .. } => Some(Some(partition.clone())),
            IdState::Reading | IdState::Absent => None,
        }
    }

    fn known_id(&self) -> Option<String> {
        match (&self.collection, &self.coll_id) {
            (Some(_), IdState::Known(id)) => Some(id.clone()),
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
        if self.collection.is_none() {
            return Some("open a collection first".into());
        }
        if self.data_dir.is_none() {
            return Some("the data directory is unknown".into());
        }
        match &self.coll_id {
            IdState::Failed { reason, .. } => Some(reason.clone()),
            _ => None,
        }
    }

    /// What the Cookies manager shows.
    pub fn cookie_panel(&self) -> CookiePanel {
        let name = self.collection.as_ref().map(|c| {
            c.tree.root.file_name().map_or_else(
                || c.tree.root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        });
        let persistence = if let Some(reason) = self.cookie_unavailable() {
            Persistence::Unavailable(reason)
        } else if matches!(self.coll_id, IdState::Absent) {
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
        if self.collection.as_ref().map(|c| c.tree.root.as_path()) != Some(root) {
            return;
        }
        if matches!(self.coll_id, IdState::Known(_))
            || (from_read && !matches!(self.coll_id, IdState::Reading))
        {
            return;
        }
        self.coll_id = state.clone();
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
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Option<Option<String>>, &mut Context<Self>) + 'static,
    ) {
        if let Some(p) = self.partition_now() {
            then(self, Some(p), cx);
            return;
        }
        let Some(root) = self.collection.as_ref().map(|c| c.tree.root.clone()) else {
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
                let same = this.collection.as_ref().map(|c| c.tree.root.as_path()) == Some(&root);
                let p = if same { this.partition_now() } else { None };
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

    /// True when no cookie-file job is queued or running.
    fn cookie_idle(&self) -> bool {
        !self.cookie_busy && self.cookie_queue.is_empty()
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
        let mut failure: Option<String> = None;
        let jar = self.jars.jar(Some(id));
        if let Some(st) = self.cookie_files.get_mut(id) {
            match done {
                CookieDone::Loaded(Ok(None)) => {
                    st.loaded = true;
                    st.enabled = false;
                }
                CookieDone::Loaded(Ok(Some(stored))) => {
                    st.loaded = true;
                    st.enabled = true;
                    // Cookies set while the load ran are newer and win.
                    let mut records = cookie_ui::to_records(stored);
                    records.extend(jar.snapshot(SystemTime::now()));
                    jar.load(records);
                }
                CookieDone::Loaded(Err(e)) => {
                    // The file is left alone until the toggle is turned off and on.
                    st.loaded = true;
                    st.enabled = true;
                    st.error = Some(e.clone());
                    failure = Some(e);
                }
                CookieDone::Saved(Err(e)) => {
                    st.error = Some(e.clone());
                    failure = Some(e);
                }
                CookieDone::Enabled(Err(e)) => {
                    st.enabled = false;
                    failure = Some(e);
                }
                CookieDone::Disabled(Err(e)) => {
                    st.enabled = true;
                    st.error = Some(e.clone());
                    failure = Some(e);
                }
                CookieDone::Saved(Ok(()))
                | CookieDone::Enabled(Ok(()))
                | CookieDone::Disabled(Ok(())) => {}
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

    /// True when the open collection's file is on and healthy, so changes are saved.
    fn cookie_saving(&self) -> Option<String> {
        let id = self.known_id()?;
        let st = self.cookie_files.get(&id)?;
        (st.loaded && st.enabled && st.error.is_none()).then_some(id)
    }

    /// Save now if the open collection's jar changed since the last save.
    fn flush_cookies(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.cookie_saving() else {
            return;
        };
        let generation = self.jars.jar(Some(&id)).generation();
        let dirty = self.cookie_files.get_mut(&id).is_some_and(|st| {
            st.debounce.observe(generation, Instant::now());
            st.debounce.dirty()
        });
        if dirty {
            self.queue_cookie_save(&id, cx);
        }
    }

    /// Watch the open collection's jar while persistence is on. The timer stops itself when
    /// there is nothing to watch.
    fn ensure_cookie_ticker(&mut self, cx: &mut Context<Self>) {
        if self.cookie_ticker.is_some() || self.cookie_saving().is_none() {
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
        let Some(id) = self.cookie_saving() else {
            return false;
        };
        let generation = self.jars.jar(Some(&id)).generation();
        let due = self.cookie_files.get_mut(&id).is_some_and(|st| {
            st.debounce.observe(generation, now);
            st.debounce.due(now)
        });
        if due {
            self.queue_cookie_save(&id, cx);
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
        self.resolve_id_then(cx, move |this, partition, cx| {
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
                error: None,
                debounce: Debounce::new(generation),
            });
            st.error = None;
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
