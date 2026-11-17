//! One saved request, sent once: the whole path of `wisp-cli run`. It never retries, never
//! prompts and keeps every secret value out of the report it returns.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use wisp_core::assert::Verdict;
use wisp_core::auth::EngineAuth;
use wisp_core::request::{CollectionDefaults, Environment, Request, Var};
use wisp_core::resolve::{
    DynamicValue, HideReason, Problem, Recording, Scope, ShownPart, explain,
};
use wisp_core::{
    Engine, EngineConfig, EngineError, ErrorKind, Event, Header, Method, RequestId,
};
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
    KeychainSecret { name: String },
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
    let info =
        environment_info(root, name).map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
    match info.store {
        SecretsMode::File => {
            let env = load_environment(root, name)
                .map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
            Ok(Loaded {
                env,
                unloaded: Vec::new(),
            })
        }
        SecretsMode::Unknown(_) => Err(EnvFailure::Problem(RunProblem::UnknownStore)),
        SecretsMode::Store(_) if !input.keychain => {
            let mut env = load_environment(root, name)
                .map_err(|e| EnvFailure::Unreadable(unreadable(e)))?;
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
            Ok(Loaded {
                env: loaded.env,
                unloaded: Vec::new(),
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
    ttfb: Option<Duration>,
    headers_seen: bool,
}

fn await_end(events: &wisp_core::EventReceiver, id: RequestId) -> Ended {
    let mut ended = Ended {
        terminal: Event::Cancelled { id },
        headers: Vec::new(),
        status: None,
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
                headers,
                ttfb,
                ..
            } if i == id => {
                ended.headers = headers;
                ended.status = Some(status);
                ended.ttfb = Some(ttfb);
                ended.headers_seen = true;
            }
            Event::Completed { id: i, .. } | Event::Failed { id: i, .. } | Event::Cancelled { id: i }
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
        expected_status: draft.expected_status.clone(),
        status_ok: None,
        ttfb: None,
        total: None,
        size: None,
        checks: Vec::new(),
        dynamics: Vec::new(),
        history: if input.history {
            HistoryStatus::Failed
        } else {
            HistoryStatus::Off
        },
    };

    let loaded = match &input.env {
        Some(name) => match load_env(&input, &tree.root, name, data.as_ref()) {
            Ok(l) => Some(l),
            Err(EnvFailure::Unreadable(e)) => return Err(e),
            Err(EnvFailure::Problem(p)) => {
                blank(&mut report, vec![p]);
                report.url = display_for(&tree, &rel, &draft, None);
                return Ok(report);
            }
        },
        None => None,
    };
    let env = loaded.as_ref().map(|l| &l.env);
    let unloaded: &[String] = loaded.as_ref().map_or(&[], |l| &l.unloaded);
    report.url = display_for(&tree, &rel, &draft, env);

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
                        RunProblem::KeychainSecret { name: name.clone() }
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
    let prefix = handle.map(|h| {
        let bytes = bodies.read(&h, 0, BODY_CAP + 1).unwrap_or_default();
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
    let finished = finish(sent.record, &response, t, &report.expected_status);
    drop(engine);

    report.end = match t {
        Terminal::Completed { .. } => RunEnd::Completed,
        Terminal::Failed { error, .. } => RunEnd::Failed(error.kind),
        Terminal::Cancelled => RunEnd::Cancelled,
    };
    report.delivery = finished.delivery;
    report.retry_safe = retry_safe(finished.delivery, report.method);
    report.status = ended.status;
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

/// The URL as Explain would show it, without the query. Secret values are masked first.
fn display_for(
    tree: &CollectionTree,
    rel: &Path,
    draft: &Request,
    env: Option<&Environment>,
) -> String {
    let masked_env = env.map(masked);
    let scope: Scope = tree.scope_for(rel, masked_env.as_ref());
    display_url(&explain(draft, &scope).url.0)
}

/// A skip reason or `None`; used by callers that print assertions.
pub fn skip_reason(c: &CheckOutcome) -> Option<&str> {
    match &c.result.verdict {
        Verdict::Skipped { reason } => Some(reason),
        _ => None,
    }
}
