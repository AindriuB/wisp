//! The token cache and the two grant flows.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use super::token::{ClientCreds, Secret, TokenResponse, check_endpoint, post};
use super::{Grant, RefreshStoreError, RefreshTokenStore, TokenKey, loopback, pkce};
use crate::auth::EngineAuth;
use crate::client::ClientCache;
use crate::engine::err;
use crate::model::{ClientSettings, EngineError, ErrorKind, Event, RequestId};

/// An access token is refetched this long before it expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(30);
/// How long the user has to finish authorizing when the command names no timeout.
const DEFAULT_AUTHORIZE_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a token request may take when the caller names no timeout.
const DEFAULT_TOKEN_TIMEOUT: Duration = Duration::from_secs(30);
/// How long acquiring a key's store lock, or one store call, may take before the operation
/// fails closed. A keychain prompt that nobody answers must not wedge a key forever.
const STORE_TIMEOUT: Duration = Duration::from_secs(30);
/// The message of a store timeout. Fixed: it holds no token.
const STORE_TIMED_OUT: &str = "the refresh token store timed out";

/// A future that resolves when the command that started an operation is cancelled.
type CancelFut<'a> = &'a mut (dyn std::future::Future<Output = ()> + Unpin + Send);

/// Why a bounded wait for the store lock or a store call ended without a result.
enum Stop {
    Cancelled,
    TimedOut,
    /// The blocking task panicked.
    Failed,
}

impl Stop {
    fn error(&self) -> EngineError {
        match self {
            Stop::Cancelled => err(ErrorKind::Auth, "cancelled"),
            Stop::TimedOut => err(ErrorKind::Auth, STORE_TIMED_OUT),
            Stop::Failed => err(ErrorKind::Auth, "the refresh token store failed"),
        }
    }
}

type Guard = tokio::sync::OwnedMutexGuard<()>;

/// Waits for `fut`, but at most [`STORE_TIMEOUT`] and not past `cancel`.
async fn bounded<T>(
    fut: impl std::future::Future<Output = T>,
    cancel: &mut Option<CancelFut<'_>>,
) -> Result<T, Stop> {
    let cancelled = async {
        match cancel {
            Some(c) => c.await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        v = fut => Ok(v),
        () = cancelled => Err(Stop::Cancelled),
        () = tokio::time::sleep(STORE_TIMEOUT) => Err(Stop::TimedOut),
    }
}

/// The engine's source of the current time for token expiry; tests inject their own.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

struct Access {
    token: Secret,
    /// `None` when the server gave no lifetime: used until a 401 drops it.
    expires_at: Option<Instant>,
}

#[derive(Default)]
struct Entry {
    access: Option<Access>,
    refresh: Option<Secret>,
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("access", &self.access.as_ref().map(|_| "<redacted>"))
            .field("refresh", &self.refresh.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Everything about an auth that the flows need, borrowed from an [`EngineAuth`].
struct Params<'a> {
    key: TokenKey,
    token_url: &'a str,
    creds: ClientCreds<'a>,
    scope: &'a str,
    persist_refresh: bool,
}

fn params<'a>(
    auth: &'a EngineAuth,
    collection: &Option<String>,
    environment: &Option<String>,
) -> Option<Params<'a>> {
    let (grant, token_url, client_id, client_secret, scope, client_auth, persist_refresh) =
        match auth {
            EngineAuth::OAuth2ClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
            } => (
                Grant::ClientCredentials,
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
                false,
            ),
            EngineAuth::OAuth2AuthCode {
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
                persist_refresh,
                ..
            } => (
                Grant::AuthCode,
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
                *persist_refresh,
            ),
            _ => return None,
        };
    Some(Params {
        key: TokenKey {
            grant,
            token_url: token_url.clone(),
            client_id: client_id.clone(),
            scope: scope.clone(),
            collection: collection.clone(),
            environment: environment.clone(),
        },
        token_url,
        creds: ClientCreds {
            id: client_id,
            secret: client_secret,
            mode: *client_auth,
        },
        scope,
        persist_refresh,
    })
}

