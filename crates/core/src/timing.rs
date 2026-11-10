//! Phase timings of one request.
//!
//! Attribution works through a registry shared by all clients of an engine. A send runs inside
//! a task-local scope carrying a token. The connector layer, on a connect it starts under a
//! token, measures the connect and records one entry keyed by the socket addresses of the new
//! connection (`HttpInfo`). The resolver records its lookup time into the connect that
//! triggered it. When the response arrives, the send claims the entry whose token is its own
//! and whose connection is the one that served it. A request that reused a connection, or
//! whose own connect lost the race and was dropped, finds nothing and reports `None`. An entry
//! is claimed at most once, so a connection shared by HTTP/2 streams is reported once.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use hyper_util::client::legacy::connect::{Connection, HttpInfo};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::{Deserialize, Serialize};

/// Connection-phase timings. `None` means not measured or a reused connection, never zero.
///
/// With redirects that reqwest follows itself, the values describe the connection that served
/// the final response; with the engine's own redirect loop, the first hop that opened one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Timings {
    pub dns: Option<Duration>,
    /// TCP connect and TLS handshake combined.
    pub connect: Option<Duration>,
}

type Key = (SocketAddr, SocketAddr);

/// Most unclaimed entries kept; the oldest is dropped beyond this.
const MAX_ENTRIES: usize = 64;

struct Entry {
    token: u64,
    key: Key,
    timings: Timings,
}

/// Per-engine store of connect measurements awaiting the request that waited on them.
#[derive(Default)]
pub(crate) struct Registry {
    next: AtomicU64,
    entries: Mutex<Vec<Entry>>,
}

tokio::task_local! {
    static TOKEN: u64;
    static CONN: Arc<Mutex<Option<Duration>>>;
}

impl Registry {
    pub(crate) fn new_token(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// Runs `fut` so that connects it starts are attributed to `token`.
    pub(crate) async fn scope<F: Future>(&self, token: u64, fut: F) -> F::Output {
        TOKEN.scope(token, fut).await
    }

    fn record(&self, entry: Entry) {
        let mut v = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|e| e.key != entry.key);
        if v.len() >= MAX_ENTRIES {
            v.remove(0);
        }
        v.push(entry);
    }

    /// The timings of the connection that served `resp`, if `token` opened it.
    pub(crate) fn claim(&self, token: u64, resp: &reqwest::Response) -> Timings {
        let Some(info) = resp.extensions().get::<HttpInfo>() else {
            return Timings::default();
        };
        let key = (info.local_addr(), info.remote_addr());
        let mut v = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        match v.iter().position(|e| e.key == key && e.token == token) {
            Some(i) => v.remove(i).timings,
            None => Timings::default(),
        }
    }
}

/// A DNS resolver that times its lookups into the connect that asked for them.
pub(crate) struct TimedResolver;

impl Resolve for TimedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let slot = CONN.try_with(Arc::clone).ok();
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let start = Instant::now();
            let found = tokio::net::lookup_host((host.as_str(), 0)).await;
            if let Some(slot) = slot {
                *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(start.elapsed());
            }
            let addrs = found?.collect::<Vec<_>>().into_iter();
            Ok(Box::new(addrs) as Addrs)
        })
    }
}

/// Connector layer measuring each connect started under a `Registry::scope`.
#[derive(Clone)]
pub(crate) struct TimingLayer(pub(crate) Arc<Registry>);

impl<S> tower_layer::Layer<S> for TimingLayer {
    type Service = TimingService<S>;
    fn layer(&self, inner: S) -> TimingService<S> {
        TimingService {
            inner,
            registry: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct TimingService<S> {
    inner: S,
    registry: Arc<Registry>,
}

type BoxFut<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

impl<S, Req> tower_service::Service<Req> for TimingService<S>
where
    S: tower_service::Service<Req>,
    S::Response: Connection + Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFut<S::Response, S::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let token = TOKEN.try_with(|t| *t).ok();
        let registry = self.registry.clone();
        let fut = self.inner.call(req);
        Box::pin(async move {
            let Some(token) = token else {
                return fut.await;
            };
            let dns = Arc::new(Mutex::new(None));
            let start = Instant::now();
            let res = CONN.scope(dns.clone(), fut).await?;
            let elapsed = start.elapsed();
            let mut ext = http::Extensions::new();
            res.connected().get_extras(&mut ext);
            if let Some(info) = ext.get::<HttpInfo>() {
                let dns = *dns.lock().unwrap_or_else(|e| e.into_inner());
                registry.record(Entry {
                    token,
                    key: (info.local_addr(), info.remote_addr()),
                    timings: Timings {
                        dns,
                        connect: Some(elapsed.saturating_sub(dns.unwrap_or_default())),
                    },
                });
            }
            Ok(res)
        })
    }
}
