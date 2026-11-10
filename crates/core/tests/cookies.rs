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

fn with_header(mut req: PreparedRequest, name: &str, value: &str) -> PreparedRequest {
    req.headers.push(Header {
        name: name.into(),
        value: value.into(),
    });
    req
}

async fn send_req(engine: &Engine, rx: &EventReceiver, req: PreparedRequest) {
    engine.submit(Command::Send(req));
    loop {
        match rx.recv().await.unwrap() {
            Event::Completed { .. } => return,
            Event::Failed { .. } | Event::Cancelled { .. } => panic!("request did not complete"),
            _ => {}
        }
    }
}

fn digest_auth(mut req: PreparedRequest) -> PreparedRequest {
    req.auth = Some(wisp_core::auth::EngineAuth::Digest {
        username: "u".into(),
        password: "p".into(),
    });
    req
}

fn set_many(jar: &CookieJar, from: &str, headers: Vec<String>) {
    use reqwest::cookie::CookieStore;
    let values: Vec<_> = headers
        .iter()
        .map(|h| reqwest::header::HeaderValue::from_str(h).unwrap())
        .collect();
    jar.set_cookies(&mut values.iter(), &reqwest::Url::parse(from).unwrap());
}

fn big_snapshot(jar: &CookieJar) -> Vec<CookieRecord> {
    jar.snapshot(SystemTime::now())
}

#[test]
fn many_cookies_for_one_domain_stay_within_fifty() {
    let jar = CookieJar::default();
    set_many(
        &jar,
        "https://example.com/",
        (0..2000).map(|i| format!("c{i}=v; Path=/")).collect(),
    );
    let snap = big_snapshot(&jar);
    assert_eq!(snap.len(), 50);
    // The newest survive.
    assert!(snap.iter().any(|c| c.name == "c1999"));
    assert!(!snap.iter().any(|c| c.name == "c0"));
}

#[test]
fn many_domains_stay_within_the_jar_cap() {
    let jar = CookieJar::default();
    jar.load(
        (0..4000)
            .map(|i| record("a", "1", &format!("h{i}.example.com")))
            .collect(),
    );
    assert_eq!(big_snapshot(&jar).len(), 3000);
}

#[test]
fn an_oversized_cookie_is_dropped_and_the_limit_itself_is_kept() {
    let jar = CookieJar::default();
    let big = "v".repeat(4096);
    set(&jar, "https://example.com/", &format!("big={big}"));
    assert!(big_snapshot(&jar).is_empty());
    let ok = "v".repeat(4096 - 2);
    set(&jar, "https://example.com/", &format!("ok={ok}"));
    assert_eq!(big_snapshot(&jar).len(), 1);
}

#[test]
fn max_age_zero_deletes_the_cookie() {
    let jar = CookieJar::default();
    set(&jar, "https://example.com/", "sid=1; Path=/");
    assert_eq!(big_snapshot(&jar).len(), 1);
    set(&jar, "https://example.com/", "sid=; Path=/; Max-Age=0");
    assert!(big_snapshot(&jar).is_empty());
    assert!(!sent_to(&jar, "https://example.com/"));
}

#[test]
fn load_rejects_a_suffix_domain_record_and_an_unparseable_one() {
    let jar = CookieJar::default();
    let mut suffix = record("a", "1", "co.uk");
    suffix.host_only = false;
    let mut private = record("b", "1", "github.io");
    private.host_only = false;
    let mut junk = record("c", "1", "bad host!");
    junk.host_only = false;
    let mut ok = record("d", "1", "example.com");
    ok.host_only = false;
    jar.load(vec![suffix, private, junk, ok]);
    let snap = big_snapshot(&jar);
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].name, "d");
}

#[test]
fn a_private_section_suffix_is_rejected_as_a_cookie_domain() {
    let jar = CookieJar::default();
    set(&jar, "https://evil.github.io/", "x=1; Domain=github.io");
    assert!(!sent_to(&jar, "https://other.github.io/"));
}

async fn login_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("set-cookie", "session=abc; Path=/")
                .insert_header("location", "/home"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/home"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

fn post_login(server: &MockServer) -> PreparedRequest {
    let mut req = get(format!("{}/login", server.uri()), None);
    req.method = Method::Post;
    req
}

#[tokio::test]
async fn a_302_login_sets_the_cookie_for_the_redirect_target() {
    let server = login_server().await;
    let jars = CookieJars::default();
    let (engine, rx) = engine_with(&jars);
    send_req(&engine, &rx, post_login(&server)).await;
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("session=abc")
    );
}

#[tokio::test]
async fn a_302_login_sets_the_cookie_on_the_engines_own_redirect_path() {
    let server = login_server().await;
    let jars = CookieJars::default();
    let (engine, rx) = engine_with(&jars);
    send_req(&engine, &rx, digest_auth(post_login(&server))).await;
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("session=abc")
    );
}

#[tokio::test]
async fn a_user_cookie_header_and_the_jar_cookie_both_arrive() {
    let server = server_setting("sid=JAR; Path=/").await;
    let jars = CookieJars::default();
    let (engine, rx) = engine_with(&jars);
    send(&engine, &rx, format!("{}/set", server.uri()), None).await;
    let req = with_header(get(format!("{}/echo", server.uri()), None), "Cookie", "a=1");
    send_req(&engine, &rx, req).await;
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("a=1; sid=JAR")
    );
}

#[tokio::test]
async fn on_a_name_clash_the_user_value_wins() {
    let server = server_setting("sid=JAR; Path=/").await;
    let jars = CookieJars::default();
    let (engine, rx) = engine_with(&jars);
    send(&engine, &rx, format!("{}/set", server.uri()), None).await;
    let req = with_header(
        get(format!("{}/echo", server.uri()), None),
        "Cookie",
        "sid=USER; a=1",
    );
    send_req(&engine, &rx, req).await;
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("sid=USER; a=1")
    );
}

#[tokio::test]
async fn the_merge_holds_on_an_own_redirect_hop() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("set-cookie", "sid=JAR; Path=/")
                .insert_header("location", "/end"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/end"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let jars = CookieJars::default();
    let (engine, rx) = engine_with(&jars);
    let req = digest_auth(with_header(
        get(format!("{}/start", server.uri()), None),
        "Cookie",
        "a=1",
    ));
    send_req(&engine, &rx, req).await;
    assert_eq!(cookie_header(&server, 0).await.as_deref(), Some("a=1"));
    assert_eq!(
        cookie_header(&server, 1).await.as_deref(),
        Some("a=1; sid=JAR")
    );
}

#[test]
fn response_cookies_across_many_domains_stay_within_the_jar_cap() {
    let jar = CookieJar::default();
    for i in 0..3100 {
        set(&jar, &format!("https://h{i}.example.com/"), "a=1; Path=/");
    }
    assert_eq!(big_snapshot(&jar).len(), 3000);
}