/// The token cache. One per engine. A handle to shared state, so a refresh can run in a task
/// that outlives the send that started it.
pub(crate) struct OAuth2 {
    inner: Arc<Inner>,
}

impl std::ops::Deref for OAuth2 {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.inner
    }
}

pub(crate) struct Inner {
    cache: Mutex<HashMap<TokenKey, Entry>>,
    /// Keys whose stored refresh token is stale: a forget failed, so `bearer` must not load it.
    stale: Mutex<std::collections::HashSet<TokenKey>>,
    /// Bumped by every authorize of a key; a refresh that started before it must not land.
    generation: Mutex<HashMap<TokenKey, u64>>,
    /// Held across "check generation, then call the store" so a late refresh cannot overwrite
    /// a re-authorize's store state. Never held with the in-memory locks across a store call.
    store_locks: Mutex<HashMap<TokenKey, Arc<tokio::sync::Mutex<()>>>>,
    store: Arc<dyn RefreshTokenStore>,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for OAuth2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2")
            .field("cache", &format_args!("<redacted>"))
            .finish_non_exhaustive()
    }
}

fn not_authorized() -> EngineError {
    err(ErrorKind::Auth, "Not authorized: use Authorize first")
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

impl OAuth2 {
    pub(crate) fn new(store: Arc<dyn RefreshTokenStore>, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Inner {
                cache: Mutex::default(),
                stale: Mutex::default(),
                generation: Mutex::default(),
                store_locks: Mutex::default(),
                store,
                clock,
            }),
        }
    }
}

