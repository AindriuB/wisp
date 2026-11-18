//! One saved request, sent once: the whole path of `wisp-cli run`. It never retries, never
//! prompts and keeps every secret value out of the report it returns.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use wisp_core::assert::Verdict;
use wisp_core::auth::EngineAuth;
use wisp_core::request::{CollectionDefaults, Environment, Request, Var};
use wisp_core::resolve::{DynamicValue, HideReason, Problem, Recording, Scope, ShownPart, explain};
use wisp_core::{Engine, EngineConfig, EngineError, ErrorKind, Event, Header, Method, RequestId};
use wisp_storage::collection::{CollectionTree, open_collection, read_collection_id};
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::{
    SecretsMode, environment_info, list_environments, load_environment,
};
use wisp_storage::history::{BODY_CAP, Delivery};
use wisp_storage::request::load_request;
use wisp_storage::vault::{LoadStatus, SecretStores, VaultCtx, load_environment_with};

use crate::history::{HistoryBodies, record};
use crate::outcome::{CheckOutcome, Response, ResponseTimings, Terminal, finish};
use crate::plan::{SendInput, prepare};
use crate::send::submit;
use crate::vault::{KeychainStores, portable_marker};

/// Stands in for every secret value before the report is built.
const PLACEHOLDER: &str = "<secret>";

/// Everything `run_once` reads.
pub struct RunInput {
    pub collection: PathBuf,
    /// Relative to the collection root.
    pub request: String,
    pub env: Option<String>,
    /// Load keychain-backed environment secrets.
    pub keychain: bool,
    /// Append a history entry when the collection already has an id.
    pub history: bool,
    /// Where history and keychain bindings live; `None` resolves the per-user directory.
    pub data: Option<DataDir>,
    /// Test seam: replaces the OS keychain when `keychain` is set. The CLI never sets it.
    pub stores: Option<Box<dyn SecretStores>>,
}

/// Why nothing was run: the collection, request or environment cannot be read (exit 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    Unreadable(String),
}

/// Why a request was not prepared (exit 4).
#[derive(Debug, Clone, PartialEq)]
pub enum RunProblem {
    Resolve(Problem),
    /// The variable is a keychain secret and `--keychain` was not given.
    KeychainSecret {
        name: String,
    },
    /// `--keychain` was given but the keychain holds no value for the declared secret.
    MissingSecret {
        name: String,
    },
    /// The keychain could not supply the environment's secrets.
    KeychainUnavailable,
    /// The environment names a secret store this build does not know.
    UnknownStore,
    /// The request's auth needs a person to authorize it.
    InteractiveAuth,
}

/// How the send ended, without any engine message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    /// Nothing was sent: preparation failed.
    NotPrepared,
    Completed,
    Failed(ErrorKind),
    Cancelled,
}

/// What became of the history entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStatus {
    Off,
    Recorded,
    /// `--history` was given but the run never sent (it was not prepared).
    SkippedNotSent,
    /// `--history` was given but the collection has no id.
    SkippedNoId,
    /// The id, the data directory or the append failed.
    Failed,
}

/// The facts of one run, for the CLI to print and to map to an exit code.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub request: String,
    pub environment: Option<String>,
    pub method: Method,
    /// The display URL: no query, secret parts as `{secret NAME}`, no userinfo.
    pub url: String,
    pub end: RunEnd,
    pub problems: Vec<RunProblem>,
    pub delivery: Delivery,
    pub retry_safe: bool,
    pub status: Option<u16>,
    /// The reason phrase of the response, when the server sent one.
    pub reason: Option<String>,
    pub expected_status: Vec<u16>,
    pub status_ok: Option<bool>,
    pub ttfb: Option<Duration>,
    pub total: Option<Duration>,
    pub size: Option<u64>,
    pub checks: Vec<CheckOutcome>,
    pub dynamics: Vec<DynamicValue>,
    pub history: HistoryStatus,
}

fn unreadable(e: impl std::fmt::Display) -> RunError {
    RunError::Unreadable(e.to_string())
}

