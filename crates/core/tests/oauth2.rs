//! OAuth2 in the engine, against local wiremock servers and the engine's own loopback listener.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wisp_core::auth::EngineAuth;
use wisp_core::auth::oauth2::{
    Clock, Grant, MemoryOnly, RefreshStoreError, RefreshTokenStore, TokenKey,
};
use wisp_core::request::ClientAuth;
use wisp_core::*;

fn settings(collection: Option<&str>) -> ClientSettings {
    ClientSettings {
        verify_tls: true,
        follow_redirects: true,
        proxy: None,
        collection: collection.map(str::to_owned),
    }
}

fn request(url: String, auth: EngineAuth) -> PreparedRequest {
    PreparedRequest {
        method: Method::Get,
        url,
        headers: vec![],
        body: PreparedBody::None,
        timeout: None,
        client: settings(None),
        warnings: vec![],
        auth: Some(auth),
        environment: None,
    }
}

fn cc(token_url: &str) -> EngineAuth {
    EngineAuth::OAuth2ClientCredentials {
        token_url: token_url.into(),
        client_id: "cid".into(),
        client_secret: "csecret".into(),
        scope: "read".into(),
        client_auth: ClientAuth::Basic,
    }
}

fn code_auth(base: &str, port: u16, persist: bool) -> EngineAuth {
    EngineAuth::OAuth2AuthCode {
        auth_url: format!("{base}/authorize?audience=api"),
        token_url: format!("{base}/token"),
        client_id: "cid".into(),
        client_secret: "csecret".into(),
        scope: "read write".into(),
        client_auth: ClientAuth::Basic,
        redirect_port: port,
        persist_refresh: persist,
    }
}

fn token_body(access: &str) -> serde_json::Value {
    json!({"access_token": access, "token_type": "Bearer", "expires_in": 3600})
}

async fn token_server(access: &str) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body(access)))
        .mount(&s)
        .await;
    s
}

async fn resource_server() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    s
}

/// Requests the server saw at `p`, with their bodies as text.
async fn seen(s: &MockServer, p: &str) -> Vec<(String, String)> {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == p)
        .map(|r| {
            (
                r.headers
                    .get("authorization")
                    .map(|v| v.to_str().unwrap().to_owned())
                    .unwrap_or_default(),
                String::from_utf8_lossy(&r.body).into_owned(),
            )
        })
        .collect()
}

struct TestClock(Mutex<Instant>);

