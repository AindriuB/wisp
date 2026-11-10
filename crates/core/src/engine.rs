//! The engine: owns a small tokio runtime, a body store and a cache of reqwest clients.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::oneshot;

use crate::auth::oauth2::{Clock, MemoryOnly, OAuth2, RefreshTokenStore, TokenKey};
use crate::auth::{EngineAuth, digest, sigv4};
use crate::body::{BodyHandle, BodyStore, DEFAULT_MEMORY_LIMIT};
use crate::client::{ClientCache, HTTP1_MAX_RESPONSE_HEADERS};
use crate::cookies::CookieJars;
use crate::model::{
    ClientSettings, Command, EngineError, ErrorKind, Event, Header, HttpVersion, Method,
    PreparedBody, PreparedPart, PreparedPartValue, PreparedRequest, RequestId,
};
use crate::timing::Timings;

/// Receiving end of the engine's event channel; works from any executor or plain threads.
pub type EventReceiver = async_channel::Receiver<Event>;

/// Minimum spacing between `Progress` events of one request.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Engine configuration.
#[derive(Clone)]
pub struct EngineConfig {
    /// Clamped to `1..=2`.
    pub worker_threads: usize,
    pub default_timeout: Option<Duration>,
    /// Response bodies larger than this spill to disk.
    pub body_memory_limit: u64,
    /// Where OAuth2 refresh tokens persist between runs.
    pub refresh_tokens: Arc<dyn RefreshTokenStore>,
    /// The in-memory cookie jars, one per collection; the app keeps a clone to reach them.
    pub cookies: CookieJars,
}

impl fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineConfig")
            .field("worker_threads", &self.worker_threads)
            .field("default_timeout", &self.default_timeout)
            .field("body_memory_limit", &self.body_memory_limit)
            .field("refresh_tokens", &format_args!("<redacted>"))
            .field("cookies", &self.cookies)
            .finish()
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            worker_threads: 1,
            default_timeout: Some(Duration::from_secs(30)),
            body_memory_limit: DEFAULT_MEMORY_LIMIT,
            refresh_tokens: Arc::new(MemoryOnly::default()),
            cookies: CookieJars::default(),
        }
    }
}

