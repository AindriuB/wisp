use std::time::{Duration, SystemTime};

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wisp_core::cookies::{CookieJar, CookieJars, CookieRecord};
use wisp_core::*;

fn get(url: String, collection: Option<&str>) -> PreparedRequest {
    PreparedRequest {
        method: Method::Get,
        url,
        headers: vec![],
        body: PreparedBody::None,
        timeout: None,
        client: ClientSettings {
            verify_tls: true,
            follow_redirects: true,
            proxy: None,
            collection: collection.map(str::to_owned),
        },
        warnings: vec![],
        auth: None,
        environment: None,
    }
}

async fn send(engine: &Engine, rx: &EventReceiver, url: String, collection: Option<&str>) {
    engine.submit(Command::Send(get(url, collection)));
    loop {
        match rx.recv().await.unwrap() {
            Event::Completed { .. } => return,
            Event::Failed { .. } | Event::Cancelled { .. } => panic!("request did not complete"),
            _ => {}
        }
    }
}

/// The `Cookie` header of the n-th request the server saw.
async fn cookie_header(server: &MockServer, n: usize) -> Option<String> {
    let reqs = server.received_requests().await.unwrap();
    reqs[n]
        .headers
        .get("cookie")
        .map(|v| v.to_str().unwrap().to_owned())
}

async fn server_setting(set_cookie: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/set"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", set_cookie))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

fn engine_with(jars: &CookieJars) -> (Engine, EventReceiver) {
    Engine::start(EngineConfig {
        cookies: jars.clone(),
        ..Default::default()
    })
}

fn record(name: &str, value: &str, domain: &str) -> CookieRecord {
    CookieRecord {
        name: name.into(),
        value: value.into(),
        domain: domain.into(),
        host_only: true,
        path: "/".into(),
        secure: false,
        http_only: false,
        expires: None,
    }
}

#[tokio::test]
async fn a_set_cookie_is_sent_on_the_next_request_to_the_same_origin() {
    let server = server_setting("sid=SECRETVAL; Path=/").await;
    let (engine, rx) = engine_with(&CookieJars::default());
    send(&engine, &rx, format!("{}/set", server.uri()), None).await;
    send(&engine, &rx, format!("{}/echo", server.uri()), None).await;
    assert_eq!(cookie_header(&server, 0).await, None);
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("sid=SECRETVAL")
    );
}

#[tokio::test]
async fn a_cookie_is_not_sent_to_a_different_host() {
    let server = server_setting("sid=SECRETVAL; Path=/").await;
    let (engine, rx) = engine_with(&CookieJars::default());
    send(&engine, &rx, format!("{}/set", server.uri()), None).await;
    let other = server.uri().replace("127.0.0.1", "localhost");
    send(&engine, &rx, format!("{other}/echo"), None).await;
    assert_eq!(cookie_header(&server, 1).await, None);
}

/// cookie_store treats loopback hosts as secure contexts (as browsers do), and wiremock only
/// binds loopback, so the plain-http rule is checked on the provider with a non-loopback host.
#[test]
fn a_secure_cookie_is_not_sent_over_http() {
    use reqwest::cookie::CookieStore;
    let jar = CookieJar::default();
    let https = reqwest::Url::parse("https://example.test/").unwrap();
    let http = reqwest::Url::parse("http://example.test/").unwrap();
    let set = reqwest::header::HeaderValue::from_static("sid=SECRETVAL; Path=/; Secure");
    jar.set_cookies(&mut std::iter::once(&set), &https);
    assert!(jar.cookies(&http).is_none());
    assert_eq!(jar.cookies(&https).unwrap(), "sid=SECRETVAL");
}

#[tokio::test]
async fn snapshot_returns_unexpired_cookies_with_session_ones_as_none() {
    let jars = CookieJars::default();
    let jar = jars.jar(None);
    let now = SystemTime::now();
    let mut live = record("live", "1", "example.com");
    live.expires = Some(now + Duration::from_secs(3600));
    jar.load(vec![record("sess", "2", "example.com"), live]);
    let mut snap = jar.snapshot(now);
    snap.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].name, "live");
    assert!(snap[0].expires.is_some());
    assert_eq!(snap[1].name, "sess");
    assert_eq!(snap[1].expires, None);
    // Past its expiry the persistent cookie is gone; the session one stays.
    let later = jar.snapshot(now + Duration::from_secs(7200));
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].name, "sess");
}

#[tokio::test]
async fn load_replaces_contents_and_a_loaded_cookie_is_sent() {
    let server = server_setting("a=b").await;
    let jars = CookieJars::default();
    let jar = jars.jar(None);
    jar.load(vec![record("old", "1", "127.0.0.1")]);
    jar.load(vec![record("loaded", "LOADEDVAL", "127.0.0.1")]);
    assert_eq!(jar.snapshot(SystemTime::now()).len(), 1);
    let (engine, rx) = engine_with(&jars);
    send(&engine, &rx, format!("{}/echo", server.uri()), None).await;
    assert_eq!(
        cookie_header(&server, 0).await.as_deref(),
        Some("loaded=LOADEDVAL")
    );
}

#[test]
fn load_drops_expired_records() {
    let jar = CookieJar::default();
    let mut gone = record("gone", "1", "example.com");
    gone.expires = Some(SystemTime::now() - Duration::from_secs(10));
    jar.load(vec![gone]);
    assert!(jar.snapshot(SystemTime::now()).is_empty());
}

