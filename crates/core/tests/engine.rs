use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use wiremock::matchers::{body_bytes, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wisp_core::resolve::ResolveWarning;
use wisp_core::*;

fn get(url: String) -> PreparedRequest {
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
            collection: None,
        },
        warnings: vec![],
        auth: None,
        environment: None,
    }
}

async fn collect_until_terminal(rx: &EventReceiver) -> Vec<Event> {
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

#[tokio::test]
async fn a_200_yields_started_headers_completed_in_order_with_readable_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    let id = engine.submit(Command::Send(get(format!("{}/x", server.uri()))));
    let evs = collect_until_terminal(&rx).await;
    assert!(matches!(&evs[0], Event::Started { id: i, .. } if *i == id));
    match &evs[1] {
        Event::Headers {
            id: i,
            status,
            reason,
            version,
            ..
        } => {
            assert_eq!(*i, id);
            assert_eq!(*status, 200);
            assert_eq!(reason.as_deref(), Some("OK"));
            assert_eq!(*version, HttpVersion::Http11);
        }
        other => panic!("unexpected {other:?}"),
    }
    match evs.last().unwrap() {
        Event::Completed {
            id: got,
            size,
            body,
            ..
        } => {
            assert_eq!(*got, id);
            assert_eq!(*size, 5);
            assert_eq!(engine.bodies().read(body, 0, 100).unwrap(), b"hello");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(
        evs[2..evs.len() - 1]
            .iter()
            .all(|e| matches!(e, Event::Progress { .. }))
    );
    assert!(rx.is_empty());
}

#[tokio::test]
async fn a_large_body_over_the_memory_limit_spills_and_reads_back() {
    let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 251) as u8).collect();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig {
        body_memory_limit: 64 * 1024,
        ..EngineConfig::default()
    });
    engine.submit(Command::Send(get(server.uri())));
    let evs = collect_until_terminal(&rx).await;
    let Event::Completed { body, size, .. } = evs.last().unwrap() else {
        panic!("unexpected {evs:?}");
    };
    assert_eq!(*size, 1_048_576);
    let store = engine.bodies();
    assert!(store.is_spilled(body));
    assert_eq!(store.read(body, 0, 4).unwrap(), data[0..4]);
    assert_eq!(
        store.read(body, 1_000_000, 4).unwrap(),
        data[1_000_000..1_000_004]
    );
}

#[tokio::test]
async fn a_missing_body_file_fails_with_body_file_naming_the_path() {
    let (engine, rx) = Engine::start(EngineConfig::default());
    let mut req = get("http://127.0.0.1:1/".into());
    req.body = PreparedBody::File("/definitely/missing.bin".into());
    engine.submit(Command::Send(req));
    let evs = collect_until_terminal(&rx).await;
    match evs.last().unwrap() {
        Event::Failed { error, .. } => {
            assert_eq!(error.kind, ErrorKind::BodyFile);
            assert!(error.message.contains("/definitely/missing.bin"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_multipart_body_reaches_the_server_with_boundary_and_both_parts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let file = std::env::temp_dir().join(format!("wisp-mp-{}.txt", std::process::id()));
    std::fs::write(&file, b"file-contents").unwrap();
    let (engine, rx) = Engine::start(EngineConfig::default());
    let mut req = get(server.uri());
    req.method = Method::Post;
    req.body = PreparedBody::Multipart(vec![
        PreparedPart {
            name: "alpha".into(),
            value: PreparedPartValue::Text("one".into()),
        },
        PreparedPart {
            name: "beta".into(),
            value: PreparedPartValue::File(file.clone()),
        },
    ]);
    engine.submit(Command::Send(req));
    let evs = collect_until_terminal(&rx).await;
    assert!(matches!(evs.last().unwrap(), Event::Completed { .. }));
    let got = &server.received_requests().await.unwrap()[0];
    let ct = got.headers.get("content-type").unwrap().to_str().unwrap();
    assert!(ct.starts_with("multipart/form-data; boundary="));
    let body = String::from_utf8_lossy(&got.body);
    assert!(body.contains("name=\"alpha\""));
    assert!(body.contains("name=\"beta\""));
    assert!(body.contains("file-contents"));
    let _ = std::fs::remove_file(file);
}

#[tokio::test]
async fn follow_redirects_false_surfaces_the_302() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    let mut req = get(server.uri());
    req.client.follow_redirects = false;
    engine.submit(Command::Send(req));
    let evs = collect_until_terminal(&rx).await;
    assert!(matches!(&evs[1], Event::Headers { status: 302, .. }));
}

#[tokio::test]
async fn a_post_sends_the_given_headers_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("x-test", "yes"))
        .and(body_bytes(b"payload".to_vec()))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(PreparedRequest {
        method: Method::Post,
        url: server.uri(),
        headers: vec![Header {
            name: "x-test".into(),
            value: "yes".into(),
        }],
        body: PreparedBody::Bytes(b"payload".to_vec()),
        timeout: None,
        client: ClientSettings {
            verify_tls: true,
            follow_redirects: true,
            proxy: None,
            collection: None,
        },
        warnings: vec![],
        auth: None,
        environment: None,
    }));
    let evs = collect_until_terminal(&rx).await;
    assert!(matches!(&evs[1], Event::Headers { status: 201, .. }));
}