impl TestClock {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Instant::now())))
    }
    fn advance(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

/// A store that records every call and can be made to fail.
#[derive(Default)]
struct Recording {
    inner: MemoryOnly,
    loads: Mutex<Vec<TokenKey>>,
    saves: Mutex<Vec<(TokenKey, String)>>,
    fail_load: bool,
    fail_save: bool,
}

impl RefreshTokenStore for Recording {
    fn load(&self, key: &TokenKey) -> Result<Option<String>, RefreshStoreError> {
        self.loads.lock().unwrap().push(key.clone());
        if self.fail_load {
            return Err(RefreshStoreError::new("keychain locked"));
        }
        self.inner.load(key)
    }
    fn save(&self, key: &TokenKey, rt: &str) -> Result<(), RefreshStoreError> {
        self.saves
            .lock()
            .unwrap()
            .push((key.clone(), rt.to_owned()));
        if self.fail_save {
            return Err(RefreshStoreError::new("keychain denied"));
        }
        self.inner.save(key, rt)
    }
    fn forget(&self, key: &TokenKey) -> Result<(), RefreshStoreError> {
        self.inner.forget(key)
    }
}

fn engine_with(store: Arc<dyn RefreshTokenStore>) -> (Engine, EventReceiver, Arc<TestClock>) {
    let clock = TestClock::new();
    let (engine, rx) = Engine::start_with_clock(
        EngineConfig {
            refresh_tokens: store,
            ..EngineConfig::default()
        },
        clock.clone(),
    );
    (engine, rx, clock)
}

fn engine() -> (Engine, EventReceiver, Arc<TestClock>) {
    engine_with(Arc::new(MemoryOnly::default()))
}

/// Sends and waits for the terminal event.
async fn send(engine: &Engine, rx: &EventReceiver, req: PreparedRequest) -> Vec<Event> {
    engine.submit(Command::Send(req));
    let mut out = Vec::new();
    loop {
        let ev = rx.recv().await.unwrap();
        let done = matches!(
            ev,
            Event::Completed { .. } | Event::Failed { .. } | Event::Cancelled { .. }
        );
        out.push(ev);
        if done {
            return out;
        }
    }
}

fn status_of(evs: &[Event]) -> Option<u16> {
    evs.iter().find_map(|e| match e {
        Event::Headers { status, .. } => Some(*status),
        _ => None,
    })
}

fn auth_error(evs: &[Event]) -> EngineError {
    match evs.last().unwrap() {
        Event::Failed { error, .. } | Event::AuthFailed { error, .. } => {
            assert_eq!(error.kind, ErrorKind::Auth, "{error:?}");
            error.clone()
        }
        other => panic!("unexpected {other:?}"),
    }
}

// ---- token endpoint ----

#[tokio::test]
async fn token_url_must_be_https_or_loopback_http() {
    let (engine, rx, _) = engine();
    let target = resource_server().await;
    let evs = send(
        &engine,
        &rx,
        request(target.uri(), cc("http://idp.test/token")),
    )
    .await;
    let e = auth_error(&evs);
    assert!(e.message.contains("token_url"), "{e:?}");
    assert_eq!(evs.len(), 2, "no Headers event: {evs:?}");
    assert!(target.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn token_requests_never_follow_redirects() {
    let elsewhere = token_server("STOLEN").await;
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/token", elsewhere.uri())),
        )
        .mount(&idp)
        .await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let evs = send(
        &engine,
        &rx,
        request(target.uri(), cc(&format!("{}/token", idp.uri()))),
    )
    .await;
    auth_error(&evs);
    assert!(elsewhere.received_requests().await.unwrap().is_empty());
    assert!(target.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn basic_client_auth_puts_credentials_in_the_header() {
    let idp = token_server("AT").await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    send(
        &engine,
        &rx,
        request(target.uri(), cc(&format!("{}/token", idp.uri()))),
    )
    .await;
    let reqs = seen(&idp, "/token").await;
    let want = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("cid:csecret")
    );
    assert_eq!(reqs[0].0, want);
    assert!(
        reqs[0].1.contains("grant_type=client_credentials"),
        "{}",
        reqs[0].1
    );
    assert!(reqs[0].1.contains("scope=read"));
    assert!(!reqs[0].1.contains("csecret") && !reqs[0].1.contains("client_id"));
}

#[tokio::test]
async fn body_client_auth_puts_credentials_in_the_form() {
    let idp = token_server("AT").await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let mut auth = cc(&format!("{}/token", idp.uri()));
    if let EngineAuth::OAuth2ClientCredentials { client_auth, .. } = &mut auth {
        *client_auth = ClientAuth::Body;
    }
    send(&engine, &rx, request(target.uri(), auth)).await;
    let reqs = seen(&idp, "/token").await;
    assert_eq!(reqs[0].0, "");
    assert!(reqs[0].1.contains("client_id=cid"));
    assert!(reqs[0].1.contains("client_secret=csecret"));
}

#[tokio::test]
async fn public_client_sends_the_id_and_no_secret() {
    let idp = token_server("AT").await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let mut auth = cc(&format!("{}/token", idp.uri()));
    if let EngineAuth::OAuth2ClientCredentials { client_secret, .. } = &mut auth {
        client_secret.clear();
    }
    send(&engine, &rx, request(target.uri(), auth)).await;
    let reqs = seen(&idp, "/token").await;
    assert_eq!(reqs[0].0, "");
    assert!(reqs[0].1.contains("client_id=cid"));
    assert!(!reqs[0].1.contains("client_secret"));
}

#[tokio::test]
async fn a_token_type_other_than_bearer_is_an_auth_error() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "SECRETVAL", "token_type": "mac"})),
        )
        .mount(&idp)
        .await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let evs = send(
        &engine,
        &rx,
        request(target.uri(), cc(&format!("{}/token", idp.uri()))),
    )
    .await;
    let e = auth_error(&evs);
    assert!(e.message.contains("bearer"), "{e:?}");
    assert!(!e.message.contains("SECRETVAL"));
    assert!(target.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn failing_responses_hold_the_status_and_a_redacted_capped_body() {
    let padding = "x".repeat(5000);
    let bodies = [
        (
            500u16,
            format!(
                r#"{{"access_token":"SECRETVAL","refresh_token":"SECRETVAL","id_token":"SECRETVAL","pad":"{padding}"}}"#
            ),
        ),
        (
            200,
            r#"{"access_token":"SECRETVAL","refresh_token":"SECRETVAL","id_token":"SECRETVAL"#
                .to_owned(),
        ),
    ];
    for (status, body) in bodies {
        let idp = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&idp)
            .await;
        let target = resource_server().await;
        let (engine, rx, _) = engine();
        let evs = send(
            &engine,
            &rx,
            request(target.uri(), cc(&format!("{}/token", idp.uri()))),
        )
        .await;
        let e = auth_error(&evs);
        assert!(e.message.contains(&status.to_string()), "{e:?}");
        assert!(!e.message.contains("SECRETVAL"), "{e:?}");
        assert!(e.message.contains("<redacted>"), "{e:?}");
        assert!(e.message.len() < 2048 + 200, "{}", e.message.len());
    }
}