/// Rate limiter for progress events; takes the time as an argument so tests need no clock.
#[derive(Debug)]
pub(crate) struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    /// True if an event may be emitted at `now`; records the emission.
    pub(crate) fn allow(&mut self, now: Instant) -> bool {
        match self.last {
            Some(last) if now.saturating_duration_since(last) < self.interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

type Inflight = Arc<Mutex<HashMap<RequestId, oneshot::Sender<()>>>>;

struct Inner {
    runtime: Option<tokio::runtime::Runtime>,
    clients: Arc<ClientCache>,
    bodies: BodyStore,
    events: async_channel::Sender<Event>,
    next_id: AtomicU64,
    inflight: Inflight,
    default_timeout: Option<Duration>,
    oauth: Arc<OAuth2>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.take() {
            // Never blocks, and is safe inside another runtime.
            rt.shutdown_background();
        }
    }
}

/// Handle to the engine. Cheap to clone; the last drop shuts the runtime down.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

fn lock(m: &Inflight) -> std::sync::MutexGuard<'_, HashMap<RequestId, oneshot::Sender<()>>> {
    // A poisoned map is still a valid map; keep going.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    /// The store holding completed response bodies; read and release them here.
    pub fn bodies(&self) -> BodyStore {
        self.inner.bodies.clone()
    }

    /// Start the engine and return it with its event receiver.
    ///
    /// # Panics
    /// Panics only if the OS cannot create the runtime or TLS client at startup.
    pub fn start(config: EngineConfig) -> (Engine, EventReceiver) {
        Self::start_with_clock(config, Arc::new(crate::auth::oauth2::SystemClock))
    }

    /// Like [`Engine::start`], with the clock that decides when cached OAuth2 tokens expire.
    ///
    /// # Panics
    /// Panics only if the OS cannot create the runtime or TLS client at startup.
    pub fn start_with_clock(
        config: EngineConfig,
        clock: Arc<dyn Clock>,
    ) -> (Engine, EventReceiver) {
        let threads = clamp_threads(config.worker_threads);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .enable_all()
            .thread_name("wisp-engine")
            .build()
            .expect("tokio runtime creation fails only on OS resource exhaustion at startup");
        let (tx, rx) = async_channel::unbounded();
        let inner = Inner {
            runtime: Some(runtime),
            clients: Arc::new(ClientCache::new(config.cookies)),
            bodies: BodyStore::new(config.body_memory_limit),
            events: tx,
            next_id: AtomicU64::new(1),
            inflight: Arc::default(),
            default_timeout: config.default_timeout,
            oauth: Arc::new(OAuth2::new(config.refresh_tokens, clock)),
        };
        (
            Engine {
                inner: Arc::new(inner),
            },
            rx,
        )
    }

    /// Submit a command; returns immediately.
    pub fn submit(&self, cmd: Command) -> RequestId {
        match cmd {
            Command::Cancel(id) => {
                if let Some(tx) = lock(&self.inner.inflight).remove(&id) {
                    let _ = tx.send(());
                }
                id
            }
            Command::AuthorizeCancel(id) => {
                if let Some(tx) = lock(&self.inner.inflight).remove(&id) {
                    let _ = tx.send(());
                }
                id
            }
            Command::AuthorizeStart {
                auth,
                client,
                environment,
                timeout,
            } => {
                let id = RequestId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
                let (ctx, crx) = oneshot::channel();
                lock(&self.inner.inflight).insert(id, ctx);
                let clients = self.inner.clients.clone();
                let oauth = self.inner.oauth.clone();
                let events = self.inner.events.clone();
                let inflight = self.inner.inflight.clone();
                let fut = async move {
                    let result = oauth
                        .authorize(
                            &clients,
                            &events,
                            id,
                            &auth,
                            &client,
                            &environment,
                            timeout,
                            crx,
                        )
                        .await;
                    lock(&inflight).remove(&id);
                    let ev = match result {
                        Ok(()) => Event::AuthCompleted { id },
                        Err(error) => Event::AuthFailed { id, error },
                    };
                    let _ = events.send(ev).await;
                };
                match &self.inner.runtime {
                    Some(rt) => {
                        rt.spawn(fut);
                    }
                    None => tracing::warn!("engine runtime already shut down"),
                }
                id
            }
            Command::Send(req) => {
                let id = RequestId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
                let (ctx, crx) = oneshot::channel();
                lock(&self.inner.inflight).insert(id, ctx);
                let clients = self.inner.clients.clone();
                let oauth = self.inner.oauth.clone();
                let bodies = self.inner.bodies.clone();
                let events = self.inner.events.clone();
                let inflight = self.inner.inflight.clone();
                let timeout = req.timeout.or(self.inner.default_timeout);
                let fut = async move {
                    let start = Instant::now();
                    let _ = events
                        .send(Event::Started {
                            id,
                            at: SystemTime::now(),
                        })
                        .await;
                    let terminal = tokio::select! {
                        r = perform(id, &clients, &oauth, &bodies, &events, req, timeout, start) => {
                            Some(match r {
                                Ok((size, body)) => Event::Completed {
                                    id,
                                    total: start.elapsed(),
                                    size,
                                    body,
                                    timings: Timings::default(),
                                },
                                Err(error) => Event::Failed { id, error },
                            })
                        }
                        _ = crx => None,
                    };
                    lock(&inflight).remove(&id);
                    let ev = terminal.unwrap_or(Event::Cancelled { id });
                    tracing::debug!(id = id.0, "request finished");
                    let _ = events.send(ev).await;
                };
                match &self.inner.runtime {
                    Some(rt) => {
                        rt.spawn(fut);
                    }
                    None => tracing::warn!("engine runtime already shut down"),
                }
                id
            }
        }
    }
}

fn clamp_threads(n: usize) -> usize {
    n.clamp(1, 2)
}

pub(crate) fn err(kind: ErrorKind, message: impl Into<String>) -> EngineError {
    EngineError {
        kind,
        message: message.into(),
    }
}

