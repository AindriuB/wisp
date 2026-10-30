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
        },
        warnings: vec![],
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
        },
        warnings: vec![],
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
        },
        warnings: vec![ResolveWarning::PatternMismatch {
            name: "n".into(),
            pattern: "p".into(),
            value: "v".into(),
        }],
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