#[tokio::test]
async fn a_token_response_over_one_mib_is_refused() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(1024 * 1024 + 10)))
        .mount(&idp)
        .await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let evs = send(
        &engine,
        &rx,
        request(target.uri(), cc(&format!("{}/token", idp.uri()))),
    )
    .await;
    assert!(auth_error(&evs).message.contains("1 MiB"));
}

// ---- sending ----

#[tokio::test]
async fn client_credentials_fetch_once_reuse_and_refetch_after_expiry() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token": "AT-1", "token_type": "bearer", "expires_in": 100}),
        ))
        .expect(2)
        .mount(&idp)
        .await;
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("authorization", "Bearer AT-1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(3)
        .mount(&target)
        .await;
    let (engine, rx, clock) = engine();
    let auth = cc(&format!("{}/token", idp.uri()));
    let mut req = request(target.uri(), auth);
    // A user-set Authorization is replaced.
    req.headers.push(Header {
        name: "Authorization".into(),
        value: "Basic dXNlcjpwdw==".into(),
    });
    assert_eq!(status_of(&send(&engine, &rx, req.clone()).await), Some(200));
    clock.advance(Duration::from_secs(60));
    send(&engine, &rx, req.clone()).await;
    // 71 s in: within 30 s of the 100 s expiry, so the token is fetched again.
    clock.advance(Duration::from_secs(11));
    send(&engine, &rx, req).await;
}

#[tokio::test]
async fn a_401_drops_the_cached_token() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("AT")))
        .expect(2)
        .mount(&idp)
        .await;
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .expect(2)
        .mount(&target)
        .await;
    let (engine, rx, _) = engine();
    let req = request(target.uri(), cc(&format!("{}/token", idp.uri())));
    assert_eq!(status_of(&send(&engine, &rx, req.clone()).await), Some(401));
    // No automatic retry: one resource hit per send.
    assert_eq!(target.received_requests().await.unwrap().len(), 1);
    send(&engine, &rx, req).await;
}

#[tokio::test]
async fn sends_that_differ_only_in_environment_or_collection_fetch_separately() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("AT")))
        .expect(4)
        .mount(&idp)
        .await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let base = request(target.uri(), cc(&format!("{}/token", idp.uri())));
    for env in ["dev", "prod"] {
        let mut r = base.clone();
        r.environment = Some(env.into());
        send(&engine, &rx, r).await;
    }
    for coll in ["c1", "c2"] {
        let mut r = base.clone();
        r.client.collection = Some(coll.into());
        send(&engine, &rx, r).await;
    }
}