/// The path must be relative, free of `..`, and name a regular file with no symlink on the way.
fn confined(root: &Path, request: &str) -> Result<PathBuf, RunError> {
    let rel = Path::new(request);
    if request.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(unreadable(
            "request path must be relative to the collection",
        ));
    }
    let mut cur = root.to_path_buf();
    let n = rel.components().count();
    for (i, c) in rel.components().enumerate() {
        cur.push(c);
        let m = std::fs::symlink_metadata(&cur).map_err(|_| unreadable("request not found"))?;
        let ok = !m.file_type().is_symlink() && if i + 1 == n { m.is_file() } else { m.is_dir() };
        if !ok {
            return Err(unreadable(
                "request path must be a regular file, no symlinks",
            ));
        }
    }
    Ok(cur)
}

/// Whether `method` is idempotent in the sense of RFC 9110 for a retry decision.
fn idempotent(method: Method) -> bool {
    matches!(
        method,
        Method::Get | Method::Head | Method::Options | Method::Put | Method::Delete
    )
}

/// `retry_safe` for the method the user sent: a request that was not sent is always safe.
pub fn retry_safe(delivery: Delivery, method: Method) -> bool {
    delivery == Delivery::NotSent || idempotent(method)
}

/// Render a display URL: the query is cut, a secret is `{secret NAME}`, userinfo is dropped.
fn display_url(parts: &[ShownPart]) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            ShownPart::Text(t) => match t.find('?') {
                Some(i) => {
                    out.push_str(&t[..i]);
                    break;
                }
                None => out.push_str(t),
            },
            ShownPart::Secret { name, .. } => {
                out.push_str("{secret ");
                out.push_str(name);
                out.push('}');
            }
            ShownPart::Dynamic { name } => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
            ShownPart::Hidden(HideReason::Userinfo) => {}
            ShownPart::Hidden(HideReason::Credential) => out.push_str("{hidden}"),
            ShownPart::Hidden(HideReason::TooLong) => out.push_str("{too long}"),
        }
    }
    // Userinfo is hidden as a part; the `@` that followed it goes with it.
    if let Some(rest) = out.split_once("://").map(|(_, r)| r)
        && rest.starts_with('@')
    {
        out = out.replacen("://@", "://", 1);
    }
    out
}

/// The environment for the send, plus what the report needs to describe it.
struct Loaded {
    env: Environment,
    /// Secret names that live in the keychain and were not loaded.
    unloaded: Vec<String>,
    /// Secret names declared in the keychain with no stored value (`--keychain`).
    missing: Vec<String>,
}

enum EnvFailure {
    Unreadable(RunError),
    Problem(RunProblem),
}

fn load_env(
    input: &RunInput,
    root: &Path,
    name: &str,
    data: Option<&DataDir>,
) -> Result<Loaded, EnvFailure> {
    let names = list_environments(root).map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
    if !names.iter().any(|e| e == name) {
        return Err(EnvFailure::Unreadable(unreadable("unknown environment")));
    }
    let info = environment_info(root, name).map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
    match info.store {
        SecretsMode::File => {
            let env =
                load_environment(root, name).map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
            Ok(Loaded {
                env,
                unloaded: Vec::new(),
                missing: Vec::new(),
            })
        }
        SecretsMode::Unknown(_) => Err(EnvFailure::Problem(RunProblem::UnknownStore)),
        SecretsMode::Store(_) if !input.keychain => {
            let mut env =
                load_environment(root, name).map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
            // Declared but not loaded: empty secrets shadow lower scopes and stay unresolved.
            for n in &info.secret_names {
                env.secrets.push(Var {
                    name: n.clone(),
                    value: String::new(),
                });
            }
            Ok(Loaded {
                env,
                unloaded: info.secret_names,
                missing: Vec::new(),
            })
        }
        SecretsMode::Store(_) => {
            let id = read_collection_id(root).map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
            let Some(data) = data else {
                return Err(EnvFailure::Problem(RunProblem::KeychainUnavailable));
            };
            let default_stores;
            let stores: &dyn SecretStores = match &input.stores {
                Some(s) => s.as_ref(),
                None => {
                    default_stores = KeychainStores::open(portable_marker());
                    &default_stores
                }
            };
            let loaded = load_environment_with(
                root,
                name,
                id.as_ref(),
                stores,
                &VaultCtx::new(data.clone()),
            )
            .map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
            if matches!(loaded.status, LoadStatus::Failed { .. }) {
                return Err(EnvFailure::Problem(RunProblem::KeychainUnavailable));
            }
            let mut env = loaded.env;
            // A declared secret with no value shadows lower scopes and stays unresolved.
            for n in &loaded.unresolved {
                env.secrets.push(Var {
                    name: n.clone(),
                    value: String::new(),
                });
            }
            Ok(Loaded {
                env,
                unloaded: Vec::new(),
                missing: loaded.unresolved,
            })
        }
    }
}

