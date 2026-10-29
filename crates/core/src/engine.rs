//! The engine: owns a small tokio runtime and one `reqwest::Client`.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::model::{
    Command, EngineError, ErrorKind, Event, Header, Method, PreparedRequest, RequestId,
};

/// Receiving end of the engine's event channel; works from any executor or plain threads.
pub type EventReceiver = async_channel::Receiver<Event>;

/// Engine configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// Clamped to `1..=2`.
    pub worker_threads: usize,
    pub default_timeout: Option<Duration>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            worker_threads: 1,
            default_timeout: Some(Duration::from_secs(30)),
        }
    }
}

type Inflight = Arc<Mutex<HashMap<RequestId, oneshot::Sender<()>>>>;

struct Inner {
    runtime: Option<tokio::runtime::Runtime>,
    client: reqwest::Client,
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
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .expect("client builder with default rustls config is infallible at startup");
        let (tx, rx) = async_channel::unbounded();
        let inner = Inner {
            runtime: Some(runtime),
            client,
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
                let client = self.inner.client.clone();
                let events = self.inner.events.clone();
                let inflight = self.inner.inflight.clone();
                let timeout = req.timeout.or(self.inner.default_timeout);
                let fut = async move {
                    let start = Instant::now();
                    let result = tokio::select! {
                        r = perform(&client, req, timeout, start) => r,
                        _ = crx => Err(EngineError {
                            kind: ErrorKind::Cancelled,
                            message: "request cancelled".into(),
                        }),
                    };
                    lock(&inflight).remove(&id);
                    let ev = match result {
                        Ok(mut ev) => {
                            if let Event::Completed { id: slot, .. } = &mut ev {
                                *slot = id;
                            }
                            ev
                        }
                        Err(error) => Event::Failed { id, error },
                    };
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

async fn perform(
    client: &reqwest::Client,
    req: PreparedRequest,
    timeout: Option<Duration>,
    start: Instant,
) -> Result<Event, EngineError> {
    let url =
        reqwest::Url::parse(&req.url).map_err(|e| err(ErrorKind::InvalidUrl, e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(err(
            ErrorKind::InvalidUrl,
            format!("unsupported scheme `{}`", url.scheme()),
        ));
    }
    let method = match req.method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    };
    let mut rb = client.request(method, url);
    for h in &req.headers {
        rb = rb.header(h.name.as_str(), h.value.as_str());
    }
    if let Some(b) = req.body {
        rb = rb.body(b);
    }
    if let Some(t) = timeout {
        rb = rb.timeout(t);
    }
    let resp = rb.send().await.map_err(|e| classify(&e, false))?;
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(n, v)| Header {
            name: n.as_str().to_owned(),
            value: String::from_utf8_lossy(v.as_bytes()).into_owned(),
        })
        .collect();
    let body = resp.bytes().await.map_err(|e| classify(&e, true))?.to_vec();
    Ok(Event::Completed {
        id: RequestId(0), // overwritten by the caller
        status: status.as_u16(),
        reason: status.canonical_reason().map(str::to_owned),
        headers,
        total: start.elapsed(),
        size: body.len() as u64,
        body,
    })
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
    use super::clamp_threads;

    #[test]
    fn worker_threads_are_clamped_to_one_through_two() {
        assert_eq!(clamp_threads(0), 1);
        assert_eq!(clamp_threads(1), 1);
        assert_eq!(clamp_threads(2), 2);
        assert_eq!(clamp_threads(99), 2);
    }
}
