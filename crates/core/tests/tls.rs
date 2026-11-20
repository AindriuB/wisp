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

// ---- Private CA trust ----

struct Ca {
    params: rcgen::CertificateParams,
    cert: rcgen::Certificate,
    key: rcgen::KeyPair,
}

/// Validity from yesterday to 30 days ahead. `time` is not a dependency, so this is
/// day-granular rather than now-1h.
fn set_validity(params: &mut rcgen::CertificateParams) {
    let today = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        / 86_400;
    let ymd = |days: i64| {
        let z = today + days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
        rcgen::date_time_ymd(y, m, d)
    };
    params.not_before = ymd(-1);
    params.not_after = ymd(30);
}

fn make_ca() -> Ca {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Wisp test CA");
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    set_validity(&mut params);
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    Ca { params, cert, key }
}

/// Serves one TLS connection with a leaf signed by `ca`. Returns the bound port.
fn serve_tls_signed_by(ca: &Ca) -> u16 {
    let mut params =
        rcgen::CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    set_validity(&mut params);
    let key = rcgen::KeyPair::generate().unwrap();
    let issuer = rcgen::Issuer::from_params(&ca.params, &ca.key);
    let leaf = params.signed_by(&key, &issuer).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    )
    .unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((tcp, _)) = l.accept() {
            let conn = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut s = rustls::StreamOwned::new(conn, tcp);
            let mut buf = [0u8; 4096];
            if s.read(&mut buf).is_err() {
                return;
            }
            let _ =
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
            s.conn.send_close_notify();
            let _ = s.flush();
        }
    });
    port
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wisp-trust-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn trust_with(ca: &Ca, system: bool) -> TlsTrust {
    TlsTrust {
        system,
        extra: vec![ca.cert.der().to_vec()],
    }
}

async fn send_with(trust: TlsTrust, port: u16, verify_tls: bool) -> Event {
    let (engine, rx) = Engine::start(EngineConfig {
        tls: trust,
        ..EngineConfig::default()
    });
    engine.submit(Command::Send(request(
        format!("https://127.0.0.1:{port}/"),
        verify_tls,
    )));
    collect_until_terminal(&rx).await.pop().unwrap()
}

fn assert_tls_failure(ev: Event) {
    match ev {
        Event::Failed { error, .. } => assert_eq!(error.kind, ErrorKind::Tls, "{}", error.message),
        other => panic!("unexpected {other:?}"),
    }
}

fn assert_completed(ev: Event) {
    assert!(matches!(ev, Event::Completed { .. }), "unexpected {ev:?}");
}

#[tokio::test]
async fn trust_system_off_without_file_rejects_private_ca() {
    let ca = make_ca();
    let port = serve_tls_signed_by(&ca);
    let trust = TlsTrust {
        system: false,
        extra: vec![],
    };
    assert_tls_failure(send_with(trust, port, true).await);
}

#[tokio::test]
async fn trust_system_off_with_file_accepts_private_ca() {
    let ca = make_ca();
    let port = serve_tls_signed_by(&ca);
    assert_completed(send_with(trust_with(&ca, false), port, true).await);
}

#[tokio::test]
async fn trust_system_on_with_file_accepts_private_ca() {
    let ca = make_ca();
    let port = serve_tls_signed_by(&ca);
    assert_completed(send_with(trust_with(&ca, true), port, true).await);
}

#[tokio::test]
async fn trust_system_on_without_file_rejects_private_ca() {
    let ca = make_ca();
    let port = serve_tls_signed_by(&ca);
    assert_tls_failure(send_with(TlsTrust::default(), port, true).await);
}