/// A copy of `env` with every secret value replaced, safe to hand to the report builder.
fn masked(env: &Environment) -> Environment {
    let mut e = env.clone();
    for s in &mut e.secrets {
        if !s.value.is_empty() {
            s.value = PLACEHOLDER.to_owned();
        }
    }
    e
}

fn blank(report: &mut RunReport, problems: Vec<RunProblem>) {
    report.end = RunEnd::NotPrepared;
    report.problems = problems;
    report.delivery = Delivery::NotSent;
    report.retry_safe = true;
}

/// Block until the terminal event of `id`.
struct Ended {
    terminal: Event,
    headers: Vec<Header>,
    status: Option<u16>,
    reason: Option<String>,
    ttfb: Option<Duration>,
    headers_seen: bool,
}

fn await_end(events: &wisp_core::EventReceiver, id: RequestId) -> Ended {
    let mut ended = Ended {
        terminal: Event::Cancelled { id },
        headers: Vec::new(),
        status: None,
        reason: None,
        ttfb: None,
        headers_seen: false,
    };
    loop {
        let Ok(ev) = events.recv_blocking() else {
            // The engine went away: nothing proves the request did not leave.
            ended.terminal = Event::Failed {
                id,
                error: EngineError {
                    kind: ErrorKind::Other,
                    message: String::new(),
                },
                nothing_sent: false,
            };
            return ended;
        };
        match ev {
            Event::Headers {
                id: i,
                status,
                reason,
                headers,
                ttfb,
                ..
            } if i == id => {
                ended.reason = reason;
                ended.headers = headers;
                ended.status = Some(status);
                ended.ttfb = Some(ttfb);
                ended.headers_seen = true;
            }
            Event::Completed { id: i, .. }
            | Event::Failed { id: i, .. }
            | Event::Cancelled { id: i }
                if i == id =>
            {
                ended.terminal = ev;
                return ended;
            }
            _ => {}
        }
    }
}