fn version_of(v: reqwest::Version) -> HttpVersion {
    if v == reqwest::Version::HTTP_09 {
        HttpVersion::Http09
    } else if v == reqwest::Version::HTTP_10 {
        HttpVersion::Http10
    } else if v == reqwest::Version::HTTP_2 {
        HttpVersion::Http2
    } else if v == reqwest::Version::HTTP_3 {
        HttpVersion::Http3
    } else {
        HttpVersion::Http11
    }
}

fn body_file_error(path: &Path, e: &std::io::Error) -> EngineError {
    err(
        ErrorKind::BodyFile,
        format!("cannot read {}: {e}", path.display()),
    )
}

/// Bodies up to this size are sent from memory so a 307/308 redirect can resend them.
const MEMORY_BODY_LIMIT: u64 = 1024 * 1024;

/// Largest chunk read from disk when streaming a request body.
const STREAM_CHUNK: usize = 64 * 1024;

/// One run of request-body bytes: literal bytes, or a file opened and measured up front.
enum Segment {
    Mem(Vec<u8>),
    File {
        file: std::fs::File,
        path: PathBuf,
        len: u64,
    },
}

impl Segment {
    fn len(&self) -> u64 {
        match self {
            Segment::Mem(b) => b.len() as u64,
            Segment::File { len, .. } => *len,
        }
    }
}

/// Opens the file and records its size, so a bad path fails before any connection is made.
async fn open_segment(path: &Path) -> Result<Segment, EngineError> {
    let owned = path.to_owned();
    let opened = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&owned)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(std::io::Error::other("not a regular file"));
        }
        Ok((file, meta.len()))
    })
    .await;
    match opened {
        Ok(Ok((file, len))) => Ok(Segment::File {
            file,
            path: path.to_owned(),
            len,
        }),
        Ok(Err(e)) => Err(body_file_error(path, &e)),
        Err(e) => Err(err(
            ErrorKind::BodyFile,
            format!("cannot read {}: {e}", path.display()),
        )),
    }
}

/// Reads exactly `len` bytes of `file`, or fails if it is shorter.
fn read_exact_vec(
    file: &mut std::fs::File,
    len: u64,
    out: &mut Vec<u8>,
) -> Result<(), std::io::Error> {
    use std::io::Read;
    let before = out.len();
    file.take(len).read_to_end(out)?;
    if (out.len() - before) as u64 != len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "file shrank while reading",
        ));
    }
    Ok(())
}

/// Reads small segments into one buffer on the blocking pool.
async fn gather(segments: Vec<Segment>) -> Result<Vec<u8>, EngineError> {
    let joined = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for seg in segments {
            match seg {
                Segment::Mem(b) => out.extend_from_slice(&b),
                Segment::File {
                    mut file,
                    path,
                    len,
                } => read_exact_vec(&mut file, len, &mut out).map_err(|e| (path, e))?,
            }
        }
        Ok(out)
    })
    .await;
    match joined {
        Ok(Ok(v)) => Ok(v),
        Ok(Err((path, e))) => Err(body_file_error(&path, &e)),
        Err(e) => Err(err(ErrorKind::BodyFile, format!("body read failed: {e}"))),
    }
}

/// Streams segments in chunks of at most `STREAM_CHUNK`; reads happen on the blocking pool.
fn stream_body(segments: Vec<Segment>) -> reqwest::Body {
    use std::io::Read;
    // Two chunks in flight bounds memory at about 256 KiB
    // (two queued, the sender's buffer and the chunk being written).
    let (tx, rx) = async_channel::bounded::<Result<Vec<u8>, std::io::Error>>(2);
    // Detached: it ends when the receiver is dropped (send fails) or the data runs out.
    drop(tokio::task::spawn_blocking(move || {
        for seg in segments {
            match seg {
                Segment::Mem(b) => {
                    for c in b.chunks(STREAM_CHUNK) {
                        if tx.send_blocking(Ok(c.to_vec())).is_err() {
                            return;
                        }
                    }
                }
                Segment::File { mut file, len, .. } => {
                    let mut left = len;
                    while left > 0 {
                        let n = STREAM_CHUNK.min(usize::try_from(left).unwrap_or(STREAM_CHUNK));
                        let mut buf = vec![0u8; n];
                        let item = match file.read(&mut buf) {
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Ok(0) => Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "file shrank while sending",
                            )),
                            Ok(got) => {
                                buf.truncate(got);
                                left -= got as u64;
                                Ok(buf)
                            }
                            Err(e) => Err(e),
                        };
                        let failed = item.is_err();
                        if tx.send_blocking(item).is_err() || failed {
                            return;
                        }
                    }
                }
            }
        }
    }));
    reqwest::Body::wrap_stream(rx)
}