#[tokio::test]
async fn trust_set_tls_trust_takes_effect_on_a_running_engine() {
    let ca = make_ca();
    let (engine, rx) = Engine::start(EngineConfig {
        tls: TlsTrust {
            system: false,
            extra: vec![],
        },
        ..EngineConfig::default()
    });
    let port = serve_tls_signed_by(&ca);
    engine.submit(Command::Send(request(
        format!("https://127.0.0.1:{port}/"),
        true,
    )));
    assert_tls_failure(collect_until_terminal(&rx).await.pop().unwrap());
    engine.set_tls_trust(trust_with(&ca, false));
    let port = serve_tls_signed_by(&ca);
    engine.submit(Command::Send(request(
        format!("https://127.0.0.1:{port}/"),
        true,
    )));
    assert_completed(collect_until_terminal(&rx).await.pop().unwrap());
}

#[tokio::test]
async fn trust_verify_tls_false_still_accepts_self_signed_with_system_off() {
    let port = serve_tls(b"x", 0);
    let trust = TlsTrust {
        system: false,
        extra: vec![],
    };
    assert_completed(send_with(trust, port, false).await);
}

#[tokio::test]
async fn trust_verify_tls_false_still_accepts_self_signed_with_system_on() {
    let port = serve_tls(b"x", 0);
    assert_completed(send_with(TlsTrust::default(), port, false).await);
}

#[test]
fn trust_debug_prints_counts_not_bytes() {
    let t = TlsTrust {
        system: true,
        extra: vec![vec![0xAB, 0xCD]],
    };
    let s = format!("{t:?}");
    assert!(s.contains("extra_certificates: 1"), "{s}");
    assert!(!s.contains("171"), "{s}");
}

fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap());
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

fn load_text(name: &str, text: &str) -> Result<Vec<Vec<u8>>, TrustError> {
    let dir = scratch(name);
    let p = dir.join("ca.pem");
    std::fs::write(&p, text).unwrap();
    TlsTrust::load_pem_file(&p)
}

#[test]
fn trust_load_missing_path_is_refused() {
    let dir = scratch("missing");
    let e = TlsTrust::load_pem_file(&dir.join("nope.pem")).unwrap_err();
    assert!(e.to_string().contains("nope.pem"), "{e}");
}

#[test]
fn trust_load_directory_is_refused() {
    let dir = scratch("dir");
    let e = TlsTrust::load_pem_file(&dir).unwrap_err();
    assert!(e.to_string().contains("not a regular file"), "{e}");
}

#[test]
fn trust_load_file_over_1_mib_is_refused() {
    let dir = scratch("big");
    let p = dir.join("big.pem");
    std::fs::write(&p, vec![b'a'; 1024 * 1024 + 1]).unwrap();
    let e = TlsTrust::load_pem_file(&p).unwrap_err();
    assert!(e.to_string().contains("1 MiB"), "{e}");
}

#[test]
fn trust_load_file_without_certificate_is_refused() {
    let e = load_text("empty", "just some text\n").unwrap_err();
    assert!(e.to_string().contains("no certificate"), "{e}");
}

#[test]
fn trust_load_block_that_is_not_a_certificate_is_refused() {
    let e = load_text("garbage", &pem("CERTIFICATE", b"not a certificate")).unwrap_err();
    assert!(e.to_string().contains("does not parse"), "{e}");
}

#[test]
fn trust_load_private_key_block_is_refused() {
    let ca = make_ca();
    let text = format!(
        "{}{}",
        pem("CERTIFICATE", ca.cert.der()),
        pem("PRIVATE KEY", &ca.key.serialize_der())
    );
    let e = load_text("key", &text).unwrap_err();
    assert!(e.to_string().contains("private key"), "{e}");
}

#[test]
fn trust_load_two_certificate_bundle_loads_both() {
    let (a, b) = (make_ca(), make_ca());
    let text = format!(
        "# bundle\n{}\n{}",
        pem("CERTIFICATE", a.cert.der()),
        pem("CERTIFICATE", b.cert.der())
    );
    let certs = load_text("bundle", &text).unwrap();
    assert_eq!(certs, vec![a.cert.der().to_vec(), b.cert.der().to_vec()]);
}