#[tokio::test]
async fn auth_code_without_a_token_says_to_authorize_first() {
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let evs = send(
        &engine,
        &rx,
        request(target.uri(), code_auth(&target.uri(), 0, false)),
    )
    .await;
    assert_eq!(
        auth_error(&evs).message,
        "Not authorized: use Authorize first"
    );
}

/// Runs the authorize flow to its redirect URI and state.
async fn authorize(
    engine: &Engine,
    rx: &EventReceiver,
    auth: EngineAuth,
    timeout: Option<Duration>,
) -> (RequestId, reqwest::Url) {
    let id = engine.submit(Command::AuthorizeStart {
        auth,
        client: settings(None),
        environment: None,
        timeout,
    });
    match rx.recv().await.unwrap() {
        Event::AuthUrl { id: got, url } => {
            assert_eq!(got, id);
            (id, reqwest::Url::parse(&url).unwrap())
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn param(url: &reqwest::Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// A raw GET; returns the whole response text.
async fn raw_get(port: u16, target: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out).await;
    out
}

fn redirect_port(url: &reqwest::Url) -> u16 {
    reqwest::Url::parse(&param(url, "redirect_uri").unwrap())
        .unwrap()
        .port()
        .unwrap()
}

async fn next_auth_event(rx: &EventReceiver) -> Event {
    loop {
        let ev = rx.recv().await.unwrap();
        if matches!(ev, Event::AuthCompleted { .. } | Event::AuthFailed { .. }) {
            return ev;
        }
    }
}

async fn code_token_server(refresh: &str, expires_in: u64) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT-CODE",
            "token_type": "bearer",
            "expires_in": expires_in,
            "refresh_token": refresh,
        })))
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn an_expired_token_is_refreshed_and_a_rotated_refresh_token_replaces_the_old() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains("grant_type=authorization_code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT-1", "token_type": "bearer", "expires_in": 60, "refresh_token": "RT-1"
        })))
        .mount(&idp)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains("grant_type=refresh_token"))
        .and(wiremock::matchers::body_string_contains("refresh_token=RT-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT-2", "token_type": "bearer", "expires_in": 60, "refresh_token": "RT-2"
        })))
        .up_to_n_times(1)
        .mount(&idp)
        .await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains(
            "refresh_token=RT-2",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT-3", "token_type": "bearer", "expires_in": 60
        })))
        .mount(&idp)
        .await;
    let target = resource_server().await;
    let (engine, rx, clock) = engine();
    let auth = code_auth(&idp.uri(), 0, false);
    let (_, url) = authorize(&engine, &rx, auth.clone(), None).await;
    let port = redirect_port(&url);
    let state = param(&url, "state").unwrap();
    raw_get(port, &format!("/callback?code=c&state={state}")).await;
    assert!(matches!(
        next_auth_event(&rx).await,
        Event::AuthCompleted { .. }
    ));
    let req = request(target.uri(), auth);
    send(&engine, &rx, req.clone()).await;
    clock.advance(Duration::from_secs(120));
    send(&engine, &rx, req.clone()).await;
    clock.advance(Duration::from_secs(120));
    send(&engine, &rx, req).await;
    let bearers: Vec<String> = seen(&target, "/").await.into_iter().map(|r| r.0).collect();
    assert_eq!(bearers, ["Bearer AT-1", "Bearer AT-2", "Bearer AT-3"]);
}

#[tokio::test]
async fn a_redirect_to_another_origin_leaves_the_token_behind() {
    let idp = token_server("AT").await;
    let b = resource_server().await;
    let a = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", format!("{}/landed", b.uri())),
        )
        .mount(&a)
        .await;
    let (engine, rx, _) = engine();
    let evs = send(
        &engine,
        &rx,
        request(
            format!("{}/start", a.uri()),
            cc(&format!("{}/token", idp.uri())),
        ),
    )
    .await;
    assert_eq!(status_of(&evs), Some(200));
    assert_eq!(seen(&a, "/start").await[0].0, "Bearer AT");
    assert_eq!(seen(&b, "/landed").await[0].0, "");
}

