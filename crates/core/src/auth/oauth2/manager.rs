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

    /// Runs a store call on the blocking pool, so a slow store never stalls an engine worker.
    /// Callers hold the key's store lock across the await, which keeps one key's calls in order.
    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn RefreshTokenStore) -> Result<T, RefreshStoreError> + Send + 'static,
    ) -> Result<T, RefreshStoreError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&*store))
            .await
            .unwrap_or_else(|_| Err(RefreshStoreError::new("the refresh token store failed")))
    }

    async fn save_blocking(&self, key: &TokenKey, token: &str) -> Result<(), RefreshStoreError> {
        let (key, token) = (key.clone(), token.to_owned());
        self.blocking(move |s| s.save(&key, &token)).await
    }

    async fn forget_blocking(&self, key: &TokenKey) -> Result<(), RefreshStoreError> {
        let key = key.clone();
        self.blocking(move |s| s.forget(&key)).await
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
    async fn put(
        &self,
        p: &Params<'_>,
        resp: &TokenResponse,
        keep_refresh: bool,
        refreshed: Option<(u64, Secret)>,
    ) -> bool {
        let expires_at = resp
            .expires_in
            .and_then(|d| self.clock.now().checked_add(d));
        let lock = self.store_lock(&p.key);
        let _store_guard = lock.lock().await;
        {
            let mut map = self.entries();
            if let Some((epoch, _)) = &refreshed
                && self.generation(&p.key) != *epoch
            {
                return false;
            }
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
            } else if let Some((_, used)) = refreshed {
                // The old refresh token stays valid; remember it in memory.
                e.refresh = Some(used);
            }
        }
        let saved = match &resp.refresh_token {
            Some(rt) if p.persist_refresh => match self.save_blocking(&p.key, rt.expose()).await {
                Ok(()) => {
                    self.stale().remove(&p.key);
                    true
                }
                Err(e) => {
                    tracing::warn!(error = %e, "cannot store the refresh token; it stays in memory");
                    false
                }
            },
            _ => false,
        };
        if !saved && !keep_refresh {
            // A re-authorize leaves no earlier authorization's token in the store.
            match self.forget_blocking(&p.key).await {
                Ok(()) => {
                    self.stale().remove(&p.key);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "cannot forget the stored refresh token");
                    self.stale().insert(p.key.clone());
                }
            }
        }
        true
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
        if !self.put(&p, &resp, true, Some((epoch, refresh))).await {
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
                    let guard = self.store_lock(&p.key).lock_owned().await;
                    epoch = self.generation(&p.key);
                    held = self.entries().get(&p.key).and_then(|e| e.refresh.clone());
                    if held.is_none() && !self.stale().contains(&p.key) {
                        // The owned guard moves into the blocking call: a cancelled send
                        // must not release the lock while the store call still runs.
                        let (store, key) = (self.inner.store.clone(), p.key.clone());
                        let loaded = tokio::task::spawn_blocking(move || {
                            let r = store.load(&key);
                            drop(guard);
                            r
                        })
                        .await
                        .unwrap_or_else(|_| {
                            Err(RefreshStoreError::new("the refresh token store failed"))
                        });
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
        self.put(&p, &resp, true, None).await;
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
        self.put(&p, &resp, false, None).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::oauth2::MemoryOnly;

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
}