/// Send one saved request once and describe what happened.
pub fn run_once(input: RunInput) -> Result<RunReport, RunError> {
    // Read-only opening: nothing is created, no id is assigned.
    let tree: CollectionTree = open_collection(&input.collection).map_err(unreadable)?;
    let abs = confined(&tree.root, &input.request)?;
    let draft: Request = load_request(&abs).map_err(unreadable)?;
    let rel = abs.strip_prefix(&tree.root).unwrap_or(&abs).to_path_buf();
    let data = input.data.clone().or_else(DataDir::resolve);

    let mut report = RunReport {
        request: input.request.clone(),
        environment: input.env.clone(),
        method: draft.method,
        url: String::new(),
        end: RunEnd::NotPrepared,
        problems: Vec::new(),
        delivery: Delivery::NotSent,
        retry_safe: true,
        status: None,
        reason: None,
        expected_status: draft.expected_status.clone(),
        status_ok: None,
        ttfb: None,
        total: None,
        size: None,
        checks: Vec::new(),
        dynamics: Vec::new(),
        // Replaced once the run has sent; a run that never sent keeps this.
        history: if !input.history {
            HistoryStatus::Off
        } else if matches!(read_collection_id(&tree.root), Ok(None)) {
            HistoryStatus::SkippedNoId
        } else {
            HistoryStatus::SkippedNotSent
        },
    };

    let loaded = match &input.env {
        Some(name) => match load_env(&input, &tree.root, name, data.as_ref()) {
            Ok(l) => Some(l),
            Err(EnvFailure::Unreadable(e)) => return Err(e),
            Err(EnvFailure::Problem(p)) => {
                blank(&mut report, vec![p]);
                report.url = display_for(&tree, &rel, &draft, None, &[]).0;
                return Ok(report);
            }
        },
        None => None,
    };
    let env = loaded.as_ref().map(|l| &l.env);
    let missing: &[String] = loaded.as_ref().map_or(&[], |l| &l.missing);
    let unloaded: Vec<String> = loaded
        .as_ref()
        .map_or(&[][..], |l| &l.unloaded[..])
        .iter()
        .chain(missing)
        .cloned()
        .collect();
    let secret_problem = |name: String| {
        if missing.contains(&name) {
            RunProblem::MissingSecret { name }
        } else {
            RunProblem::KeychainSecret { name }
        }
    };
    let (url, keychain_used) = display_for(&tree, &rel, &draft, env, &unloaded);
    report.url = url;
    if !keychain_used.is_empty() {
        let problems = keychain_used.into_iter().map(secret_problem).collect();
        blank(&mut report, problems);
        return Ok(report);
    }

    let defaults = CollectionDefaults::default();
    let mut rec = Recording::new();
    let plan = prepare(
        &SendInput {
            draft: &draft,
            tab_path: Some(&rel),
            origin: None,
            tree: Some(&tree),
            empty_defaults: &defaults,
            env,
        },
        &mut rec,
    );
    let mut plan = match plan {
        Ok(p) => p,
        Err(e) => {
            let problems = e
                .problems
                .into_iter()
                .map(|p| match &p {
                    Problem::UnresolvedVar { name, .. } if unloaded.contains(name) => {
                        secret_problem(name.clone())
                    }
                    _ => RunProblem::Resolve(p),
                })
                .collect();
            blank(&mut report, problems);
            return Ok(report);
        }
    };
    if matches!(plan.prepared.auth, Some(EngineAuth::OAuth2AuthCode { .. })) {
        blank(&mut report, vec![RunProblem::InteractiveAuth]);
        return Ok(report);
    }
    plan.dynamics = rec.into_values();
    report.dynamics = plan.dynamics.clone();

    let id_read = read_collection_id(&tree.root);
    let id = id_read.as_ref().ok().cloned().flatten();
    // A fresh engine, one worker, refresh tokens in memory only, a fresh cookie jar and the
    // default timeout (all of `EngineConfig::default`).
    let (engine, events) = Engine::start(EngineConfig {
        worker_threads: 1,
        ..EngineConfig::default()
    });
    let sent = submit(
        &engine,
        plan,
        draft,
        id.as_ref().map(ToString::to_string),
        Some(tree.root.clone()),
    );
    let ended = await_end(&events, sent.id);

    let bodies = engine.bodies();
    let (terminal, timings, size, handle) = match &ended.terminal {
        Event::Completed {
            total,
            size,
            body,
            timings,
            ..
        } => (
            ended.terminal.clone(),
            ResponseTimings {
                ttfb: ended.ttfb,
                total: Some(*total),
                dns: timings.dns,
                connect: timings.connect,
            },
            Some(*size),
            Some(*body),
        ),
        _ => (
            ended.terminal.clone(),
            ResponseTimings {
                ttfb: ended.ttfb,
                ..ResponseTimings::default()
            },
            None,
            None,
        ),
    };
    // At most the check cap plus one byte, then the handle (and any spill file) goes.
    // A read error gives `None`, as the GUI does, so checks skip rather than fail.
    let prefix = handle.and_then(|h| {
        let bytes = bodies.read(&h, 0, BODY_CAP + 1).ok();
        bodies.release(&h);
        bytes
    });
    let end_terminal;
    let err_holder;
    let t = match &terminal {
        Event::Completed { .. } => {
            end_terminal = Terminal::Completed {
                body_prefix: prefix.as_deref(),
            };
            &end_terminal
        }
        Event::Failed {
            error,
            nothing_sent,
            ..
        } => {
            err_holder = Terminal::Failed {
                error,
                nothing_sent: *nothing_sent,
            };
            &err_holder
        }
        _ => &Terminal::Cancelled,
    };
    let response = Response {
        status: ended.status,
        headers: &ended.headers,
        timings,
        size,
        headers_seen: ended.headers_seen,
    };
    let mut finished = finish(sent.record, &response, t, &report.expected_status);
    drop(engine);
    // Response headers can echo anything the server likes: the CLI keeps none in history.
    finished.entry.response_headers.clear();

    report.end = match t {
        Terminal::Completed { .. } => RunEnd::Completed,
        Terminal::Failed { error, .. } => RunEnd::Failed(error.kind),
        Terminal::Cancelled => RunEnd::Cancelled,
    };
    report.delivery = finished.delivery;
    report.retry_safe = retry_safe(finished.delivery, report.method);
    report.status = ended.status;
    report.reason = ended.reason.clone();
    report.status_ok = finished.status_ok;
    report.ttfb = timings.ttfb;
    report.total = timings.total;
    report.size = size;
    report.checks = finished.checks;

    if input.history {
        report.history = match (id_read, data) {
            (Ok(None), _) => HistoryStatus::SkippedNoId,
            (Ok(Some(id)), Some(data)) => {
                match record(&data, &id, finished.entry, None, HistoryBodies::Skip) {
                    Ok(_) => HistoryStatus::Recorded,
                    Err(_) => HistoryStatus::Failed,
                }
            }
            _ => HistoryStatus::Failed,
        };
    }
    Ok(report)
}

