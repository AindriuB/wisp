//! The token cache and the two grant flows.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use super::token::{ClientCreds, Secret, TokenResponse, check_endpoint, post};
use super::{Grant, RefreshTokenStore, TokenKey, loopback, pkce};
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

/// The token cache. One per engine.
pub(crate) struct OAuth2 {
    cache: Mutex<HashMap<TokenKey, Entry>>,
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
            cache: Mutex::default(),
            store,
            clock,
        }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<TokenKey, Entry>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
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
    /// has none, `keep_refresh` says whether the old one stays.
    fn put(&self, p: &Params<'_>, resp: &TokenResponse, keep_refresh: bool) {
        let expires_at = resp
            .expires_in
            .and_then(|d| self.clock.now().checked_add(d));
        {
            let mut map = self.entries();
            let e = map.entry(p.key.clone()).or_default();
            e.access = Some(Access {
                token: resp.access_token.clone(),
                expires_at,
            });
            if let Some(rt) = &resp.refresh_token {
                e.refresh = Some(rt.clone());
            } else if !keep_refresh {
                e.refresh = None;
            }
        }
        if let (true, Some(rt)) = (p.persist_refresh, &resp.refresh_token)
            && let Err(e) = self.store.save(&p.key, rt.expose())
        {
            tracing::warn!(error = %e, "cannot store the refresh token; it stays in memory");
        }
    }

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
                let held = self.entries().get(&p.key).and_then(|e| e.refresh.clone());
                let refresh = match held {
                    Some(r) => r,
                    None if p.persist_refresh => match self.store.load(&p.key) {
                        Ok(Some(r)) => Secret::new(r),
                        Ok(None) => return Err(not_authorized()),
                        Err(e) => {
                            return Err(err(
                                ErrorKind::Auth,
                                format!("cannot read stored refresh token: {e}"),
                            ));
                        }
                    },
                    None => return Err(not_authorized()),
                };
                let fields = [
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh.expose()),
                ];
                let resp = post(&http, &url, &p.creds, &fields, timeout).await?;
                if resp.refresh_token.is_none() {
                    // The old refresh token stays valid; remember it in memory.
                    self.entries().entry(p.key.clone()).or_default().refresh = Some(refresh);
                }
                resp
            }
        };
        self.put(&p, &resp, true);
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
            cb = loopback::wait(listener) => cb,
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
        match &callback.state {
            Some(got) if same(got.expose(), state.expose()) => {}
            _ => {
                return Err(err(
                    ErrorKind::Auth,
                    "authorization failed: the state did not match",
                ));
            }
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
        self.put(&p, &resp, false);
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