async fn failure(req: PreparedRequest) -> ErrorKind {
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(req));
    match collect_until_terminal(&rx).await.pop().unwrap() {
        Event::Failed { error, .. } => error.kind,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_request_to_a_port_with_nothing_listening_fails_with_connect() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    assert_eq!(
        failure(get(format!("http://127.0.0.1:{port}/"))).await,
        ErrorKind::Connect
    );
}

#[tokio::test]
async fn a_response_delayed_past_the_timeout_fails_with_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    let mut req = get(server.uri());
    req.timeout = Some(Duration::from_millis(100));
    assert_eq!(failure(req).await, ErrorKind::Timeout);
}

#[tokio::test]
async fn an_unparsable_url_fails_with_invalid_url() {
    assert_eq!(
        failure(get("not a url".into())).await,
        ErrorKind::InvalidUrl
    );
}

#[tokio::test]
async fn cancel_on_an_in_flight_request_emits_cancelled_and_nothing_else_terminal() {
    let slow = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&slow)
        .await;
    let fast = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(800)))
        .mount(&fast)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    let id = engine.submit(Command::Send(get(slow.uri())));
    let sentinel = engine.submit(Command::Send(get(fast.uri())));
    assert_eq!(engine.submit(Command::Cancel(id)), id);
    let mut cancelled = false;
    loop {
        match rx.recv().await.unwrap() {
            Event::Cancelled { id: got } => {
                assert_eq!(got, id);
                assert!(!cancelled);
                cancelled = true;
            }
            Event::Completed { id: got, .. } if got == sentinel => break,
            Event::Completed { id: got, .. } | Event::Failed { id: got, .. } => {
                panic!("terminal for {got:?} after cancel")
            }
            _ => {}
        }
    }
    assert!(cancelled, "Cancelled arrives before the sentinel completes");
    // Cancelling a finished id emits nothing.
    engine.submit(Command::Cancel(sentinel));
    let next = engine.submit(Command::Send(get(fast.uri())));
    match rx.recv().await.unwrap() {
        Event::Started { id: got, .. } => assert_eq!(got, next),
        other => panic!("expected Started for the next request, got {other:?}"),
    }
}

#[tokio::test]
async fn events_are_received_from_a_plain_thread_with_recv_blocking() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(get(server.uri())));
    let ev = std::thread::spawn(move || rx.recv_blocking().unwrap())
        .join()
        .unwrap();
    assert!(matches!(ev, Event::Started { .. }));
}

#[tokio::test]
async fn dropping_the_engine_inside_a_tokio_runtime_does_not_panic() {
    let (engine, _rx) = Engine::start(EngineConfig::default());
    let clone = engine.clone();
    drop(engine);
    drop(clone);
}

#[test]
fn ids_are_monotonic() {
    let (engine, _rx) = Engine::start(EngineConfig {
        worker_threads: 99,
        default_timeout: None,
        ..EngineConfig::default()
    });
    let a = engine.submit(Command::Send(get("nope".into())));
    let b = engine.submit(Command::Send(get("nope".into())));
    assert!(b > a);
    assert_eq!(EngineConfig::default().worker_threads, 1);
}

#[tokio::test]
async fn words_in_the_url_do_not_change_the_error_kind() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    assert_eq!(
        failure(get(format!("http://127.0.0.1:{port}/tls"))).await,
        ErrorKind::Connect
    );
}

