use std::io::{Read, Write};
use std::sync::Arc;

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use wisp_core::*;

fn request(url: String, verify_tls: bool) -> PreparedRequest {
    PreparedRequest {
        method: Method::Get,
        url,
        headers: vec![],
        body: PreparedBody::None,
        timeout: None,
        client: ClientSettings {
            verify_tls,
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

/// Serves one TLS connection with a fresh self-signed certificate. `headers` extra headers are
/// added to the response. Returns the bound port.
fn serve_tls(body: &'static [u8], headers: usize) -> u16 {
    let ck =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![ck.cert.der().clone()], key)
    .unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((tcp, _)) = l.accept() {
            let conn = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut s = rustls::StreamOwned::new(conn, tcp);
            let mut buf = [0u8; 4096];
            // A failed handshake (untrusted cert) surfaces here; nothing more to do then.
            if s.read(&mut buf).is_err() {
                return;
            }
            let mut r = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for i in 0..headers {
                r.push_str(&format!("X-H-{i}: v\r\n"));
            }
            r.push_str("\r\n");
            let _ = s.write_all(r.as_bytes());
            let _ = s.write_all(body);
            s.conn.send_close_notify();
            let _ = s.flush();
        }
    });
    port
}

async fn send(url: String, verify_tls: bool) -> (Engine, Vec<Event>) {
    let (engine, rx) = Engine::start(EngineConfig::default());
    engine.submit(Command::Send(request(url, verify_tls)));
    let evs = collect_until_terminal(&rx).await;
    (engine, evs)
}

#[tokio::test]
async fn verify_tls_true_rejects_a_self_signed_certificate() {
    let port = serve_tls(b"hello", 0);
    let (_e, mut evs) = send(format!("https://127.0.0.1:{port}/"), true).await;
    match evs.pop().unwrap() {
        Event::Failed { error, .. } => {
            assert_eq!(error.kind, ErrorKind::Tls, "{}", error.message);
            assert!(
                error.message.to_lowercase().contains("certificate"),
                "{}",
                error.message
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn verify_tls_false_accepts_a_self_signed_certificate() {
    let port = serve_tls(b"hello over tls", 0);
    let (engine, mut evs) = send(format!("https://127.0.0.1:{port}/"), false).await;
    match evs.pop().unwrap() {
        Event::Completed { body, size, .. } => {
            assert_eq!(size, 14);
            assert_eq!(
                engine.bodies().read(&body, 0, 100).unwrap(),
                b"hello over tls"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let status = evs.iter().find_map(|e| match e {
        Event::Headers { status, .. } => Some(*status),
        _ => None,
    });
    assert_eq!(status, Some(200));
}

#[tokio::test]
async fn a_tls_response_with_1000_headers_completes() {
    let port = serve_tls(b"", 998);
    let (_e, mut evs) = send(format!("https://127.0.0.1:{port}/"), false).await;
    assert!(matches!(evs.pop().unwrap(), Event::Completed { .. }));
}