impl Inner {
    fn stale(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<TokenKey>> {
        self.stale.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<TokenKey, Entry>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn store_lock(&self, key: &TokenKey) -> Arc<tokio::sync::Mutex<()>> {
        let mut m = self.store_locks.lock().unwrap_or_else(|e| e.into_inner());
        m.entry(key.clone()).or_default().clone()
    }

    /// Takes the key's store lock, bounded by [`STORE_TIMEOUT`] and `cancel`.
    async fn lock_store(
        &self,
        key: &TokenKey,
        cancel: &mut Option<CancelFut<'_>>,
    ) -> Result<Guard, Stop> {
        bounded(self.store_lock(key).lock_owned(), cancel).await
    }

    /// Runs a store call on the blocking pool, so a slow store never stalls an engine worker.
    /// The key's lock guard moves into the blocking task and comes back with the result. A
    /// call that times out or is cancelled cannot be killed: its guard stays with it and is
    /// released only when the call really returns, so a later call for the key waits on the
    /// lock (and times out there) instead of overlapping it.
    async fn blocking<T: Send + 'static>(
        &self,
        guard: Guard,
        cancel: &mut Option<CancelFut<'_>>,
        f: impl FnOnce(&dyn RefreshTokenStore) -> Result<T, RefreshStoreError> + Send + 'static,
    ) -> Result<(Guard, Result<T, RefreshStoreError>), Stop> {
        let store = self.store.clone();
        let task = tokio::task::spawn_blocking(move || {
            let r = f(&*store);
            (guard, r)
        });
        match bounded(task, cancel).await? {
            Ok(done) => Ok(done),
            Err(_) => Err(Stop::Failed),
        }
    }

    fn generation(&self, key: &TokenKey) -> u64 {
        let g = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        g.get(key).copied().unwrap_or(0)
    }

    fn fresh(&self, key: &TokenKey) -> Option<Secret> {
        let now = self.clock.now();
        let map = self.entries();
        let access = map.get(key)?.access.as_ref()?;
        match access.expires_at {
            Some(at) if now + EXPIRY_MARGIN >= at => None,
            _ => Some(access.token.clone()),
        }
    }

    /// Drops the cached access token of `key` (after a 401). The refresh token stays.
    pub(crate) fn invalidate(&self, key: &TokenKey) {
        if let Some(e) = self.entries().get_mut(key) {
            e.access = None;
        }
    }

    /// Caches a token response. A new refresh token replaces the old one; when the response
    /// has none, `keep_refresh` says whether the old one stays. If not (an authorize), the
    /// stored one is forgotten whatever `persist_refresh` says, and the key's generation moves
    /// on. `refreshed` is `(generation read before the refresh, the refresh token it used)`:
    /// when the generation has moved, nothing is cached or saved and `false` is returned.
    ///
    /// Fails closed, caching nothing, when the key's store lock is not won within
    /// [`STORE_TIMEOUT`] or `cancel` fires first. The store is written before the cache: an
    /// authorize whose save or forget fails, times out or is cancelled changes nothing in
    /// memory and returns a token-free error. A call that times out or is cancelled leaves
    /// the key stale (the stored token is unknown). A refresh whose save fails or times out
    /// still caches its token in memory.
    async fn put(
        &self,
        p: &Params<'_>,
        resp: &TokenResponse,
        keep_refresh: bool,
        refreshed: Option<(u64, Secret)>,
        mut cancel: Option<CancelFut<'_>>,
    ) -> Result<bool, EngineError> {
        let expires_at = resp
            .expires_in
            .and_then(|d| self.clock.now().checked_add(d));
        let used = refreshed.as_ref().map(|(_, u)| u.clone());
        let guard = self
            .lock_store(&p.key, &mut cancel)
            .await
            .map_err(|s| s.error())?;
        if let Some((epoch, _)) = &refreshed
            && self.generation(&p.key) != *epoch
        {
            return Ok(false);
        }
        if keep_refresh {
            // The IdP has already revoked the old refresh token: cache the new one first, so
            // a failed, panicked, stuck or cancelled save cannot lose it.
            self.cache(p, resp, keep_refresh, expires_at, used.as_ref());
        }
        // An authorize writes the store before anything is cached: one that fails, times out
        // or is cancelled while saving changes nothing in memory.
        let save = match (&resp.refresh_token, p.persist_refresh) {
            (Some(rt), true) => Some(rt.expose().to_owned()),
            _ => None,
        };
        let persisting = save.is_some();
        let key = p.key.clone();
        let outcome = match save {
            Some(token) => {
                self.blocking(guard, &mut cancel, move |s| s.save(&key, &token))
                    .await
            }
            None if !keep_refresh => {
                // A re-authorize leaves no earlier authorization's token in the store.
                self.blocking(guard, &mut cancel, move |s| s.forget(&key))
                    .await
            }
            None => Ok((guard, Ok(()))),
        };
        // Held until the cache is updated.
        let mut _held = None;
        match outcome {
            Ok((g, Ok(()))) => {
                _held = Some(g);
                if persisting || !keep_refresh {
                    self.stale().remove(&p.key);
                }
            }
            Ok((g, Err(e))) => {
                _held = Some(g);
                if keep_refresh {
                    tracing::warn!(error = %e, "cannot store the refresh token; it stays in memory");
                    self.stale().insert(p.key.clone());
                    return Ok(true);
                }
                tracing::warn!(error = %e, "the refresh token store failed; nothing was cached");
                let what = if persisting {
                    "cannot save refresh token"
                } else {
                    "cannot remove stored refresh token"
                };
                return Err(err(ErrorKind::Auth, format!("{what}: {e}")));
            }
            Err(stop) => {
                // The call may still land: the stored token is unknown.
                self.stale().insert(p.key.clone());
                if keep_refresh {
                    tracing::warn!(
                        "the refresh token store gave no answer; the token stays in memory"
                    );
                    return Ok(true);
                }
                return Err(stop.error());
            }
        }
        if !keep_refresh {
            self.cache(p, resp, keep_refresh, expires_at, used.as_ref());
        }
        Ok(true)
    }

    /// Updates the in-memory entry of `p.key` from `resp` (and bumps the generation of an
    /// authorize).
    fn cache(
        &self,
        p: &Params<'_>,
        resp: &TokenResponse,
        keep_refresh: bool,
        expires_at: Option<Instant>,
        used: Option<&Secret>,
    ) {
        let mut map = self.entries();
        if !keep_refresh {
            let mut g = self.generation.lock().unwrap_or_else(|e| e.into_inner());
            *g.entry(p.key.clone()).or_insert(0) += 1;
        }
        let e = map.entry(p.key.clone()).or_default();
        e.access = Some(Access {
            token: resp.access_token.clone(),
            expires_at,
        });
        if let Some(rt) = &resp.refresh_token {
            e.refresh = Some(rt.clone());
        } else if !keep_refresh {
            e.refresh = None;
        } else if let Some(used) = used {
            // The old refresh token stays valid; remember it in memory.
            e.refresh = Some(used.clone());
        }
    }

    /// One refresh-token grant and the caching of its result.
    #[allow(clippy::too_many_arguments)]
    async fn refresh(
        self: Arc<Self>,
        auth: EngineAuth,
        collection: Option<String>,
        environment: Option<String>,
        http: reqwest::Client,
        url: reqwest::Url,
        epoch: u64,
        refresh: Secret,
        timeout: Duration,
    ) -> Result<(TokenKey, Secret), EngineError> {
        let p = params(&auth, &collection, &environment)
            .ok_or_else(|| err(ErrorKind::Auth, "not an OAuth2 auth"))?;
        let fields = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.expose()),
        ];
        let resp = post(&http, &url, &p.creds, &fields, timeout).await?;
        if !self
            .put(&p, &resp, true, Some((epoch, refresh)), None)
            .await?
        {
            // A re-authorize landed meanwhile: use its state, never this result.
            return match self.fresh(&p.key) {
                Some(t) => Ok((p.key, t)),
                None => Err(not_authorized()),
            };
        }
        Ok((p.key, resp.access_token))
    }
}