/// Attaches a body: from memory when small, else streamed with an exact `Content-Length`.
async fn attach_segments(
    rb: reqwest::RequestBuilder,
    segments: Vec<Segment>,
) -> Result<reqwest::RequestBuilder, EngineError> {
    let total: u64 = segments.iter().map(Segment::len).sum();
    if total <= MEMORY_BODY_LIMIT {
        Ok(rb.body(gather(segments).await?))
    } else {
        Ok(rb
            .header("content-length", total.to_string())
            .body(stream_body(segments)))
    }
}

/// Header-parameter value: quotes and line breaks would break out of the quoted string.
fn quote_param(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .flat_map(|c| {
            if c == '"' || c == '\\' {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

async fn build_multipart(
    boundary: &str,
    parts: &[PreparedPart],
) -> Result<Vec<Segment>, EngineError> {
    let mut out: Vec<Segment> = Vec::new();
    let mut head = Vec::new();
    for part in parts {
        head.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let name = quote_param(&part.name);
        match &part.value {
            PreparedPartValue::Text(t) => {
                head.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                head.extend_from_slice(t.as_bytes());
            }
            PreparedPartValue::File(path) => {
                let filename = path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let seg = open_segment(path).await?;
                head.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
                        quote_param(&filename)
                    )
                    .as_bytes(),
                );
                out.push(Segment::Mem(std::mem::take(&mut head)));
                out.push(seg);
            }
        }
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out.push(Segment::Mem(head));
    Ok(out)
}

/// Redirects followed by the engine for a streamed body before it gives up.
const MAX_REDIRECTS: u32 = 10;

/// The request body still to be attached to the next send.
enum Pending {
    None,
    Bytes(Vec<u8>),
    Segments(Vec<Segment>),
}

/// What is needed to rebuild a streamed body: literal bytes, or a file path and its first length.
enum SegmentSpec {
    Mem(Vec<u8>),
    File { path: PathBuf, len: u64 },
}

impl SegmentSpec {
    fn of(seg: &Segment) -> Self {
        match seg {
            Segment::Mem(b) => SegmentSpec::Mem(b.clone()),
            Segment::File { path, len, .. } => SegmentSpec::File {
                path: path.clone(),
                len: *len,
            },
        }
    }
}

/// Reopens every file; fails if one is gone or no longer the length of the first send.
async fn reopen(specs: &[SegmentSpec]) -> Result<Vec<Segment>, EngineError> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        match spec {
            SegmentSpec::Mem(b) => out.push(Segment::Mem(b.clone())),
            SegmentSpec::File { path, len } => {
                let changed = |why: String| {
                    err(
                        ErrorKind::BodyFile,
                        format!(
                            "{} changed before the redirect resend: {why}",
                            path.display()
                        ),
                    )
                };
                let seg = open_segment(path).await.map_err(|e| changed(e.message))?;
                if seg.len() != *len {
                    return Err(changed(format!("length {} was {len}", seg.len())));
                }
                out.push(seg);
            }
        }
    }
    Ok(out)
}

/// What a Digest retry resends: the body of the first attempt, held so it can be sent again.
enum Replay {
    None,
    Bytes(Vec<u8>),
    Specs(Vec<SegmentSpec>),
}

async fn replay_pending(r: &Replay) -> Result<Pending, EngineError> {
    Ok(match r {
        Replay::None => Pending::None,
        Replay::Bytes(b) => Pending::Bytes(b.clone()),
        Replay::Specs(s) => Pending::Segments(reopen(s).await?),
    })
}

/// True when the body reads from a file, so it cannot be held for a Digest resend.
fn body_has_file(body: &PreparedBody) -> bool {
    match body {
        PreparedBody::File(_) => true,
        PreparedBody::Multipart(parts) => parts
            .iter()
            .any(|p| matches!(p.value, PreparedPartValue::File(_))),
        _ => false,
    }
}

/// Feeds a file's first `len` bytes into `hasher`, read in bounded chunks from a fresh open.
/// Fails if the file is gone, no longer `len` long, or shrinks while read.
async fn hash_file(
    mut hasher: sha2::Sha256,
    path: &Path,
    len: u64,
) -> Result<sha2::Sha256, EngineError> {
    let owned = path.to_owned();
    let hashed = tokio::task::spawn_blocking(move || {
        use sha2::Digest as _;
        use std::io::Read;
        let mut file = std::fs::File::open(&owned)?;
        if file.metadata()?.len() != len {
            return Err(std::io::Error::other("length changed before signing"));
        }
        let mut buf = vec![0u8; STREAM_CHUNK];
        let mut left = len;
        while left > 0 {
            let want = STREAM_CHUNK.min(usize::try_from(left).unwrap_or(STREAM_CHUNK));
            match file.read(&mut buf[..want]) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "file shrank while signing",
                    ));
                }
                Ok(got) => {
                    hasher.update(&buf[..got]);
                    left -= got as u64;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(hasher)
    })
    .await;
    match hashed {
        Ok(Ok(h)) => Ok(h),
        Ok(Err(e)) => Err(body_file_error(path, &e)),
        Err(e) => Err(err(
            ErrorKind::BodyFile,
            format!("cannot read {}: {e}", path.display()),
        )),
    }
}

