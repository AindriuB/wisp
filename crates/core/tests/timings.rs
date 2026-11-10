//! Phase-timing attribution: DNS and connect values belong to the request that opened the
//! connection, `None` for one that reused it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use wisp_core::*;

fn request(url: String) -> PreparedRequest {
    PreparedRequest {
        method: Method::Get,
        url,
        headers: vec![],
        body: PreparedBody::None,
        timeout: None,
        client: ClientSettings {
            verify_tls: false,
            follow_redirects: true,
            proxy: None,
            collection: None,
        },
        warnings: vec![],
        auth: None,
        environment: None,
    }
}

/// The terminal event of each of `n` requests, in completion order.
async fn terminals(rx: &EventReceiver, n: usize) -> Vec<Event> {
    let mut out = Vec::new();
    while out.len() < n {
        let ev = rx.recv().await.unwrap();
        if matches!(
            ev,
            Event::Completed { .. } | Event::Failed { .. } | Event::Cancelled { .. }
        ) {
            out.push(ev);
        }
    }
    out
}

fn completed(ev: &Event) -> (Duration, Timings) {
    match ev {
        Event::Completed { total, timings, .. } => (*total, *timings),
        other => panic!("unexpected {other:?}"),
    }
}

/// A keep-alive HTTP/1.1 server. Each connection answers every request on it. With `gate`, a
/// connection holds its first response until that many connections have a request waiting, so
/// no connection becomes idle while another request is still connecting.
fn serve_h1(gate: Option<usize>) -> (u16, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    let barrier = gate.map(|n| Arc::new(Barrier::new(n)));
    std::thread::spawn(move || {
        for tcp in l.incoming().flatten() {
            count.fetch_add(1, Ordering::SeqCst);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut tcp = tcp;
                let mut first = true;
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match tcp.read(&mut chunk) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    buf.clear();
                    if let (true, Some(b)) = (first, &barrier) {
                        b.wait();
                    }
                    first = false;
                    let r = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
                    if tcp.write_all(r.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, accepted)
}

fn some_connects(evs: &[Event]) -> usize {
    evs.iter()
        .filter(|e| completed(e).1.connect.is_some())
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn sequential_sends_report_one_connect_per_accepted_connection() {
    let (port, accepted) = serve_h1(None);
    let (engine, rx) = Engine::start(EngineConfig::default());
    let mut all = Vec::new();
    for _ in 0..20 {
        engine.submit(Command::Send(request(format!("http://127.0.0.1:{port}/"))));
        all.extend(terminals(&rx, 1).await);
    }
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert_eq!(some_connects(&all), 1);
    assert!(completed(&all[0]).1.connect.is_some());
    assert!(
        all[1..]
            .iter()
            .all(|e| completed(e).1 == Timings::default())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_sends_report_one_connect_per_accepted_connection() {
    let (port, accepted) = serve_h1(Some(20));
    let (engine, rx) = Engine::start(EngineConfig::default());
    for _ in 0..20 {
        engine.submit(Command::Send(request(format!("http://127.0.0.1:{port}/"))));
    }
    let all = terminals(&rx, 20).await;
    assert_eq!(accepted.load(Ordering::SeqCst), 20);
    assert_eq!(some_connects(&all), 20);
    // A second wave reuses the idle connections: nothing opened, nothing reported.
    for _ in 0..5 {
        engine.submit(Command::Send(request(format!("http://127.0.0.1:{port}/"))));
    }
    let more = terminals(&rx, 5).await;
    assert_eq!(accepted.load(Ordering::SeqCst), 20);
    assert_eq!(some_connects(&more), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn dns_is_set_only_for_a_request_that_did_a_lookup() {
    let (port, _) = serve_h1(None);
    let (engine, rx) = Engine::start(EngineConfig::default());
    // A name: lookup, then connect.
    engine.submit(Command::Send(request(format!("http://localhost:{port}/"))));
    let first = terminals(&rx, 1).await;
    let (total, t) = completed(&first[0]);
    let dns = t.dns.expect("a hostname is looked up");
    assert!(dns <= total && t.connect.unwrap() <= total);
    // Reused connection: no lookup either.
    engine.submit(Command::Send(request(format!("http://localhost:{port}/"))));
    let second = terminals(&rx, 1).await;
    assert_eq!(completed(&second[0]).1, Timings::default());
    // An IP literal on a fresh connection: connect, but no lookup.
    let (port2, _) = serve_h1(None);
    engine.submit(Command::Send(request(format!("http://127.0.0.1:{port2}/"))));
    let third = terminals(&rx, 1).await;
    let (_, t) = completed(&third[0]);
    assert_eq!(t.dns, None);
    assert!(t.connect.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_lookup_is_still_a_dns_error() {
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(request(
        "http://no-such-host.invalid/".to_owned(),
    )));
    match terminals(&rx, 1).await.pop().unwrap() {
        Event::Failed { error, .. } => assert_eq!(error.kind, ErrorKind::Dns, "{}", error.message),
        other => panic!("unexpected {other:?}"),
    }
}

fn tls_config(alpn: &[&[u8]]) -> Arc<rustls::ServerConfig> {
    let ck =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der()));
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![ck.cert.der().clone()], key)
    .unwrap();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Arc::new(config)
}

/// An HTTP/2-over-TLS server (ALPN `h2`) answering every stream with 200.
async fn serve_h2() -> (u16, Arc<AtomicUsize>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    let acceptor = tokio_rustls::TlsAcceptor::from(tls_config(&[b"h2"]));
    tokio::spawn(async move {
        while let Ok((tcp, _)) = l.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let Ok(mut conn) = h2::server::handshake(tls).await else {
                    return;
                };
                while let Some(Ok((_req, mut respond))) = conn.accept().await {
                    let resp = http::Response::new(());
                    if let Ok(mut s) = respond.send_response(resp, false) {
                        let _ = s.send_data(bytes::Bytes::from_static(b"ok"), true);
                    }
                }
            });
        }
    });
    (port, accepted)
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_concurrent_http2_sends_report_one_connect() {
    let (port, accepted) = serve_h2().await;
    let (engine, rx) = Engine::start(EngineConfig::default());
    for _ in 0..10 {
        engine.submit(Command::Send(request(format!("https://localhost:{port}/"))));
    }
    let all = terminals(&rx, 10).await;
    let ok: Vec<_> = all
        .iter()
        .map(|e| matches!(e, Event::Completed { .. }))
        .collect();
    assert!(ok.iter().all(|v| *v), "{all:?}");
    assert!(accepted.load(Ordering::SeqCst) >= 1);
    assert_eq!(some_connects(&all), 1, "{all:?}");
    // DNS belongs to the same request as the connect.
    for e in &all {
        let (total, t) = completed(e);
        assert_eq!(t.dns.is_some(), t.connect.is_some());
        assert!(t.dns.is_none_or(|d| d <= total));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_stalled_connect_leaves_nothing_for_the_next_request() {
    // A TLS endpoint that accepts and never answers the handshake.
    let stall = TcpListener::bind("127.0.0.1:0").unwrap();
    let stall_port = stall.local_addr().unwrap().port();
    let (tx, accepted) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for tcp in stall.incoming().flatten() {
            held.push(tcp);
            let _ = tx.send(());
        }
    });
    let (engine, rx) = Engine::start(EngineConfig::default());
    let id = engine.submit(Command::Send(request(format!(
        "https://127.0.0.1:{stall_port}/"
    ))));
    tokio::task::spawn_blocking(move || accepted.recv().unwrap())
        .await
        .unwrap();
    engine.submit(Command::Cancel(id));
    assert!(matches!(
        terminals(&rx, 1).await.pop().unwrap(),
        Event::Cancelled { .. }
    ));
    let (port, _) = serve_h1(None);
    engine.submit(Command::Send(request(format!("http://127.0.0.1:{port}/"))));
    let ev = terminals(&rx, 1).await;
    let (total, t) = completed(&ev[0]);
    assert!(t.connect.is_some_and(|c| c <= total), "{t:?}");
    assert_eq!(t.dns, None);
}

/// A TLS HTTP/1.1 server whose second connection is held at the handshake until released, and
/// whose first connection holds its first response until released.
struct Raced {
    port: u16,
    /// Per accepted connection (in accept order), the number of requests it answered.
    served: Arc<[AtomicUsize; 3]>,
    first_request: std::sync::mpsc::Receiver<()>,
    second_accepted: std::sync::mpsc::Receiver<()>,
    release_first: std::sync::mpsc::Sender<()>,
    release_second: std::sync::mpsc::Sender<()>,
}

fn serve_raced() -> Raced {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let served: Arc<[AtomicUsize; 3]> = Arc::new(Default::default());
    let (first_tx, first_request) = std::sync::mpsc::channel::<()>();
    let (second_tx, second_accepted) = std::sync::mpsc::channel::<()>();
    let (release_first, mut rel_first) = {
        let (t, r) = std::sync::mpsc::channel::<()>();
        (t, Some(r))
    };
    let (release_second, mut rel_second) = {
        let (t, r) = std::sync::mpsc::channel::<()>();
        (t, Some(r))
    };
    let config = tls_config(&[b"http/1.1"]);
    let counts = served.clone();
    std::thread::spawn(move || {
        for (idx, tcp) in l.incoming().flatten().enumerate().take(3) {
            let (first_tx, second_tx) = (first_tx.clone(), second_tx.clone());
            let hold = match idx {
                0 => rel_first.take(),
                1 => rel_second.take(),
                _ => None,
            };
            let (config, counts) = (config.clone(), counts.clone());
            std::thread::spawn(move || {
                if idx == 1 {
                    let _ = second_tx.send(());
                    // Hold the handshake: the client's connect cannot finish.
                    let _ = hold.as_ref().unwrap().recv();
                }
                let conn = rustls::ServerConnection::new(config).unwrap();
                let mut tls = rustls::StreamOwned::new(conn, tcp);
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match tls.read(&mut chunk) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    buf.clear();
                    if idx == 0 && counts[0].load(Ordering::SeqCst) == 0 {
                        let _ = first_tx.send(());
                        let _ = hold.as_ref().unwrap().recv();
                    }
                    counts[idx].fetch_add(1, Ordering::SeqCst);
                    let r = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
                    if tls.write_all(r.as_bytes()).is_err() || tls.flush().is_err() {
                        return;
                    }
                }
            });
        }
    });
    Raced {
        port,
        served,
        first_request,
        second_accepted,
        release_first,
        release_second,
    }
}

/// A connect that loses the race to a pooled connection still finishes and is pooled. A later
/// request that reuses it did not open it, so it must not inherit that connect's timings.
#[tokio::test(flavor = "multi_thread")]
async fn a_pooled_connection_whose_connect_lost_a_race_is_not_claimed_by_a_later_request() {
    let Raced {
        port,
        served,
        first_request,
        second_accepted,
        release_first,
        release_second,
    } = serve_raced();
    let url = format!("https://127.0.0.1:{port}/");
    let (engine, rx) = Engine::start(EngineConfig::default());
    // B opens connection 0 and is held at the server.
    engine.submit(Command::Send(request(url.clone())));
    tokio::task::spawn_blocking(move || first_request.recv().unwrap())
        .await
        .unwrap();
    // A finds nothing idle and starts connecting; the server holds that handshake.
    engine.submit(Command::Send(request(url.clone())));
    tokio::task::spawn_blocking(move || second_accepted.recv().unwrap())
        .await
        .unwrap();
    // B finishes, its connection goes idle, and A takes it instead of waiting for its own.
    release_first.send(()).unwrap();
    let both = terminals(&rx, 2).await;
    assert_eq!(some_connects(&both), 1, "{both:?}");
    assert_eq!(served[0].load(Ordering::SeqCst), 2, "A rode connection 0");
    // A's own connect now finishes and is pooled behind connection 0.
    release_second.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    engine.submit(Command::Send(request(url)));
    let z = terminals(&rx, 1).await;
    assert_eq!(
        served[1].load(Ordering::SeqCst),
        1,
        "Z reused A's connection"
    );
    assert_eq!(completed(&z[0]).1, Timings::default(), "{z:?}");
}