/// Serves a response with exactly `n` headers in total (Content-Length and Connection count).
fn serve_headers(n: usize) -> u16 {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let mut r =
                String::from("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n");
            for i in 0..n.saturating_sub(2) {
                r.push_str(&format!("X-H-{i}: v\r\n"));
            }
            r.push_str("\r\n");
            let _ = s.write_all(r.as_bytes());
        }
    });
    port
}

async fn terminal(url: String) -> Event {
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(get(url)));
    collect_until_terminal(&rx).await.pop().unwrap()
}

#[tokio::test]
async fn a_response_with_1001_headers_fails_naming_the_limit() {
    let port = serve_headers(1001);
    match terminal(format!("http://127.0.0.1:{port}/")).await {
        Event::Failed { error, .. } => {
            assert_eq!(error.kind, ErrorKind::Other);
            assert!(
                error
                    .message
                    .starts_with("response headers too large (limit 1000 headers"),
                "{}",
                error.message
            );
            assert!(!error.message.contains("client error"), "{}", error.message);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_response_with_150_headers_completes() {
    let port = serve_headers(150);
    let ev = terminal(format!("http://127.0.0.1:{port}/")).await;
    assert!(matches!(ev, Event::Completed { .. }), "{ev:?}");
}

#[tokio::test]
async fn a_response_with_1000_headers_completes() {
    let port = serve_headers(1000);
    let ev = terminal(format!("http://127.0.0.1:{port}/")).await;
    assert!(matches!(ev, Event::Completed { .. }), "{ev:?}");
}

#[tokio::test]
async fn a_response_with_90_headers_still_completes() {
    let port = serve_headers(90);
    let ev = terminal(format!("http://127.0.0.1:{port}/")).await;
    assert!(matches!(ev, Event::Completed { .. }), "{ev:?}");
}

#[tokio::test]
async fn a_bad_header_name_is_not_reported_as_an_invalid_url() {
    let mut req = get("http://127.0.0.1:1/".into());
    req.headers.push(Header {
        name: "bad name".into(),
        value: "v".into(),
    });
    assert_eq!(failure(req).await, ErrorKind::Other);
}

fn round_trip<T: Serialize + DeserializeOwned + std::fmt::Debug + PartialEq>(v: T) {
    let json = serde_json::to_string(&v).unwrap();
    assert_eq!(serde_json::from_str::<T>(&json).unwrap(), v);
}

#[test]
fn commands_and_events_round_trip_through_serde_json() {
    round_trip(Command::Send(PreparedRequest {
        method: Method::Post,
        url: "http://x/".into(),
        headers: vec![Header {
            name: "a".into(),
            value: "b".into(),
        }],
        body: PreparedBody::Multipart(vec![
            PreparedPart {
                name: "a".into(),
                value: PreparedPartValue::Text("t".into()),
            },
            PreparedPart {
                name: "f".into(),
                value: PreparedPartValue::File("/x/y".into()),
            },
        ]),
        timeout: Some(Duration::from_millis(1500)),
        client: ClientSettings {
            verify_tls: false,
            follow_redirects: true,
            proxy: Some("http://p".into()),
            collection: None,
        },
        warnings: vec![ResolveWarning::PatternMismatch {
            name: "n".into(),
            pattern: "p".into(),
            value: "v".into(),
        }],
        auth: Some(wisp_core::auth::EngineAuth::Digest {
            username: "u".into(),
            password: "p".into(),
        }),
        environment: Some("dev".into()),
    }));
    round_trip(Command::Cancel(RequestId(7)));
    let id = RequestId(3);
    let events = vec![
        Event::Started {
            id,
            at: std::time::UNIX_EPOCH + Duration::from_secs(5),
        },
        Event::Headers {
            id,
            status: 200,
            reason: Some("OK".into()),
            version: HttpVersion::Http2,
            headers: vec![Header {
                name: "a".into(),
                value: "b".into(),
            }],
            ttfb: Duration::from_millis(4),
        },
        Event::Progress { id, bytes: 10 },
        Event::Completed {
            id,
            total: Duration::from_millis(12),
            size: 1,
            body: wisp_core::body::BodyHandle {
                id: wisp_core::body::BodyId(9),
                len: 1,
            },
            timings: Timings {
                dns: Some(Duration::from_millis(1)),
                connect: None,
            },
        },
        Event::AuthUrl {
            id,
            url: "https://idp.test/auth".into(),
        },
        Event::AuthCompleted { id },
        Event::AuthFailed {
            id,
            error: EngineError {
                kind: ErrorKind::Auth,
                message: "m".into(),
            },
        },
        Event::Failed {
            id,
            error: EngineError {
                kind: ErrorKind::Tls,
                message: "m".into(),
            },
        },
        Event::Cancelled { id },
    ];
    for e in events {
        round_trip(e);
    }
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + i / 251) % 256) as u8).collect()
}

