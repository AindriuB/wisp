use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use wiremock::matchers::{body_bytes, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wisp_core::*;

fn get(url: String) -> PreparedRequest {
    PreparedRequest {
        method: Method::Get,
        url,
        headers: vec![],
        body: None,
        timeout: None,
    }
}

#[tokio::test]
async fn a_get_returning_200_yields_one_completed_with_matching_body_and_size() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
        .mount(&server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    let id = engine.submit(Command::Send(get(format!("{}/x", server.uri()))));
    match rx.recv().await.unwrap() {
        Event::Completed {
            id: got,
            status,
            body,
            size,
            reason,
            ..
        } => {
            assert_eq!(got, id);
            assert_eq!(status, 200);
            assert_eq!(reason.as_deref(), Some("OK"));
            assert_eq!(body, b"hello");
            assert_eq!(size, 5);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(rx.is_empty());
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
        body: Some(b"payload".to_vec()),
        timeout: None,
    }));
    match rx.recv().await.unwrap() {
        Event::Completed { status, .. } => assert_eq!(status, 201),
        other => panic!("unexpected {other:?}"),
    }
}

async fn failure(req: PreparedRequest) -> ErrorKind {
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(req));
    match rx.recv().await.unwrap() {
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
async fn cancel_on_an_in_flight_request_fails_with_cancelled_and_no_completed() {
    let slow = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(300)))
        .mount(&slow)
        .await;
    let sentinel_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(800)))
        .mount(&sentinel_server)
        .await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    let id = engine.submit(Command::Send(get(slow.uri())));
    let sentinel = engine.submit(Command::Send(get(sentinel_server.uri())));
    assert_eq!(engine.submit(Command::Cancel(id)), id);
    let mut cancelled = false;
    loop {
        match rx.recv().await.unwrap() {
            Event::Failed { id: got, error } => {
                assert_eq!(got, id);
                assert_eq!(error.kind, ErrorKind::Cancelled);
                cancelled = true;
            }
            Event::Completed { id: got, .. } if got == sentinel => break,
            Event::Completed { id: got, .. } => panic!("Completed for {got:?} after cancel"),
        }
    }
    assert!(cancelled, "Failed(Cancelled) arrives before the sentinel");
    drop(engine);
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
    assert!(matches!(ev, Event::Completed { status: 204, .. }));
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
        body: Some(b"hi".to_vec()),
        timeout: Some(Duration::from_millis(1500)),
    }));
    round_trip(Command::Cancel(RequestId(7)));
    round_trip(Event::Completed {
        id: RequestId(3),
        status: 200,
        reason: Some("OK".into()),
        headers: vec![Header {
            name: "a".into(),
            value: "b".into(),
        }],
        body: b"x".to_vec(),
        size: 1,
        total: Duration::from_millis(12),
    });
    round_trip(Event::Failed {
        id: RequestId(4),
        error: EngineError {
            kind: ErrorKind::Tls,
            message: "m".into(),
        },
    });
}