/// What Explain says about the request, with secret values masked first. Returns the display
/// URL and the names of unloaded keychain secrets the request uses.
fn display_for(
    tree: &CollectionTree,
    rel: &Path,
    draft: &Request,
    env: Option<&Environment>,
    unloaded: &[String],
) -> (String, Vec<String>) {
    let masked_env = env.map(masked);
    let scope: Scope = tree.scope_for(rel, masked_env.as_ref());
    let report = explain(draft, &scope);
    let used = report
        .variables
        .iter()
        .filter(|v| {
            unloaded.contains(&v.name)
                && v.value.0.iter().any(|p| {
                    matches!(
                        p,
                        ShownPart::Secret {
                            resolved: false,
                            ..
                        }
                    )
                })
        })
        .map(|v| v.name.clone())
        .collect();
    (display_url(&report.url.0), used)
}

/// A skip reason or `None`; used by callers that print assertions.
pub fn skip_reason(c: &CheckOutcome) -> Option<&str> {
    match &c.result.verdict {
        Verdict::Skipped { reason } => Some(reason),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    use wisp_storage::collection::CollectionId;
    use wisp_storage::vault::{MemoryStore, SecretStore, SecretValue, bind, secret_account};

    use super::*;

    /// Lets one `MemoryStore` be shared between the test and the run.
    struct Shared(Arc<MemoryStore>);
    impl SecretStores for Shared {
        fn get(&self, kind: wisp_storage::vault::StoreKind) -> Option<&dyn SecretStore> {
            SecretStores::get(&*self.0, kind)
        }
    }

    fn server() -> std::net::SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let _ = s.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nX-Echo: CANARY_KC\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{head}",
                        head.len()
                    )
                    .as_bytes(),
                );
            }
        });
        addr
    }

    struct Fx {
        coll: tempfile::TempDir,
        data: tempfile::TempDir,
    }

    const ID: &str = "abcdefgh12345678";

    fn fx(addr: std::net::SocketAddr, id: bool) -> Fx {
        let f = Fx {
            coll: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        };
        let idline = if id {
            format!("id = \"{ID}\"\n")
        } else {
            String::new()
        };
        let w = |rel: &str, t: &str| {
            let p = f.coll.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w("collection.toml", &format!("format = 1\n{idline}"));
        w(
            "environments/prod.toml",
            "secrets = \"keychain\"\nsecret_names = [\"kc\"]\n",
        );
        w(
            "a.toml",
            &format!(
                "name = \"a\"\nmethod = \"POST\"\nurl = \"http://{addr}/{{{{kc}}}}\"\n\n[[headers]]\nname = \"X-K\"\nvalue = \"{{{{kc}}}}\"\nenabled = true\n"
            ),
        );
        f
    }

    fn input(
        f: &Fx,
        keychain: bool,
        history: bool,
        stores: Option<Box<dyn SecretStores>>,
    ) -> RunInput {
        RunInput {
            collection: f.coll.path().to_path_buf(),
            request: "a.toml".into(),
            env: Some("prod".into()),
            keychain,
            history,
            data: Some(DataDir::at(f.data.path())),
            stores,
        }
    }

    fn bound_store(f: &Fx) -> Arc<MemoryStore> {
        let id = CollectionId::parse(ID).unwrap();
        let root = std::fs::canonicalize(f.coll.path()).unwrap();
        // A folder without an id cannot be bound (the binding check refuses it).
        let carries_id = std::fs::read_to_string(root.join("collection.toml"))
            .is_ok_and(|t| t.contains(ID));
        if carries_id {
            bind(&DataDir::at(f.data.path()), &id, &root).unwrap();
        }
        let store = Arc::new(MemoryStore::new());
        store
            .set(
                &secret_account(&id, "prod", "kc"),
                &SecretValue::new("CANARY_KC"),
            )
            .unwrap();
        store
    }

    fn history_text(f: &Fx) -> String {
        let mut out = String::new();
        let mut stack = vec![f.data.path().join("history")];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(t) = std::fs::read_to_string(&p) {
                    out.push_str(&t);
                }
            }
        }
        out
    }

    #[test]
    fn run_retry_safe_follows_delivery_and_the_method_sent() {
        use Delivery::*;
        for m in [Method::Post, Method::Patch] {
            assert!(retry_safe(NotSent, m));
            assert!(!retry_safe(Unknown, m));
            assert!(!retry_safe(Responded, m));
        }
        for m in [
            Method::Get,
            Method::Head,
            Method::Options,
            Method::Put,
            Method::Delete,
        ] {
            for d in [NotSent, Unknown, Responded] {
                assert!(retry_safe(d, m));
            }
        }
    }

    #[test]
    fn run_display_url_cuts_query_marks_secrets_and_drops_userinfo() {
        let parts = [
            ShownPart::Text("http://".into()),
            ShownPart::Hidden(HideReason::Userinfo),
            ShownPart::Text("@h/".into()),
            ShownPart::Secret {
                name: "k".into(),
                resolved: true,
            },
            ShownPart::Text("/p?x=".into()),
            ShownPart::Secret {
                name: "z".into(),
                resolved: true,
            },
        ];
        assert_eq!(display_url(&parts), "http://h/{secret k}/p");
    }

    #[test]
    fn run_keychain_secret_loads_through_the_store_and_never_leaks() {
        let f = fx(server(), true);
        let store = bound_store(&f);
        let report = run_once(input(&f, true, true, Some(Box::new(Shared(store))))).unwrap();
        assert_eq!(report.end, RunEnd::Completed);
        assert_eq!(report.status, Some(200));
        assert_eq!(report.history, HistoryStatus::Recorded);
        assert!(!format!("{report:?}").contains("CANARY_KC"));
        assert!(report.url.contains("{secret kc}"), "{}", report.url);
        let h = history_text(&f);
        assert!(!h.is_empty() && !h.contains("CANARY_KC"), "{h}");
    }

    #[test]
    fn run_keychain_secret_without_the_flag_is_not_prepared() {
        let f = fx(server(), true);
        let store = bound_store(&f);
        let report = run_once(input(&f, false, false, Some(Box::new(Shared(store))))).unwrap();
        assert_eq!(report.end, RunEnd::NotPrepared);
        assert_eq!(
            report.problems,
            vec![RunProblem::KeychainSecret { name: "kc".into() }]
        );
        assert_eq!(report.delivery, Delivery::NotSent);
        assert!(report.retry_safe);
    }

    #[test]
    fn run_keychain_failure_is_not_prepared_with_a_fixed_problem() {
        let f = fx(server(), true);
        // Not bound on this machine: the load fails closed.
        let store = Arc::new(MemoryStore::new());
        let report = run_once(input(&f, true, false, Some(Box::new(Shared(store))))).unwrap();
        assert_eq!(report.end, RunEnd::NotPrepared);
        assert_eq!(report.problems, vec![RunProblem::KeychainUnavailable]);
        let f2 = fx(server(), true);
        let store = bound_store(&f2);
        store.fail_probe(Some(wisp_storage::vault::SecretStoreError::Unavailable {
            reason: wisp_storage::vault::UnavailableReason::TimedOut,
        }));
        let report = run_once(input(&f2, true, false, Some(Box::new(Shared(store))))).unwrap();
        assert_eq!(report.problems, vec![RunProblem::KeychainUnavailable]);
    }

    #[test]
    fn run_history_is_skipped_when_the_collection_has_no_id() {
        let f = fx(server(), false);
        let mut i = input(&f, false, true, None);
        i.env = None;
        i.request = "a.toml".into();
        std::fs::write(
            f.coll.path().join("a.toml"),
            "name = \"a\"\nmethod = \"GET\"\nurl = \"http://127.0.0.1:1/\"\n",
        )
        .unwrap();
        let report = run_once(i).unwrap();
        assert_eq!(report.history, HistoryStatus::SkippedNoId);
        assert!(!f.data.path().join("history").exists());
        assert!(
            !std::fs::read_to_string(f.coll.path().join("collection.toml"))
                .unwrap()
                .contains("id =")
        );
    }

    fn silent_server() -> std::net::SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for s in l.incoming().flatten() {
                held.push(s);
            }
        });
        addr
    }

    fn write_request(f: &Fx, addr: std::net::SocketAddr, method: &str, extra: &str) {
        std::fs::write(
            f.coll.path().join("a.toml"),
            format!(
                "name = \"a\"\nmethod = \"{method}\"\nurl = \"http://{addr}/{{{{kc}}}}\"\n{extra}\n[[headers]]\nname = \"X-K\"\nvalue = \"{{{{kc}}}}\"\nenabled = true\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7() {
        let closed = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let slow = "[settings]\ntimeout_ms = 300\n";
        let cases: [(&str, std::net::SocketAddr, &str, &str, RunEnd); 6] = [
            ("exit 0", server(), "GET", "", RunEnd::Completed),
            (
                "exit 1",
                server(),
                "GET",
                "[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 201\n",
                RunEnd::Completed,
            ),
            (
                "exit 4",
                server(),
                "GET",
                "[[headers]]\nname = \"X-M\"\nvalue = \"{{missing}}\"\nenabled = true\n",
                RunEnd::NotPrepared,
            ),
            (
                "exit 5",
                closed,
                "POST",
                "",
                RunEnd::Failed(ErrorKind::Connect),
            ),
            (
                "exit 6",
                silent_server(),
                "POST",
                slow,
                RunEnd::Failed(ErrorKind::Timeout),
            ),
            (
                "exit 7",
                server(),
                "GET",
                "expected_status = [201]\n",
                RunEnd::Completed,
            ),
        ];
        for (what, addr, method, extra, end) in cases {
            let f = fx(addr, true);
            write_request(&f, addr, method, extra);
            let store = bound_store(&f);
            let report = run_once(input(&f, true, true, Some(Box::new(Shared(store))))).unwrap();
            assert_eq!(report.end, end, "{what}");
            let shown = format!("{report:?}");
            assert!(!shown.contains("CANARY_KC"), "{what}: {shown}");
            let h = history_text(&f);
            assert!(!h.contains("CANARY_KC"), "{what}: {h}");
        }
    }

    #[test]
    fn run_missing_keychain_secret_is_not_prepared_and_sends_nothing() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h2 = hits.clone();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                h2.fetch_add(1, Ordering::SeqCst);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let f = fx(addr, true);
        let w = |rel: &str, t: &str| std::fs::write(f.coll.path().join(rel), t).unwrap();
        w(
            "collection.toml",
            &format!("format = 1\nid = \"{ID}\"\n\n[vars]\nkc = \"dev-default\"\n"),
        );
        // Bound, but nothing stored for `kc`.
        let id = CollectionId::parse(ID).unwrap();
        let root = std::fs::canonicalize(f.coll.path()).unwrap();
        // A folder without an id cannot be bound (the binding check refuses it).
        let carries_id = std::fs::read_to_string(root.join("collection.toml"))
            .is_ok_and(|t| t.contains(ID));
        if carries_id {
            bind(&DataDir::at(f.data.path()), &id, &root).unwrap();
        }
        let store = Arc::new(MemoryStore::new());
        let report = run_once(input(&f, true, true, Some(Box::new(Shared(store))))).unwrap();
        assert_eq!(report.end, RunEnd::NotPrepared);
        assert_eq!(
            report.problems,
            vec![RunProblem::MissingSecret { name: "kc".into() }]
        );
        assert_eq!(report.history, HistoryStatus::SkippedNotSent);
        assert!(!report.url.contains("dev-default"), "{}", report.url);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn run_history_for_a_not_prepared_run_is_skipped_not_sent_or_no_id() {
        for (id, want) in [
            (true, HistoryStatus::SkippedNotSent),
            (false, HistoryStatus::SkippedNoId),
        ] {
            let f = fx(server(), id);
            let store = bound_store(&f);
            let report = run_once(input(&f, false, true, Some(Box::new(Shared(store))))).unwrap();
            assert_eq!(report.end, RunEnd::NotPrepared);
            assert_eq!(report.history, want);
        }
    }

    #[test]
    fn run_unknown_environment_is_unreadable() {
        let f = fx(server(), true);
        let mut i = input(&f, false, false, None);
        i.env = Some("nope".into());
        assert!(matches!(run_once(i), Err(RunError::Unreadable(_))));
    }
}