fn post_with(url: String, body: PreparedBody) -> PreparedRequest {
    let mut req = get(url);
    req.method = Method::Post;
    req.body = body;
    req
}

fn temp_file(tag: &str, data: &[u8]) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("wisp-{tag}-{}.bin", std::process::id()));
    std::fs::write(&p, data).unwrap();
    p
}

#[tokio::test]
async fn a_small_file_body_is_resent_after_a_307_redirect() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/first"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", "/second"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/second"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let data = pattern(100_000);
    let file = temp_file("small307", &data);
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(post_with(
        format!("{}/first", server.uri()),
        PreparedBody::File(file.clone()),
    )));
    let evs = collect_until_terminal(&rx).await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[1].url.path(), "/second");
    assert_eq!(got[1].body, data);
    let _ = std::fs::remove_file(file);
}

fn assert_streamed(got: &wiremock::Request, expected_len: usize) {
    let cl = got.headers.get("content-length").expect("content-length");
    assert_eq!(cl.to_str().unwrap(), expected_len.to_string());
    assert!(!got.headers.contains_key("transfer-encoding"));
    assert_eq!(got.body.len(), expected_len);
}

#[tokio::test]
async fn a_20_mib_file_body_streams_with_content_length() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let data = pattern(20 * 1024 * 1024);
    let file = temp_file("big-file", &data);
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(post_with(
        server.uri(),
        PreparedBody::File(file.clone()),
    )));
    let evs = collect_until_terminal(&rx).await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let got = &server.received_requests().await.unwrap()[0];
    assert_streamed(got, data.len());
    // Byte-exact comparison (stronger than a digest; no hashing crate is a dependency).
    assert!(got.body == data, "received bytes differ");
    let _ = std::fs::remove_file(file);
}

#[tokio::test]
async fn user_length_headers_do_not_turn_a_streamed_file_body_chunked() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let data = pattern(20 * 1024 * 1024);
    let file = temp_file("big-file-hdr", &data);
    let mut req = post_with(server.uri(), PreparedBody::File(file.clone()));
    for (n, v) in [("Content-Length", "5"), ("Transfer-Encoding", "chunked")] {
        req.headers.push(Header {
            name: n.into(),
            value: v.into(),
        });
    }
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(req));
    let evs = collect_until_terminal(&rx).await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let got = &server.received_requests().await.unwrap()[0];
    assert_streamed(got, data.len());
    let _ = std::fs::remove_file(file);
}

#[tokio::test]
async fn a_multipart_body_with_a_20_mib_file_streams_with_exact_content_length() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let data = pattern(20 * 1024 * 1024);
    let file = temp_file("big-mp", &data);
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(post_with(
        server.uri(),
        PreparedBody::Multipart(vec![
            PreparedPart {
                name: "t".into(),
                value: PreparedPartValue::Text("hello".into()),
            },
            PreparedPart {
                name: "f".into(),
                value: PreparedPartValue::File(file.clone()),
            },
        ]),
    )));
    let evs = collect_until_terminal(&rx).await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let got = &server.received_requests().await.unwrap()[0];
    let ct = got.headers.get("content-type").unwrap().to_str().unwrap();
    let boundary = ct.strip_prefix("multipart/form-data; boundary=").unwrap();
    let cl: usize = got
        .headers
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(cl, got.body.len());
    assert!(!got.headers.contains_key("transfer-encoding"));
    let start = got
        .body
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .and_then(|first| {
            let tail = &got.body[first + 4..];
            tail.windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| first + 4 + p + 4)
        })
        .unwrap();
    assert_eq!(&got.body[start..start + data.len()], &data[..]);
    let trailer = format!("\r\n--{boundary}--\r\n");
    assert_eq!(&got.body[start + data.len()..], trailer.as_bytes());
    let _ = std::fs::remove_file(file);
}

/// A temp file removed on drop, so a failed assertion leaves nothing behind.
struct TempGuard(std::path::PathBuf);