#[tokio::test]
async fn a_same_origin_redirect_keeps_the_token() {
    let idp = token_server("AT").await;
    let a = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/landed"))
        .mount(&a)
        .await;
    Mock::given(method("GET"))
        .and(path("/landed"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&a)
        .await;
    let (engine, rx, _) = engine();
    send(
        &engine,
        &rx,
        request(
            format!("{}/start", a.uri()),
            cc(&format!("{}/token", idp.uri())),
        ),
    )
    .await;
    assert_eq!(seen(&a, "/landed").await[0].0, "Bearer AT");
}

// ---- authorize ----

#[tokio::test]
async fn the_auth_url_carries_the_pkce_parameters_and_keeps_the_existing_query() {
    let idp = code_token_server("RT", 3600).await;
    let (engine, rx, _) = engine();
    let (id, url) = authorize(&engine, &rx, code_auth(&idp.uri(), 0, false), None).await;
    assert_eq!(url.path(), "/authorize");
    assert_eq!(param(&url, "audience").as_deref(), Some("api"));
    assert_eq!(param(&url, "response_type").as_deref(), Some("code"));
    assert_eq!(param(&url, "client_id").as_deref(), Some("cid"));
    assert_eq!(param(&url, "scope").as_deref(), Some("read write"));
    assert_eq!(
        param(&url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert!(param(&url, "code_challenge").unwrap().len() >= 43);
    assert!(param(&url, "state").unwrap().len() >= 22);
    let port = redirect_port(&url);
    assert_eq!(
        param(&url, "redirect_uri").unwrap(),
        format!("http://127.0.0.1:{port}/callback")
    );
    engine.submit(Command::AuthorizeCancel(id));
    next_auth_event(&rx).await;
}

#[tokio::test]
async fn an_auth_url_that_is_not_https_or_loopback_fails_before_binding() {
    let (engine, rx, _) = engine();
    let mut auth = code_auth("https://idp.test", 0, false);
    if let EngineAuth::OAuth2AuthCode { auth_url, .. } = &mut auth {
        *auth_url = "http://idp.test/authorize".into();
    }
    engine.submit(Command::AuthorizeStart {
        auth,
        client: settings(None),
        environment: None,
        timeout: None,
    });
    let ev = next_auth_event(&rx).await;
    assert!(auth_error(&[ev]).message.contains("auth_url"));
}

#[tokio::test]
async fn the_listener_answers_404_to_other_requests_and_serves_a_static_page() {
    let idp = code_token_server("RT", 3600).await;
    let target = resource_server().await;
    let (engine, rx, _) = engine();
    let auth = code_auth(&idp.uri(), 0, false);
    let (_, url) = authorize(&engine, &rx, auth.clone(), None).await;
    let port = redirect_port(&url);
    assert!(
        raw_get(port, "/favicon.ico")
            .await
            .starts_with("HTTP/1.1 404")
    );
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"POST /callback?state=x HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out).await;
    assert!(out.starts_with("HTTP/1.1 404"), "{out}");
    // A head over 8 KiB is refused; the listener keeps going.
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let junk = format!("GET /callback HTTP/1.1\r\nX: {}", "a".repeat(9000));
    let _ = s.write_all(junk.as_bytes()).await;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out).await;
    assert!(out.is_empty() || out.starts_with("HTTP/1.1 404"), "{out}");
    // A connection that stays silent does not hold up a real callback.
    let _silent = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let state = param(&url, "state").unwrap();
    let page = raw_get(port, &format!("/callback?code=CODEVAL&state={state}")).await;
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(
        !page.contains("CODEVAL") && !page.contains(&state),
        "{page}"
    );
    assert!(matches!(
        next_auth_event(&rx).await,
        Event::AuthCompleted { .. }
    ));
    assert_eq!(
        status_of(&send(&engine, &rx, request(target.uri(), auth)).await),
        Some(200)
    );
}

#[tokio::test]
async fn a_matching_callback_exchanges_the_code_with_the_verifier_and_caches_the_token() {
    let idp = code_token_server("RT", 3600).await;
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("authorization", "Bearer AT-CODE"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&target)
        .await;
    let (engine, rx, _) = engine();
    let auth = code_auth(&idp.uri(), 0, false);
    let (_, url) = authorize(&engine, &rx, auth.clone(), None).await;
    let state = param(&url, "state").unwrap();
    let port = redirect_port(&url);
    raw_get(port, &format!("/callback?code=CODEVAL&state={state}")).await;
    assert!(matches!(
        next_auth_event(&rx).await,
        Event::AuthCompleted { .. }
    ));
    let body = seen(&idp, "/token").await[0].1.clone();
    assert!(body.contains("grant_type=authorization_code"), "{body}");
    assert!(body.contains("code=CODEVAL"));
    assert!(body.contains(&format!(
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Fcallback"
    )));
    let verifier = body
        .split('&')
        .find_map(|p| p.strip_prefix("code_verifier="))
        .unwrap();
    assert!((43..=128).contains(&verifier.len()));
    // The challenge in the URL is the S256 of that verifier.
    use sha2::Digest as _;
    let want = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    assert_eq!(param(&url, "code_challenge").unwrap(), want);
    send(&engine, &rx, request(target.uri(), auth)).await;
}

#[tokio::test]
async fn a_wrong_state_or_an_error_parameter_fails_without_an_exchange() {
    for query in [
        "code=c&state=WRONG",
        "error=access_denied&state=WRONG",
        "code=c",
    ] {
        let idp = code_token_server("RT", 3600).await;
        let (engine, rx, _) = engine();
        let (_, url) = authorize(&engine, &rx, code_auth(&idp.uri(), 0, false), None).await;
        raw_get(redirect_port(&url), &format!("/callback?{query}")).await;
        let ev = next_auth_event(&rx).await;
        let e = auth_error(&[ev]);
        assert!(!e.message.contains("WRONG"), "{e:?}");
        assert!(seen(&idp, "/token").await.is_empty(), "{query}");
    }
    // A genuine error with the right state names the code and exchanges nothing.
    let idp = code_token_server("RT", 3600).await;
    let (engine, rx, _) = engine();
    let (_, url) = authorize(&engine, &rx, code_auth(&idp.uri(), 0, false), None).await;
    raw_get(redirect_port(&url), "/callback?error=access_denied").await;
    let e = auth_error(&[next_auth_event(&rx).await]);
    assert!(e.message.contains("access_denied"), "{e:?}");
    assert!(seen(&idp, "/token").await.is_empty());
}

#[tokio::test]
async fn timeout_cancel_and_port_in_use_end_in_auth_failed_and_close_the_port() {
    let idp = code_token_server("RT", 3600).await;
    let (engine, rx, _) = engine();
    // Timeout.
    let (_, url) = authorize(
        &engine,
        &rx,
        code_auth(&idp.uri(), 0, false),
        Some(Duration::from_millis(200)),
    )
    .await;
    let port = redirect_port(&url);
    let e = auth_error(&[next_auth_event(&rx).await]);
    assert!(e.message.contains("timed out"), "{e:?}");
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    // Cancel.
    let (id, url) = authorize(&engine, &rx, code_auth(&idp.uri(), 0, false), None).await;
    let port = redirect_port(&url);
    engine.submit(Command::AuthorizeCancel(id));
    let e = auth_error(&[next_auth_event(&rx).await]);
    assert_eq!(e.message, "cancelled");
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    // Port in use.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = taken.local_addr().unwrap().port();
    engine.submit(Command::AuthorizeStart {
        auth: code_auth(&idp.uri(), busy, false),
        client: settings(None),
        environment: None,
        timeout: None,
    });
    assert!(
        auth_error(&[next_auth_event(&rx).await])
            .message
            .contains("cannot listen")
    );
}

#[tokio::test]
async fn refresh_tokens_reach_the_store_only_when_persist_is_set() {
    for persist in [false, true] {
        let idp = code_token_server("RT-SAVED", 3600).await;
        let store = Arc::new(Recording::default());
        let (engine, rx, _) = engine_with(store.clone());
        let (_, url) = authorize(&engine, &rx, code_auth(&idp.uri(), 0, persist), None).await;
        let state = param(&url, "state").unwrap();
        raw_get(
            redirect_port(&url),
            &format!("/callback?code=c&state={state}"),
        )
        .await;
        assert!(matches!(
            next_auth_event(&rx).await,
            Event::AuthCompleted { .. }
        ));
        let saves = store.saves.lock().unwrap();
        if persist {
            assert_eq!(saves.len(), 1);
            assert_eq!(saves[0].1, "RT-SAVED");
            assert_eq!(saves[0].0.grant, Grant::AuthCode);
            assert_eq!(saves[0].0.client_id, "cid");
            assert_eq!(saves[0].0.token_url, format!("{}/token", idp.uri()));
            assert_eq!(saves[0].0.scope, "read write");
        } else {
            assert!(saves.is_empty());
            assert!(store.loads.lock().unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn a_stored_refresh_token_is_loaded_and_used_after_a_restart() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains(
            "refresh_token=RT-OLD",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_body("AT-NEW")))
        .expect(1)
        .mount(&idp)
        .await;
    let target = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("authorization", "Bearer AT-NEW"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&target)
        .await;
    let store = Arc::new(Recording::default());
    let auth = code_auth(&idp.uri(), 0, true);
    let key = TokenKey {
        grant: Grant::AuthCode,
        token_url: format!("{}/token", idp.uri()),
        client_id: "cid".into(),
        scope: "read write".into(),
        collection: None,
        environment: None,
    };
    store.inner.save(&key, "RT-OLD").unwrap();
    let (engine, rx, _) = engine_with(store);
    send(&engine, &rx, request(target.uri(), auth)).await;
}

#[tokio::test]
async fn a_failing_store_load_fails_the_send_without_the_error_holding_a_token() {
    let idp = token_server("AT").await;
    let target = resource_server().await;
    let store = Arc::new(Recording {
        fail_load: true,
        ..Recording::default()
    });
    let (engine, rx, _) = engine_with(store);
    let evs = send(
        &engine,
        &rx,
        request(target.uri(), code_auth(&idp.uri(), 0, true)),
    )
    .await;
    assert_eq!(
        auth_error(&evs).message,
        "cannot read stored refresh token: keychain locked"
    );
}

#[tokio::test]
async fn a_failing_store_save_keeps_the_tokens_and_still_completes() {
    let idp = code_token_server("RT", 3600).await;
    let target = resource_server().await;
    let store = Arc::new(Recording {
        fail_save: true,
        ..Recording::default()
    });
    let (engine, rx, _) = engine_with(store.clone());
    let auth = code_auth(&idp.uri(), 0, true);
    let (_, url) = authorize(&engine, &rx, auth.clone(), None).await;
    let state = param(&url, "state").unwrap();
    raw_get(
        redirect_port(&url),
        &format!("/callback?code=c&state={state}"),
    )
    .await;
    assert!(matches!(
        next_auth_event(&rx).await,
        Event::AuthCompleted { .. }
    ));
    assert_eq!(store.saves.lock().unwrap().len(), 1);
    assert_eq!(
        status_of(&send(&engine, &rx, request(target.uri(), auth)).await),
        Some(200)
    );
}

#[tokio::test]
async fn no_event_or_error_holds_a_secret() {
    let idp = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"error":"invalid_grant","refresh_token":"RTSECRETMARK","access_token":"ATSECRETMARK"}"#,
        ))
        .mount(&idp)
        .await;
    let (engine, rx, _) = engine();
    let mut auth = code_auth(&idp.uri(), 0, false);
    if let EngineAuth::OAuth2AuthCode { client_secret, .. } = &mut auth {
        *client_secret = "CSECRETMARK".into();
    }
    let (_, url) = authorize(&engine, &rx, auth, None).await;
    let state = param(&url, "state").unwrap();
    raw_get(
        redirect_port(&url),
        &format!("/callback?code=CODEMARK&state={state}"),
    )
    .await;
    let ev = next_auth_event(&rx).await;
    let text = format!("{ev:?}");
    for marker in [
        "RTSECRETMARK",
        "ATSECRETMARK",
        "CSECRETMARK",
        "CODEMARK",
        &state,
    ] {
        assert!(!text.contains(marker), "{text}");
    }
    assert_eq!(auth_error(&[ev]).kind, ErrorKind::Auth);
}
