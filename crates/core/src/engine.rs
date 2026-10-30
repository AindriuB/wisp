//! The engine: owns a small tokio runtime, a body store and a cache of reqwest clients.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::oneshot;

use crate::body::{BodyHandle, BodyStore, DEFAULT_MEMORY_LIMIT};
use crate::model::{
    ClientSettings, Command, EngineError, ErrorKind, Event, Header, HttpVersion, Method,
    PreparedBody, PreparedPart, PreparedPartValue, PreparedRequest, RequestId,
};

/// Receiving end of the engine's event channel; works from any executor or plain threads.
pub type EventReceiver = async_channel::Receiver<Event>;

/// Minimum spacing between `Progress` events of one request.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Engine configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// Clamped to `1..=2`.
    pub worker_threads: usize,
    pub default_timeout: Option<Duration>,
    /// Response bodies larger than this spill to disk.
    pub body_memory_limit: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            worker_threads: 1,
            default_timeout: Some(Duration::from_secs(30)),
            body_memory_limit: DEFAULT_MEMORY_LIMIT,
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

/// One `reqwest::Client` per distinct `ClientSettings`.
#[derive(Default)]
pub(crate) struct ClientCache {
    clients: Mutex<HashMap<ClientSettings, reqwest::Client>>,
}

impl ClientCache {
    pub(crate) fn get(&self, settings: &ClientSettings) -> Result<reqwest::Client, EngineError> {
        let mut map = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = map.get(settings) {
            return Ok(c.clone());
        }
        let mut builder = reqwest::Client::builder().use_rustls_tls();
        if !settings.verify_tls {
            builder = builder.danger_accept_invalid_certs(true);
        }
        if !settings.follow_redirects {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }
        if let Some(proxy) = &settings.proxy {
            let proxy = reqwest::Proxy::all(proxy.as_str())
                .map_err(|e| err(ErrorKind::InvalidUrl, format!("invalid proxy: {e}")))?;
            builder = builder.proxy(proxy);
        }
        let client = builder
            .build()
            .map_err(|e| err(ErrorKind::Other, format!("cannot build client: {e}")))?;
        map.insert(settings.clone(), client.clone());
        Ok(client)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).len()
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
            clients: Arc::default(),
            bodies: BodyStore::new(config.body_memory_limit),
            events: tx,
            next_id: AtomicU64::new(1),
            inflight: Arc::default(),
            default_timeout: config.default_timeout,
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
            Command::Send(req) => {
                let id = RequestId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
                let (ctx, crx) = oneshot::channel();
                lock(&self.inner.inflight).insert(id, ctx);
                let clients = self.inner.clients.clone();
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
                        r = perform(id, &clients, &bodies, &events, req, timeout, start) => {
                            Some(match r {
                                Ok((size, body)) => Event::Completed {
                                    id,
                                    total: start.elapsed(),
                                    size,
                                    body,
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

fn err(kind: ErrorKind, message: impl Into<String>) -> EngineError {
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

async fn read_file(path: &Path) -> Result<Vec<u8>, EngineError> {
    let owned = path.to_owned();
    match tokio::task::spawn_blocking(move || std::fs::read(&owned)).await {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(e)) => Err(body_file_error(path, &e)),
        Err(e) => Err(err(
            ErrorKind::BodyFile,
            format!("cannot read {}: {e}", path.display()),
        )),
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

async fn build_multipart(boundary: &str, parts: &[PreparedPart]) -> Result<Vec<u8>, EngineError> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let name = quote_param(&part.name);
        match &part.value {
            PreparedPartValue::Text(t) => {
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                out.extend_from_slice(t.as_bytes());
            }
            PreparedPartValue::File(path) => {
                let filename = path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let data = read_file(path).await?;
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
                        quote_param(&filename)
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(&data);
            }
        }
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(out)
}

/// Runs one request, emitting `Headers` and `Progress`; returns the size and body.
async fn perform(
    id: RequestId,
    clients: &ClientCache,
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
    let client = clients.get(&req.client)?;
    let method = match req.method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    };
    let multipart = matches!(req.body, PreparedBody::Multipart(_));
    let mut rb = client.request(method, url);
    for h in &req.headers {
        // The engine owns the multipart content type: it carries the boundary.
        if multipart && h.name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        rb = rb.header(h.name.as_str(), h.value.as_str());
    }
    tracing::debug!(
        id = id.0,
        header_names = ?req.headers.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
        "sending request"
    );
    match req.body {
        PreparedBody::None => {}
        PreparedBody::Bytes(b) => rb = rb.body(b),
        PreparedBody::File(path) => rb = rb.body(read_file(&path).await?),
        PreparedBody::Multipart(parts) => {
            let boundary = format!("wisp-{}", uuid::Uuid::new_v4().simple());
            let data = build_multipart(&boundary, &parts).await?;
            rb = rb
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(data);
        }
    }
    if let Some(t) = timeout {
        rb = rb.timeout(t);
    }
    let mut resp = rb.send().await.map_err(|e| classify(&e, false))?;
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
        writer
            .write(&chunk)
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
    let handle = writer
        .finish()
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

    fn settings(verify_tls: bool) -> ClientSettings {
        ClientSettings {
            verify_tls,
            follow_redirects: true,
            proxy: None,
        }
    }

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

    #[test]
    fn equal_settings_share_a_client_and_different_settings_do_not() {
        let cache = ClientCache::default();
        cache.get(&settings(true)).unwrap();
        cache.get(&settings(true)).unwrap();
        assert_eq!(cache.len(), 1);
        cache.get(&settings(false)).unwrap();
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn an_invalid_proxy_is_an_invalid_url_error() {
        let mut s = settings(true);
        s.proxy = Some("not a proxy".into());
        let e = ClientCache::default().get(&s).unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidUrl);
    }
}