impl TempGuard {
    fn new(tag: &str, data: &[u8]) -> Self {
        TempGuard(temp_file(tag, data))
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

const TWO_MIB: usize = 2 * 1024 * 1024 + 123;

async fn redirecting_server(status: u16, to: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/first"))
        .respond_with(ResponseTemplate::new(status).insert_header("location", to))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/second"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

async fn send_and_collect(req: PreparedRequest) -> Vec<Event> {
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(req));
    collect_until_terminal(&rx).await
}

async fn streamed_file_resend(status: u16, tag: &str) {
    let server = redirecting_server(status, "/second").await;
    let data = pattern(TWO_MIB);
    let file = TempGuard::new(tag, &data);
    let evs = send_and_collect(post_with(
        format!("{}/first", server.uri()),
        PreparedBody::File(file.0.clone()),
    ))
    .await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let headers: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            Event::Headers { status, .. } => Some(*status),
            _ => None,
        })
        .collect();
    assert_eq!(headers, vec![200]);
    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[1].url.path(), "/second");
    assert_streamed(&got[1], data.len());
    assert_eq!(got[1].body, data);
}

#[tokio::test]
async fn a_streamed_file_body_is_resent_after_a_307_redirect() {
    streamed_file_resend(307, "stream307").await;
}

#[tokio::test]
async fn a_streamed_file_body_is_resent_after_a_308_redirect() {
    streamed_file_resend(308, "stream308").await;
}

#[tokio::test]
async fn a_streamed_multipart_body_is_resent_after_a_307_redirect() {
    let server = redirecting_server(307, "/second").await;
    let data = pattern(TWO_MIB);
    let file = TempGuard::new("mp307", &data);
    let evs = send_and_collect(post_with(
        format!("{}/first", server.uri()),
        PreparedBody::Multipart(vec![
            PreparedPart {
                name: "t".into(),
                value: PreparedPartValue::Text("hello".into()),
            },
            PreparedPart {
                name: "f".into(),
                value: PreparedPartValue::File(file.0.clone()),
            },
        ]),
    ))
    .await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 2);
    let second = &got[1];
    let cl: usize = second
        .headers
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(cl, second.body.len());
    let find = |needle: &[u8]| {
        second
            .body
            .windows(needle.len())
            .position(|w| w == needle)
            .unwrap()
    };
    assert!(String::from_utf8_lossy(&second.body[..200]).contains("name=\"t\""));
    find(b"hello");
    let start = find(b"filename=");
    let start = start + find_from(&second.body[start..], b"\r\n\r\n") + 4;
    assert_eq!(&second.body[start..start + data.len()], &data[..]);
}

fn find_from(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).position(|w| w == needle).unwrap()
}

/// Sends a streamed POST with credentials; returns the requests seen by (first, second) servers.
async fn auth_resend(cross_origin: bool) -> (Vec<wiremock::Request>, Vec<wiremock::Request>) {
    let second = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/second"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&second)
        .await;
    let first = if cross_origin {
        redirecting_server(307, &format!("{}/second", second.uri())).await
    } else {
        redirecting_server(307, "/second").await
    };
    let file = TempGuard::new(
        if cross_origin { "xorigin" } else { "sorigin" },
        &pattern(TWO_MIB),
    );
    let mut req = post_with(
        format!("{}/first", first.uri()),
        PreparedBody::File(file.0.clone()),
    );
    req.headers = vec![
        Header {
            name: "Authorization".into(),
            value: "Bearer secret".into(),
        },
        Header {
            name: "Cookie".into(),
            value: "a=b".into(),
        },
        Header {
            name: "Cookie2".into(),
            value: "c=d".into(),
        },
        Header {
            name: "Proxy-Authorization".into(),
            value: "Basic eDp5".into(),
        },
        Header {
            name: "X-Keep".into(),
            value: "yes".into(),
        },
    ];
    let evs = send_and_collect(req).await;
    assert!(
        matches!(evs.last().unwrap(), Event::Completed { .. }),
        "{evs:?}"
    );
    let a = first.received_requests().await.unwrap();
    let b = second.received_requests().await.unwrap();
    (a, b)
}