impl OAuth2 {
    /// The bearer token for a send: cached, refreshed or fetched. Returns its cache key too.
    pub(crate) async fn bearer(
        &self,
        clients: &ClientCache,
        auth: &EngineAuth,
        client: &ClientSettings,
        environment: &Option<String>,
        timeout: Option<Duration>,
    ) -> Result<(TokenKey, Secret), EngineError> {
        let p = params(auth, &client.collection, environment)
            .ok_or_else(|| err(ErrorKind::Auth, "not an OAuth2 auth"))?;
        let url = check_endpoint(p.token_url, "token_url")?;
        if let Some(t) = self.fresh(&p.key) {
            return Ok((p.key, t));
        }
        let http = clients.get(&ClientSettings {
            follow_redirects: false,
            ..client.clone()
        })?;
        let timeout = timeout.unwrap_or(DEFAULT_TOKEN_TIMEOUT);
        let resp = match p.key.grant {
            Grant::ClientCredentials => {
                let mut fields = vec![("grant_type", "client_credentials")];
                if !p.scope.is_empty() {
                    fields.push(("scope", p.scope));
                }
                post(&http, &url, &p.creds, &fields, timeout).await?
            }
            Grant::AuthCode => {
                let mut epoch = self.generation(&p.key);
                let mut held = self.entries().get(&p.key).and_then(|e| e.refresh.clone());
                if held.is_none() && p.persist_refresh {
                    // Serialise with a re-authorize's store writes, then look again.
                    let guard = self
                        .lock_store(&p.key, &mut None)
                        .await
                        .map_err(|s| s.error())?;
                    epoch = self.generation(&p.key);
                    held = self.entries().get(&p.key).and_then(|e| e.refresh.clone());
                    if held.is_none() && !self.stale().contains(&p.key) {
                        // The guard moves into the blocking call: a timed-out or cancelled
                        // send must not release the lock while the store call still runs.
                        let key = p.key.clone();
                        let loaded =
                            match self.blocking(guard, &mut None, move |s| s.load(&key)).await {
                                Ok((_, r)) => r,
                                Err(stop) => return Err(stop.error()),
                            };
                        held = match loaded {
                            Ok(Some(r)) => Some(Secret::new(r)),
                            Ok(None) => return Err(not_authorized()),
                            Err(e) => {
                                return Err(err(
                                    ErrorKind::Auth,
                                    format!("cannot read stored refresh token: {e}"),
                                ));
                            }
                        };
                    }
                }
                let Some(refresh) = held else {
                    return Err(not_authorized());
                };
                // The POST and the save of its result run in a task of their own: dropping
                // this future (a cancelled send) must not lose a rotated refresh token the
                // IdP has already issued.
                let task = tokio::spawn(self.inner.clone().refresh(
                    auth.clone(),
                    client.collection.clone(),
                    environment.clone(),
                    http,
                    url,
                    epoch,
                    refresh,
                    timeout,
                ));
                return match task.await {
                    Ok(r) => r,
                    Err(_) => Err(err(ErrorKind::Auth, "token refresh was interrupted")),
                };
            }
        };
        self.put(&p, &resp, true, None, None).await?;
        Ok((p.key, resp.access_token))
    }