/// Payload hash for SigV4. S3 signs `UNSIGNED-PAYLOAD` for files and bodies over the memory
/// limit; other services need the real SHA-256, stream-hashed so a file is never held whole.
async fn payload_hash(pending: &Pending, service: &str) -> Result<String, EngineError> {
    match pending {
        Pending::None => Ok(sigv4::sha256_hex(&[])),
        Pending::Bytes(b) => Ok(sigv4::sha256_hex(b)),
        Pending::Segments(segs) => {
            use sha2::Digest as _;
            let total: u64 = segs.iter().map(Segment::len).sum();
            let has_file = segs.iter().any(|s| matches!(s, Segment::File { .. }));
            if service == "s3" && (has_file || total > MEMORY_BODY_LIMIT) {
                return Ok(sigv4::UNSIGNED_PAYLOAD.to_owned());
            }
            let mut hasher = sha2::Sha256::new();
            for seg in segs {
                match seg {
                    Segment::Mem(b) => hasher.update(b),
                    Segment::File { path, len, .. } => {
                        hasher = hash_file(hasher, path, *len).await?;
                    }
                }
            }
            Ok(crate::auth::hex(&hasher.finalize()))
        }
    }
}

/// Runs one request, emitting `Headers` and `Progress`; returns the size and body.
#[allow(clippy::too_many_arguments)]
async fn perform(
    id: RequestId,
    clients: &ClientCache,
    oauth: &OAuth2,
    bodies: &BodyStore,
    events: &async_channel::Sender<Event>,
    req: PreparedRequest,
    timeout: Option<Duration>,
    start: Instant,
) -> Result<(u64, BodyHandle), EngineError> {
    let url =
        reqwest::Url::parse(&req.url).map_err(|e| err(ErrorKind::InvalidUrl, e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(err(
            ErrorKind::InvalidUrl,
            format!("unsupported scheme `{}`", url.scheme()),
        ));
    }
    let digest_creds = match &req.auth {
        Some(EngineAuth::Digest { username, password }) => Some((username, password)),
        _ => None,
    };
    if digest_creds.is_some() && body_has_file(&req.body) {
        return Err(err(
            ErrorKind::Body,
            "Digest auth needs a replayable body; File bodies cannot be resent",
        ));
    }
    // The engine follows redirects itself for engine-applied auth (reqwest would carry the
    // x-amz-* headers across origins and could not sign again), so reqwest's following is off.
    let own_redirects = req.client.follow_redirects && req.auth.is_some();
    let client = if own_redirects {
        clients.get(&ClientSettings {
            follow_redirects: false,
            ..req.client.clone()
        })?
    } else {
        clients.get(&req.client)?
    };
    // The bearer token of OAuth2 auth, fetched or refreshed before the first byte is sent.
    let bearer: Option<(TokenKey, String)> = match &req.auth {
        Some(
            a @ (EngineAuth::OAuth2ClientCredentials { .. } | EngineAuth::OAuth2AuthCode { .. }),
        ) => {
            let (key, token) = oauth
                .bearer(clients, a, &req.client, &req.environment, timeout)
                .await?;
            Some((key, format!("Bearer {}", token.expose())))
        }
        _ => None,
    };
    let mut method = match req.method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    };
    let multipart = matches!(req.body, PreparedBody::Multipart(_));
    // The engine sets the length of these bodies itself; a user value would conflict.
    let engine_length = multipart || matches!(req.body, PreparedBody::File(_));
    let mut header_list: Vec<(String, String)> = Vec::new();
    for h in &req.headers {
        // Digest, SigV4 and OAuth2 own the Authorization header.
        if req.auth.is_some() && h.name.eq_ignore_ascii_case("authorization") {
            continue;
        }
        // The engine owns the multipart content type: it carries the boundary.
        if multipart && h.name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        if engine_length
            && (h.name.eq_ignore_ascii_case("content-length")
                || h.name.eq_ignore_ascii_case("transfer-encoding"))
        {
            continue;
        }
        header_list.push((h.name.clone(), h.value.clone()));
    }
    tracing::debug!(
        id = id.0,
        header_names = ?req.headers.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
        "sending request"
    );
    let mut pending = match req.body {
        PreparedBody::None => Pending::None,
        PreparedBody::Bytes(b) => Pending::Bytes(b),
        PreparedBody::File(path) => Pending::Segments(vec![open_segment(&path).await?]),
        PreparedBody::Multipart(parts) => {
            let boundary = format!("wisp-{}", uuid::Uuid::new_v4().simple());
            let segments = build_multipart(&boundary, &parts).await?;
            header_list.push((
                "content-type".to_owned(),
                format!("multipart/form-data; boundary={boundary}"),
            ));
            Pending::Segments(segments)
        }
    };
    let mut replay = match (&req.auth, &pending) {
        (Some(_), Pending::Bytes(b)) => Replay::Bytes(b.clone()),
        (Some(_), Pending::Segments(s)) => Replay::Specs(s.iter().map(SegmentSpec::of).collect()),
        _ => Replay::None,
    };
    let mut signing_hash = match &req.auth {
        Some(EngineAuth::AwsSigV4 { service, .. }) => Some(payload_hash(&pending, service).await?),
        _ => None,
    };
    // reqwest cannot clone a streamed body, so it returns a 307/308 instead of following it.
    // With redirects on, the engine follows those itself, re-reading the files.
    let mut plan: Option<Vec<SegmentSpec>> = match &pending {
        Pending::Segments(s)
            if req.client.follow_redirects
                && s.iter().map(Segment::len).sum::<u64>() > MEMORY_BODY_LIMIT =>
        {
            Some(s.iter().map(SegmentSpec::of).collect())
        }
        _ => None,
    };
    let began = Instant::now();
    let mut url = url;
    let mut hops = 0u32;
    let mut digest_header: Option<String> = None;
    // False once a redirect has left the original origin: no more SigV4 or Digest.
    let mut auth_live = true;
    let mut resp = loop {
        let mut rb = client.request(method.clone(), url.clone());
        for (n, v) in &header_list {
            rb = rb.header(n.as_str(), v.as_str());
        }
        if let (true, Some(h)) = (auth_live, &digest_header) {
            rb = rb.header("authorization", h.as_str());
        }
        if let (true, Some((_, h))) = (auth_live, &bearer) {
            let mut value = reqwest::header::HeaderValue::from_str(h).map_err(|_| {
                err(
                    ErrorKind::Auth,
                    "the access token is not a valid header value",
                )
            })?;
            value.set_sensitive(true);
            rb = rb.header(reqwest::header::AUTHORIZATION, value);
        }
        if let (
            Some(EngineAuth::AwsSigV4 {
                access_key,
                secret_key,
                session_token,
                region,
                service,
            }),
            Some(hash),
        ) = (&req.auth, &signing_hash)
            && auth_live
        {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let creds = sigv4::Credentials {
                access_key,
                secret_key,
                session_token,
                region,
                service,
            };
            let signed = sigv4::sign(
                method.as_str(),
                url.as_str(),
                &header_list,
                hash,
                &creds,
                now,
            )
            .map_err(|m| err(ErrorKind::Other, format!("SigV4 signing failed: {m}")))?;
            for (n, v) in &signed.added {
                rb = rb.header(n.as_str(), v.as_str());
            }
        }
        rb = match std::mem::replace(&mut pending, Pending::None) {
            Pending::None => rb,
            Pending::Bytes(b) => rb.body(b),
            Pending::Segments(s) => attach_segments(rb, s).await?,
        };
        if let Some(t) = timeout {
            let left = t.saturating_sub(began.elapsed());
            if left.is_zero() {
                return Err(err(ErrorKind::Timeout, "request timed out".to_owned()));
            }
            rb = rb.timeout(left);
        }
        let resp = rb.send().await.map_err(|e| classify(&e, false))?;
        if let (true, Some((username, password)), None, 401) = (
            auth_live,
            &digest_creds,
            &digest_header,
            resp.status().as_u16(),
        ) {
            let values: Vec<&str> = resp
                .headers()
                .get_all(reqwest::header::WWW_AUTHENTICATE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .collect();
            let selected =
                digest::select(&values).map_err(|e| err(ErrorKind::Other, e.to_string()))?;
            if let Some(challenge) = selected {
                let uri = match url.query() {
                    Some(q) => format!("{}?{q}", url.path()),
                    None => url.path().to_owned(),
                };
                let cnonce = uuid::Uuid::new_v4().simple().to_string();
                let header = digest::respond(
                    &challenge,
                    method.as_str(),
                    &uri,
                    username,
                    password,
                    &cnonce,
                )
                .map_err(|e| err(ErrorKind::Other, e.to_string()))?;
                digest_header = Some(header);
                pending = replay_pending(&replay).await?;
                continue;
            }
        }
        if !own_redirects && plan.is_none() {
            break resp;
        }
        let code = resp.status().as_u16();
        // 307/308 keep method and body. With own_redirects, 303 turns any non-HEAD method into
        // GET and 301/302 turn POST into GET, both without a body. Other methods keep their
        // method and body on 301/302, which RFC 9110 allows (reqwest rewrites more of them).
        let keeps_body = match code {
            307 | 308 => true,
            301..=303 if own_redirects => {
                !(method == reqwest::Method::POST && code != 303
                    || code == 303 && method != reqwest::Method::HEAD)
            }
            _ => break resp,
        };
        let next = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|l| url.join(l).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        let Some(next) = next else { break resp };
        hops += 1;
        if hops >= MAX_REDIRECTS {
            return Err(err(
                ErrorKind::Other,
                format!("too many redirects ({MAX_REDIRECTS})"),
            ));
        }
        if keeps_body {
            pending = match &plan {
                Some(specs) => Pending::Segments(reopen(specs).await?),
                None => replay_pending(&replay).await?,
            };
        } else {
            method = reqwest::Method::GET;
            pending = Pending::None;
            replay = Replay::None;
            plan = None;
            if signing_hash.is_some() {
                signing_hash = Some(sigv4::sha256_hex(&[]));
            }
            header_list.retain(|(n, _)| {
                !["content-type", "content-length", "content-encoding"]
                    .iter()
                    .any(|s| n.eq_ignore_ascii_case(s))
            });
        }
        digest_header = None;
        if next.origin() != url.origin() {
            auth_live = false;
            header_list.retain(|(n, _)| {
                ![
                    "authorization",
                    "cookie",
                    "cookie2",
                    "proxy-authorization",
                    "www-authenticate",
                ]
                .iter()
                .any(|s| n.eq_ignore_ascii_case(s))
            });
        }
        url = next;
    };
    // A 401 from the origin that got the token: drop it so the next send fetches a new one.
    if let (true, Some((key, _)), 401) = (auth_live, &bearer, resp.status().as_u16()) {
        oauth.invalidate(key);
    }
    let status = resp.status();
    let headers: Vec<Header> = resp
        .headers()
        .iter()
        .map(|(n, v)| Header {
            name: n.as_str().to_owned(),
            value: String::from_utf8_lossy(v.as_bytes()).into_owned(),
        })
        .collect();
    let _ = events
        .send(Event::Headers {
            id,
            status: status.as_u16(),
            reason: status.canonical_reason().map(str::to_owned),
            version: version_of(resp.version()),
            headers,
            ttfb: start.elapsed(),
        })
        .await;
    let mut writer = bodies.writer();
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let mut received: u64 = 0;
    while let Some(chunk) = resp.chunk().await.map_err(|e| classify(&e, true))? {
        // `block_in_place` (the runtime is always multi-thread): the write may hit the spill
        // file, and this keeps the blocking disk I/O from stalling other tasks on this worker.
        tokio::task::block_in_place(|| writer.write(&chunk))
            .map_err(|e| err(ErrorKind::Body, e.to_string()))?;
        received += chunk.len() as u64;
        if throttle.allow(Instant::now()) {
            let _ = events
                .send(Event::Progress {
                    id,
                    bytes: received,
                })
                .await;
        }
    }
    // Same reason as the write above: `finish` flushes the spill file.
    let handle = tokio::task::block_in_place(|| writer.finish())
        .map_err(|e| err(ErrorKind::Body, e.to_string()))?;
    Ok((received, handle))
}

fn classify(e: &reqwest::Error, reading_body: bool) -> EngineError {
    // Only the causes: the top-level text embeds the URL, whose words must not influence the kind.
    let mut chain = String::new();
    let mut message = e.to_string();
    let mut src: Option<&dyn StdError> = e.source();
    while let Some(s) = src {
        let text = s.to_string();
        chain.push_str(&text.to_lowercase());
        chain.push(' ');
        message.push_str(": ");
        message.push_str(&text);
        src = s.source();
    }
    if chain.contains("message head is too large") {
        // hyper reports a head over its byte buffer and one over its header-count limit identically.
        return EngineError {
            kind: ErrorKind::Other,
            message: format!(
                "response headers too large (limit {HTTP1_MAX_RESPONSE_HEADERS} headers, or the head exceeds hyper's size limit)"
            ),
        };
    }
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else if e.is_builder() {
        ErrorKind::Other
    } else if chain.contains("dns error")
        || chain.contains("failed to lookup")
        || chain.contains("name or service not known")
    {
        ErrorKind::Dns
    } else if chain.contains("certificate") || chain.contains("tls") || chain.contains("handshake")
    {
        ErrorKind::Tls
    } else if e.is_connect() {
        ErrorKind::Connect
    } else if reading_body || e.is_body() || e.is_decode() {
        ErrorKind::Body
    } else {
        ErrorKind::Other
    };
    EngineError { kind, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_threads_are_clamped_to_one_through_two() {
        assert_eq!(clamp_threads(0), 1);
        assert_eq!(clamp_threads(1), 1);
        assert_eq!(clamp_threads(2), 2);
        assert_eq!(clamp_threads(99), 2);
    }

    #[test]
    fn ten_updates_ten_ms_apart_emit_once() {
        let t0 = Instant::now();
        let mut t = Throttle::new(PROGRESS_INTERVAL);
        let n = (0..10)
            .filter(|i| t.allow(t0 + Duration::from_millis(10 * i)))
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn two_updates_150_ms_apart_emit_twice() {
        let t0 = Instant::now();
        let mut t = Throttle::new(PROGRESS_INTERVAL);
        assert!(t.allow(t0));
        assert!(t.allow(t0 + Duration::from_millis(150)));
    }
}