#[tokio::test]
async fn a_cross_origin_redirect_drops_authorization_on_resend() {
    let (first, second) = auth_resend(true).await;
    assert!(first[0].headers.contains_key("authorization"));
    assert_eq!(second.len(), 1);
    assert!(!second[0].headers.contains_key("authorization"));
    assert!(!second[0].headers.contains_key("cookie"));
    assert!(!second[0].headers.contains_key("cookie2"));
    assert!(!second[0].headers.contains_key("proxy-authorization"));
    assert_eq!(second[0].headers.get("x-keep").unwrap(), "yes");
    assert_streamed(&second[0], TWO_MIB);
}

#[tokio::test]
async fn a_same_origin_redirect_keeps_authorization_on_resend() {
    let (first, _) = auth_resend(false).await;
    assert_eq!(first.len(), 2);
    assert_eq!(
        first[1].headers.get("authorization").unwrap(),
        "Bearer secret"
    );
    assert_eq!(first[1].headers.get("cookie").unwrap(), "a=b");
}

#[tokio::test]
async fn a_streamed_redirect_loop_fails_after_10_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", "/loop"))
        .mount(&server)
        .await;
    let file = TempGuard::new("loop", &pattern(TWO_MIB));
    let evs = send_and_collect(post_with(
        format!("{}/loop", server.uri()),
        PreparedBody::File(file.0.clone()),
    ))
    .await;
    match evs.last().unwrap() {
        Event::Failed { error, .. } => {
            assert_eq!(error.kind, ErrorKind::Other);
            assert!(
                error.message.contains("too many redirects (10)"),
                "{error:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 10);
}

struct TruncateThen307(std::path::PathBuf);

impl wiremock::Respond for TruncateThen307 {
    fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
        std::fs::write(&self.0, b"short").unwrap();
        ResponseTemplate::new(307).insert_header("location", "/second")
    }
}

#[tokio::test]
async fn a_body_file_that_shrinks_before_a_redirect_resend_fails_with_body_file() {
    let file = TempGuard::new("shrink", &pattern(TWO_MIB));
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/first"))
        .respond_with(TruncateThen307(file.0.clone()))
        .mount(&server)
        .await;
    let evs = send_and_collect(post_with(
        format!("{}/first", server.uri()),
        PreparedBody::File(file.0.clone()),
    ))
    .await;
    match evs.last().unwrap() {
        Event::Failed { error, .. } => {
            assert_eq!(error.kind, ErrorKind::BodyFile);
            assert!(
                error.message.contains("changed before the redirect resend"),
                "{error:?}"
            );
            assert!(error.message.contains(&file.0.display().to_string()));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn follow_redirects_false_surfaces_a_307_for_a_streamed_body() {
    let server = redirecting_server(307, "/second").await;
    let file = TempGuard::new("nofollow", &pattern(TWO_MIB));
    let mut req = post_with(
        format!("{}/first", server.uri()),
        PreparedBody::File(file.0.clone()),
    );
    req.client.follow_redirects = false;
    let evs = send_and_collect(req).await;
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Headers { status: 307, .. })),
        "{evs:?}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn fresh_then_reused_connection_timings() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    let mut seen = Vec::new();
    for _ in 0..2 {
        engine.submit(Command::Send(get(format!("{}/t", server.uri()))));
        match collect_until_terminal(&rx).await.last().unwrap() {
            Event::Completed { timings, .. } => seen.push(*timings),
            other => panic!("unexpected {other:?}"),
        }
    }
    // IP-literal host: no lookup. The first send opens the connection; the second reuses it.
    assert_eq!(seen[0].dns, None);
    assert!(seen[0].connect.is_some());
    assert_eq!(
        seen[1],
        Timings {
            dns: None,
            connect: None
        }
    );
}

#[test]
fn debug_redacts_engine_config() {
    use wisp_core::auth::oauth2::{Grant, TokenKey};
    let config = EngineConfig::default();
    config
        .refresh_tokens
        .save(
            &TokenKey {
                grant: Grant::AuthCode,
                token_url: "https://i/t".into(),
                client_id: "c".into(),
                scope: "r".into(),
                collection: None,
                environment: None,
            },
            "REFRESH-MARKER-123",
        )
        .unwrap();
    let text = format!("{config:?}");
    assert!(text.contains("EngineConfig"));
    assert!(text.contains("<redacted>"));
    assert!(!text.contains("REFRESH-MARKER-123"));
    let pretty = format!("{config:#?}");
    assert!(pretty.contains("<redacted>"));
    assert!(!pretty.contains("REFRESH-MARKER-123"));
}