    /// Runs the authorization-code flow of `AuthorizeStart`. Emits `AuthUrl`; the caller emits
    /// the terminal event. The listener is closed before this returns.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn authorize(
        &self,
        clients: &ClientCache,
        events: &async_channel::Sender<Event>,
        id: RequestId,
        auth: &EngineAuth,
        client: &ClientSettings,
        environment: &Option<String>,
        timeout: Option<Duration>,
        mut cancel: oneshot::Receiver<()>,
    ) -> Result<(), EngineError> {
        let EngineAuth::OAuth2AuthCode {
            auth_url,
            redirect_port,
            ..
        } = auth
        else {
            return Err(err(
                ErrorKind::Auth,
                "Authorize needs the OAuth2 authorization-code auth",
            ));
        };
        let p = params(auth, &client.collection, environment)
            .ok_or_else(|| err(ErrorKind::Auth, "not an OAuth2 auth"))?;
        let token_url = check_endpoint(p.token_url, "token_url")?;
        let mut url = check_endpoint(auth_url, "auth_url")?;
        let http = clients.get(&ClientSettings {
            follow_redirects: false,
            ..client.clone()
        })?;
        let (listener, port) = loopback::bind(*redirect_port).await.map_err(|e| {
            err(
                ErrorKind::Auth,
                format!("cannot listen on 127.0.0.1:{redirect_port}: {e}"),
            )
        })?;
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let verifier = Secret::new(pkce::new_verifier());
        let state = Secret::new(pkce::new_state());
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("response_type", "code")
                .append_pair("client_id", p.creds.id)
                .append_pair("redirect_uri", &redirect_uri);
            if !p.scope.is_empty() {
                q.append_pair("scope", p.scope);
            }
            q.append_pair("state", state.expose())
                .append_pair("code_challenge", &pkce::challenge(verifier.expose()))
                .append_pair("code_challenge_method", "S256");
        }
        tracing::debug!(id = id.0, port, "authorization listener bound");
        let _ = events
            .send(Event::AuthUrl {
                id,
                url: url.into(),
            })
            .await;

        let cancelled = async {
            if (&mut cancel).await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        tokio::pin!(cancelled);
        // `wait` owns the listener: it is closed as soon as this select ends.
        let callback = tokio::select! {
            cb = loopback::wait(listener, |cb| {
                matches!(&cb.state, Some(got) if same(got.expose(), state.expose()))
            }) => cb,
            () = &mut cancelled => return Err(err(ErrorKind::Auth, "cancelled")),
            () = tokio::time::sleep(timeout.unwrap_or(DEFAULT_AUTHORIZE_TIMEOUT)) => {
                return Err(err(ErrorKind::Auth, "authorization timed out"));
            }
        };
        if let Some(code) = &callback.error {
            return Err(err(
                ErrorKind::Auth,
                format!("authorization was refused: {code}"),
            ));
        }
        let Some(code) = &callback.code else {
            return Err(err(
                ErrorKind::Auth,
                "authorization failed: no code in the redirect",
            ));
        };
        let fields = [
            ("grant_type", "authorization_code"),
            ("code", code.expose()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.expose()),
        ];
        let resp = tokio::select! {
            r = post(&http, &token_url, &p.creds, &fields, DEFAULT_TOKEN_TIMEOUT) => r?,
            () = &mut cancelled => return Err(err(ErrorKind::Auth, "cancelled")),
        };
        self.put(&p, &resp, false, None, Some(&mut cancelled))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::oauth2::MemoryOnly;
    use crate::cookies::CookieJars;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn same_compares_whole_strings() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }

    #[test]
    fn debug_redacts_cache_entries_and_the_manager() {
        let m = OAuth2::new(Arc::new(MemoryOnly::default()), Arc::new(SystemClock));
        let key = TokenKey {
            grant: Grant::AuthCode,
            token_url: "https://i/t".into(),
            client_id: "c".into(),
            scope: String::new(),
            collection: None,
            environment: None,
        };
        m.entries().insert(
            key,
            Entry {
                access: Some(Access {
                    token: Secret::new("ACCESSMARK"),
                    expires_at: None,
                }),
                refresh: Some(Secret::new("REFRESHMARK")),
            },
        );
        let e = Entry {
            access: Some(Access {
                token: Secret::new("ACCESSMARK"),
                expires_at: None,
            }),
            refresh: Some(Secret::new("REFRESHMARK")),
        };
        for text in [
            format!("{m:?}"),
            format!("{m:#?}"),
            format!("{e:?}"),
            format!("{e:#?}"),
        ] {
            assert!(text.contains("<redacted>"), "{text}");
            assert!(!text.contains("ACCESSMARK") && !text.contains("REFRESHMARK"));
        }
    }

    fn code_auth() -> EngineAuth {
        EngineAuth::OAuth2AuthCode {
            auth_url: "https://i/a".into(),
            token_url: "https://i/t".into(),
            client_id: "c".into(),
            client_secret: String::new(),
            scope: String::new(),
            client_auth: crate::auth::ClientAuth::default(),
            redirect_port: 0,
            persist_refresh: true,
        }
    }

    fn settings() -> ClientSettings {
        ClientSettings {
            verify_tls: true,
            follow_redirects: false,
            proxy: None,
            collection: None,
        }
    }

    fn token_resp() -> TokenResponse {
        TokenResponse {
            access_token: Secret::new("AT"),
            refresh_token: Some(Secret::new("RT")),
            expires_in: Some(Duration::from_secs(100)),
        }
    }

    /// A store whose `load` (or `save`, if `stick_save`) blocks until `release` is set;
    /// `entered` says a blocking call was reached, `loads` counts `load` calls.
    #[derive(Default)]
    struct Stuck {
        release: AtomicBool,
        entered: AtomicBool,
        stick_save: bool,
        panic_save: bool,
        loads: AtomicUsize,
    }

    impl Stuck {
        fn wait(&self) {
            self.entered.store(true, Ordering::SeqCst);
            while !self.release.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
        }
    }

    /// Releases the stuck call when dropped, so a failing assert cannot hang the runtime.
    struct Release(Arc<Stuck>);

    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release.store(true, Ordering::SeqCst);
        }
    }

    impl RefreshTokenStore for Stuck {
        fn load(&self, _: &TokenKey) -> Result<Option<String>, RefreshStoreError> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            if !self.stick_save {
                self.wait();
            }
            Ok(None)
        }
        fn save(&self, _: &TokenKey, _: &str) -> Result<(), RefreshStoreError> {
            if self.panic_save {
                panic!("SAVE_SENTINEL");
            }
            if self.stick_save {
                self.wait();
            }
            Ok(())
        }
        fn forget(&self, _: &TokenKey) -> Result<(), RefreshStoreError> {
            Ok(())
        }
    }

    async fn bearer(m: &OAuth2, auth: &EngineAuth) -> Result<(TokenKey, Secret), EngineError> {
        let clients = ClientCache::new(CookieJars::default());
        m.bearer(&clients, auth, &settings(), &None, None).await
    }

    #[tokio::test(start_paused = true)]
    async fn a_load_that_never_returns_fails_a_send_closed_after_the_bound() {
        let store = Arc::new(Stuck::default());
        let _release = Release(store.clone());
        let m = OAuth2::new(store.clone(), Arc::new(SystemClock));
        let auth = code_auth();
        // Tokio does not auto-advance paused time while a blocking task runs: advance by hand.
        let advance = async {
            while !store.entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            tokio::time::advance(STORE_TIMEOUT + Duration::from_secs(1)).await;
        };
        let (first, ()) = tokio::join!(bearer(&m, &auth), advance);
        assert_eq!(first.unwrap_err().message, STORE_TIMED_OUT);
        // The load still runs and still owns the key's lock: the next send times out on it.
        let advance = async {
            tokio::task::yield_now().await;
            tokio::time::advance(STORE_TIMEOUT + Duration::from_secs(1)).await;
        };
        let (second, ()) = tokio::join!(bearer(&m, &auth), advance);
        assert_eq!(second.unwrap_err().message, STORE_TIMED_OUT);
        assert!(m.entries().is_empty(), "nothing was cached");
        // The lock was never given up: the second send did not start another load.
        assert_eq!(store.loads.load(Ordering::SeqCst), 1);
    }

    fn seed_old(m: &OAuth2, key: &TokenKey) {
        m.entries().insert(
            key.clone(),
            Entry {
                access: Some(Access {
                    token: Secret::new("OLD"),
                    expires_at: None,
                }),
                refresh: Some(Secret::new("OLDRT")),
            },
        );
    }

    fn cached_access(m: &OAuth2, key: &TokenKey) -> Option<String> {
        let t = m.fresh(key)?;
        Some(t.expose().to_owned())
    }

    /// Runs an authorize `put` against a store stuck in `save`; it is then cancelled or timed out.
    async fn stuck_put(
        cancel_it: bool,
    ) -> (OAuth2, TokenKey, Result<bool, EngineError>, Arc<Stuck>) {
        let store = Arc::new(Stuck {
            stick_save: true,
            ..Stuck::default()
        });
        let m = OAuth2::new(store.clone(), Arc::new(SystemClock));
        let auth = code_auth();
        let p = params(&auth, &None, &None).unwrap();
        seed_old(&m, &p.key);
        let (tx, rx) = oneshot::channel::<()>();
        let mut cancelled = Box::pin(async move {
            let _ = rx.await;
        });
        let act = async {
            while !store.entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            if cancel_it {
                tx.send(()).unwrap();
                None
            } else {
                tokio::time::advance(STORE_TIMEOUT + Duration::from_secs(1)).await;
                // Kept open: a dropped sender would read as a cancel.
                Some(tx)
            }
        };
        let resp = token_resp();
        let (r, _tx) = tokio::join!(m.put(&p, &resp, false, None, Some(&mut cancelled)), act);
        (m, p.key, r, store)
    }

    fn refresh_of(m: &OAuth2, key: &TokenKey) -> Option<String> {
        let map = m.entries();
        Some(map.get(key)?.refresh.as_ref()?.expose().to_owned())
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_caches_the_rotated_token_while_its_save_is_stuck() {
        let store = Arc::new(Stuck {
            stick_save: true,
            ..Stuck::default()
        });
        let _release = Release(store.clone());
        let m = OAuth2::new(store.clone(), Arc::new(SystemClock));
        let auth = code_auth();
        let p = params(&auth, &None, &None).unwrap();
        seed_old(&m, &p.key);
        let resp = token_resp();
        let act = async {
            while !store.entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            // Still saving: a second send must already see RT, not OLDRT.
            assert_eq!(refresh_of(&m, &p.key).as_deref(), Some("RT"));
            tokio::time::advance(STORE_TIMEOUT + Duration::from_secs(1)).await;
        };
        let (r, ()) = tokio::join!(
            m.put(&p, &resp, true, Some((0, Secret::new("OLDRT"))), None),
            act
        );
        assert!(r.unwrap());
        assert_eq!(refresh_of(&m, &p.key).as_deref(), Some("RT"));
        assert!(m.stale().contains(&p.key));
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_whose_save_panics_still_caches_the_rotated_token() {
        let store = Arc::new(Stuck {
            panic_save: true,
            ..Stuck::default()
        });
        let m = OAuth2::new(store, Arc::new(SystemClock));
        let auth = code_auth();
        let p = params(&auth, &None, &None).unwrap();
        seed_old(&m, &p.key);
        let r = m
            .put(
                &p,
                &token_resp(),
                true,
                Some((0, Secret::new("OLDRT"))),
                None,
            )
            .await;
        assert!(r.unwrap());
        assert_eq!(refresh_of(&m, &p.key).as_deref(), Some("RT"));
        assert_eq!(cached_access(&m, &p.key).as_deref(), Some("AT"));
        assert!(m.stale().contains(&p.key));
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancel_during_the_save_keeps_the_previous_authorization() {
        let (m, key, r, store) = stuck_put(true).await;
        let _release = Release(store);
        assert_eq!(r.unwrap_err().message, "cancelled");
        assert_eq!(cached_access(&m, &key).as_deref(), Some("OLD"));
        assert_eq!(m.generation(&key), 0, "generation unchanged");
        assert!(m.stale().contains(&key), "the save may have landed");
    }

    #[tokio::test(start_paused = true)]
    async fn a_timeout_during_the_save_keeps_the_previous_authorization() {
        let (m, key, r, store) = stuck_put(false).await;
        let _release = Release(store);
        assert_eq!(r.unwrap_err().message, STORE_TIMED_OUT);
        assert_eq!(cached_access(&m, &key).as_deref(), Some("OLD"));
        assert_eq!(m.generation(&key), 0, "generation unchanged");
        assert!(m.stale().contains(&key));
    }

    #[tokio::test(start_paused = true)]
    async fn a_saved_authorization_is_cached_and_bumps_the_generation() {
        let m = OAuth2::new(Arc::new(MemoryOnly::default()), Arc::new(SystemClock));
        let auth = code_auth();
        let p = params(&auth, &None, &None).unwrap();
        seed_old(&m, &p.key);
        assert!(m.put(&p, &token_resp(), false, None, None).await.unwrap());
        assert_eq!(cached_access(&m, &p.key).as_deref(), Some("AT"));
        assert_eq!(m.generation(&p.key), 1);
        assert!(!m.stale().contains(&p.key));
    }

    #[tokio::test(start_paused = true)]
    async fn an_authorize_waiting_on_the_lock_is_cancelled_promptly() {
        let m = OAuth2::new(Arc::new(MemoryOnly::default()), Arc::new(SystemClock));
        let auth = code_auth();
        let p = params(&auth, &None, &None).unwrap();
        // Holds the key's store lock, as a blocked store call would.
        let _held = m.store_lock(&p.key).lock_owned().await;
        let (tx, rx) = oneshot::channel::<()>();
        let mut cancelled = Box::pin(async move {
            let _ = rx.await;
        });
        tx.send(()).unwrap();
        let started = tokio::time::Instant::now();
        let e = m
            .put(&p, &token_resp(), false, None, Some(&mut cancelled))
            .await
            .unwrap_err();
        assert_eq!(e.message, "cancelled");
        assert!(started.elapsed() < STORE_TIMEOUT);
        assert!(m.entries().is_empty(), "nothing was cached");
    }

    #[tokio::test(start_paused = true)]
    async fn an_authorize_waiting_on_the_lock_times_out_and_fails_closed() {
        let m = OAuth2::new(Arc::new(MemoryOnly::default()), Arc::new(SystemClock));
        let auth = code_auth();
        let p = params(&auth, &None, &None).unwrap();
        let _held = m.store_lock(&p.key).lock_owned().await;
        let (_tx, rx) = oneshot::channel::<()>();
        let mut cancelled = Box::pin(async move {
            let _ = rx.await;
        });
        let e = m
            .put(&p, &token_resp(), false, None, Some(&mut cancelled))
            .await
            .unwrap_err();
        assert_eq!(e.message, STORE_TIMED_OUT);
        assert!(m.entries().is_empty(), "nothing was cached");
    }
}