#[test]
fn remove_deletes_one_cookie_and_clear_deletes_all() {
    let jar = CookieJar::default();
    jar.load(vec![
        record("a", "1", "example.com"),
        record("b", "2", "example.com"),
    ]);
    assert!(jar.remove("example.com", "/", "a"));
    assert!(!jar.remove("example.com", "/", "a"));
    let snap = jar.snapshot(SystemTime::now());
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].name, "b");
    jar.clear();
    assert!(jar.snapshot(SystemTime::now()).is_empty());
}

#[tokio::test]
async fn generation_increases_on_every_change() {
    let server = server_setting("sid=1; Path=/").await;
    let jars = CookieJars::default();
    let jar = jars.jar(None);
    let g0 = jar.generation();
    jar.load(vec![record("a", "1", "example.com")]);
    let g1 = jar.generation();
    assert!(g1 > g0);
    assert!(jar.remove("example.com", "/", "a"));
    let g2 = jar.generation();
    assert!(g2 > g1);
    jar.clear();
    let g3 = jar.generation();
    assert!(g3 > g2);
    let (engine, rx) = engine_with(&jars);
    send(&engine, &rx, format!("{}/set", server.uri()), None).await;
    assert!(jar.generation() > g3);
}

#[tokio::test]
async fn cookies_are_kept_per_collection_through_the_engine() {
    let server = server_setting("sid=FROMA; Path=/").await;
    let jars = CookieJars::default();
    let (engine, rx) = engine_with(&jars);
    send(&engine, &rx, format!("{}/set", server.uri()), Some("a")).await;
    let now = SystemTime::now();
    assert_eq!(jars.jar(Some("a")).snapshot(now).len(), 1);
    assert!(jars.jar(Some("b")).snapshot(now).is_empty());
    assert!(jars.jar(None).snapshot(now).is_empty());

    jars.jar(Some("b"))
        .load(vec![record("bonly", "BVAL", "127.0.0.1")]);
    send(&engine, &rx, format!("{}/echo", server.uri()), Some("b")).await;
    send(&engine, &rx, format!("{}/echo", server.uri()), Some("a")).await;
    send(&engine, &rx, format!("{}/echo", server.uri()), None).await;
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("bonly=BVAL")
    );
    assert_eq!(
        cookie_header(&server, 2).await.as_deref(),
        Some("sid=FROMA")
    );
    assert_eq!(cookie_header(&server, 3).await, None);
}

#[test]
fn same_key_returns_the_same_jar() {
    let jars = CookieJars::default();
    jars.jar(Some("a"))
        .load(vec![record("x", "1", "example.com")]);
    assert_eq!(jars.jar(Some("a")).snapshot(SystemTime::now()).len(), 1);
    assert!(jars.jar(Some("b")).snapshot(SystemTime::now()).is_empty());
}

fn loaded_jars() -> CookieJars {
    let jars = CookieJars::default();
    jars.jar(Some("a"))
        .load(vec![record("sid", "SECRETVAL", "example.com")]);
    jars
}

#[test]
fn debug_redacts_cookie_record() {
    let r = record("sid", "SECRETVAL", "example.com");
    for s in [format!("{r:?}"), format!("{r:#?}")] {
        assert!(!s.contains("SECRETVAL"), "{s}");
        assert!(s.contains("<redacted>"));
    }
}

#[test]
fn debug_redacts_cookie_jar() {
    let jar = loaded_jars().jar(Some("a"));
    for s in [format!("{jar:?}"), format!("{jar:#?}")] {
        assert!(!s.contains("SECRETVAL"), "{s}");
        assert!(s.contains("<redacted>"));
    }
}

#[test]
fn debug_redacts_cookie_jars() {
    let jars = loaded_jars();
    let cfg = EngineConfig {
        cookies: jars.clone(),
        ..Default::default()
    };
    for s in [
        format!("{jars:?}"),
        format!("{jars:#?}"),
        format!("{cfg:?}"),
        format!("{cfg:#?}"),
    ] {
        assert!(!s.contains("SECRETVAL"), "{s}");
        assert!(s.contains("<redacted>"));
    }
}

fn set(jar: &CookieJar, from: &str, header: &str) {
    use reqwest::cookie::CookieStore;
    let v = reqwest::header::HeaderValue::from_str(header).unwrap();
    jar.set_cookies(
        &mut std::iter::once(&v),
        &reqwest::Url::parse(from).unwrap(),
    );
}

fn sent_to(jar: &CookieJar, url: &str) -> bool {
    use reqwest::cookie::CookieStore;
    jar.cookies(&reqwest::Url::parse(url).unwrap()).is_some()
}

#[test]
fn a_domain_attribute_that_is_a_public_suffix_is_rejected() {
    let jar = CookieJar::default();
    set(&jar, "https://evil.com/", "x=1; Domain=com");
    set(&jar, "https://evil.co.uk/", "x=1; Domain=co.uk");
    assert!(jar.snapshot(SystemTime::now()).is_empty());
    assert!(!sent_to(&jar, "https://api.bank.com/"));
    assert!(!sent_to(&jar, "https://bank.co.uk/"));
}

#[test]
fn a_domain_attribute_for_a_registrable_domain_still_works() {
    let jar = CookieJar::default();
    set(&jar, "https://www.example.com/", "x=1; Domain=example.com");
    assert!(sent_to(&jar, "https://api.example.com/"));
}
