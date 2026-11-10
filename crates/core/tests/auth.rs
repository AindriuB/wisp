//! Digest and SigV4 through the engine, against wiremock.

use wiremock::matchers::{header_exists, method};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wisp_core::auth::{EngineAuth, sigv4 as sig};
use wisp_core::*;

fn request(url: String, auth: EngineAuth) -> PreparedRequest {
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
        auth: Some(auth),
    }
}

fn digest() -> EngineAuth {
    EngineAuth::Digest {
        username: "Mufasa".into(),
        password: "Circle of Life".into(),
    }
}

fn sigv4(service: &str, token: &str) -> EngineAuth {
    EngineAuth::AwsSigV4 {
        access_key: "AKIDEXAMPLE".into(),
        secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
        session_token: token.into(),
        region: "us-east-1".into(),
        service: service.into(),
    }
}

async fn run(req: PreparedRequest) -> Vec<Event> {
    let (engine, rx) = Engine::start(EngineConfig::default());
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

fn statuses(evs: &[Event]) -> Vec<u16> {
    evs.iter()
        .filter_map(|e| match e {
            Event::Headers { status, .. } => Some(*status),
            _ => None,
        })
        .collect()
}

const CHALLENGE: &str = "Digest realm=\"http-auth@example.org\", qop=\"auth, auth-int\", \
    algorithm=SHA-256, nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", \
    opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\"";

/// 200 for a request carrying `Authorization`, else a Digest challenge.
async fn digest_server(always_401: bool) -> MockServer {
    let server = MockServer::start().await;
    if !always_401 {
        Mock::given(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .with_priority(1)
            .mount(&server)
            .await;
    }
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(401).insert_header("www-authenticate", CHALLENGE))
        .with_priority(2)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn digest_answers_a_challenge_and_retries_once() {
    let server = digest_server(false).await;
    let evs = run(request(
        format!("{}/dir/index.html?x=1", server.uri()),
        digest(),
    ))
    .await;
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    // The first 401 emits nothing.
    assert_eq!(statuses(&evs), vec![200]);
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
    assert!(reqs[0].headers.get("authorization").is_none());
    let auth = reqs[1]
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(auth.starts_with("Digest username=\"Mufasa\""), "{auth}");
    assert!(auth.contains("uri=\"/dir/index.html?x=1\""), "{auth}");
    assert!(auth.contains("algorithm=SHA-256"), "{auth}");
}

#[tokio::test]
async fn digest_resends_an_in_memory_body() {
    let server = digest_server(false).await;
    let mut req = request(server.uri(), digest());
    req.method = Method::Post;
    req.body = PreparedBody::Bytes(b"payload".to_vec());
    let evs = run(req).await;
    assert_eq!(statuses(&evs), vec![200]);
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
    assert!(reqs.iter().all(|r| r.body == b"payload"));
}

#[tokio::test]
async fn a_second_401_is_returned_after_two_requests() {
    let server = digest_server(true).await;
    let evs = run(request(server.uri(), digest())).await;
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    assert_eq!(statuses(&evs), vec![401]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_digest_request_with_a_file_body_fails_before_sending() {
    let server = digest_server(false).await;
    let file = std::env::temp_dir().join(format!("wisp-digest-{}.bin", std::process::id()));
    std::fs::write(&file, b"data").unwrap();
    for body in [
        PreparedBody::File(file.clone()),
        PreparedBody::Multipart(vec![PreparedPart {
            name: "f".into(),
            value: PreparedPartValue::File(file.clone()),
        }]),
    ] {
        let mut req = request(server.uri(), digest());
        req.method = Method::Post;
        req.body = body;
        let evs = run(req).await;
        match evs.last().unwrap() {
            Event::Failed { error, .. } => {
                assert_eq!(error.kind, ErrorKind::Body);
                assert_eq!(
                    error.message,
                    "Digest auth needs a replayable body; File bodies cannot be resent"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    let _ = std::fs::remove_file(&file);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_digest_challenge_with_auth_int_only_fails_without_the_password() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).insert_header(
            "www-authenticate",
            "Digest realm=\"r\", nonce=\"n\", qop=\"auth-int\"",
        ))
        .mount(&server)
        .await;
    let evs = run(request(server.uri(), digest())).await;
    match evs.last().unwrap() {
        Event::Failed { error, .. } => {
            assert!(error.message.contains("not supported"), "{error:?}");
            assert!(!error.message.contains("Circle of Life"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

async fn signed_request(service: &str, token: &str, body: PreparedBody) -> wiremock::Request {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let mut req = request(
        format!("{}/bucket/key", server.uri()),
        sigv4(service, token),
    );
    req.method = Method::Put;
    req.body = body;
    let evs = run(req).await;
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    let mut reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let r = reqs.remove(0);
    assert_signature(&r, service, token);
    r
}

fn hdr<'a>(r: &'a wiremock::Request, name: &str) -> &'a str {
    r.headers.get(name).unwrap().to_str().unwrap()
}

/// Unix seconds of an `x-amz-date` stamp (`YYYYMMDDTHHMMSSZ`).
fn stamp_secs(stamp: &str) -> u64 {
    let n = |r: std::ops::Range<usize>| stamp[r].parse::<i64>().unwrap();
    let (y, m, d) = (n(0..4), n(4..6), n(6..8));
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + n(9..11) * 3600 + n(11..13) * 60 + n(13..15)).unwrap()
}

/// Re-signs the request the server received (method, URL, signed headers, body) with the
/// same credentials and the time it carries, and asserts the `Authorization` matches.
fn assert_signature(r: &wiremock::Request, service: &str, token: &str) {
    let auth = hdr(r, "authorization");
    let signed_names: Vec<&str> = auth
        .split("SignedHeaders=")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .split(';')
        .collect();
    let headers: Vec<(String, String)> = r
        .headers
        .iter()
        .filter(|(n, _)| signed_names.contains(&n.as_str()))
        .map(|(n, v)| (n.as_str().to_owned(), v.to_str().unwrap().to_owned()))
        .collect();
    let body_hash = sig::sha256_hex(&r.body);
    let payload = if service == "s3" {
        let h = hdr(r, "x-amz-content-sha256");
        assert!(h == "UNSIGNED-PAYLOAD" || h == body_hash, "{h}");
        h.to_owned()
    } else {
        body_hash
    };
    let creds = sig::Credentials {
        access_key: "AKIDEXAMPLE",
        secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        session_token: token,
        region: "us-east-1",
        service,
    };
    let want = sig::sign(
        r.method.as_str(),
        r.url.as_str(),
        &headers,
        &payload,
        &creds,
        stamp_secs(hdr(r, "x-amz-date")),
    )
    .unwrap();
    assert_eq!(auth, want.authorization, "{}", want.canonical_request);
}

#[tokio::test]
async fn sigv4_adds_date_and_authorization() {
    let r = signed_request("execute-api", "", PreparedBody::Bytes(b"hi".to_vec())).await;
    let date = hdr(&r, "x-amz-date");
    assert_eq!(date.len(), 16, "{date}");
    let auth = hdr(&r, "authorization");
    let want = format!(
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/{}/us-east-1/execute-api/aws4_request",
        &date[..8]
    );
    assert!(auth.starts_with(&want), "{auth}");
    assert!(r.headers.get("x-amz-security-token").is_none());
    assert!(r.headers.get("x-amz-content-sha256").is_none());
}

#[tokio::test]
async fn sigv4_adds_the_session_token_and_s3_content_hash() {
    let r = signed_request("s3", "TOKEN123", PreparedBody::Bytes(b"hi".to_vec())).await;
    assert_eq!(hdr(&r, "x-amz-security-token"), "TOKEN123");
    // SHA-256("hi")
    assert_eq!(
        hdr(&r, "x-amz-content-sha256"),
        "8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4"
    );
    assert!(
        hdr(&r, "authorization")
            .contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token")
    );
}

#[tokio::test]
async fn sigv4_signs_a_file_body_as_unsigned_payload() {
    let file = std::env::temp_dir().join(format!("wisp-sigv4-{}.bin", std::process::id()));
    std::fs::write(&file, b"file data").unwrap();
    let r = signed_request("s3", "", PreparedBody::File(file.clone())).await;
    let _ = std::fs::remove_file(&file);
    assert_eq!(hdr(&r, "x-amz-content-sha256"), "UNSIGNED-PAYLOAD");
    assert_eq!(r.body, b"file data");
}

async fn redirecting(status: u16, to: String) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(status).insert_header("location", to.as_str()))
        .mount(&server)
        .await;
    server
}

async fn ok_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

fn assert_no_auth(reqs: &[wiremock::Request]) {
    for r in reqs {
        for h in r.headers.iter() {
            let n = h.0.as_str();
            assert!(
                n != "authorization" && !n.starts_with("x-amz-"),
                "leaked {n}: {r:?}"
            );
        }
    }
}

#[tokio::test]
async fn sigv4_token_is_not_sent_to_another_origin_on_a_302() {
    let evil = ok_server().await;
    let good = redirecting(302, format!("{}/x", evil.uri())).await;
    let evs = run(request(
        format!("{}/a", good.uri()),
        sigv4("s3", "TOKEN123"),
    ))
    .await;
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    let got = evil.received_requests().await.unwrap();
    assert_eq!(got.len(), 1);
    assert_no_auth(&got);
    assert!(
        hdr(
            &good.received_requests().await.unwrap()[0],
            "x-amz-security-token"
        ) == "TOKEN123"
    );
}

#[tokio::test]
async fn sigv4_file_body_307_to_another_origin_sends_no_auth() {
    let file = std::env::temp_dir().join(format!("wisp-sigv4-307-{}.bin", std::process::id()));
    std::fs::write(&file, vec![7u8; 2 * 1024 * 1024]).unwrap();
    let evil = ok_server().await;
    let good = redirecting(307, format!("{}/x", evil.uri())).await;
    let mut req = request(format!("{}/a", good.uri()), sigv4("s3", "TOKEN123"));
    req.method = Method::Put;
    req.body = PreparedBody::File(file.clone());
    let evs = run(req).await;
    let _ = std::fs::remove_file(&file);
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    assert_no_auth(&evil.received_requests().await.unwrap());
}

#[tokio::test]
async fn digest_is_not_answered_after_a_307_to_another_origin() {
    let evil = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(401).insert_header("www-authenticate", CHALLENGE))
        .mount(&evil)
        .await;
    let good = redirecting(307, format!("{}/x", evil.uri())).await;
    let mut req = request(format!("{}/a", good.uri()), digest());
    req.method = Method::Post;
    req.body = PreparedBody::Bytes(b"body".to_vec());
    let evs = run(req).await;
    assert_eq!(statuses(&evs).last(), Some(&401), "{evs:?}");
    let got = evil.received_requests().await.unwrap();
    assert_eq!(got.len(), 1);
    assert_no_auth(&got);
}

#[tokio::test]
async fn sigv4_signs_again_on_a_same_origin_redirect() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/a"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/b"))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/b"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let evs = run(request(format!("{}/a", server.uri()), sigv4("s3", "T"))).await;
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[1].url.path(), "/b");
    assert!(hdr(&got[1], "authorization").starts_with("AWS4-HMAC-SHA256"));
    assert_ne!(hdr(&got[0], "authorization"), hdr(&got[1], "authorization"));
    assert_signature(&got[0], "s3", "T");
    assert_signature(&got[1], "s3", "T");
}

#[tokio::test]
async fn sigv4_non_s3_signs_the_real_hash_of_a_2_mib_file() {
    let file = std::env::temp_dir().join(format!("wisp-sigv4-big-{}.bin", std::process::id()));
    let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    std::fs::write(&file, &data).unwrap();
    // `signed_request` re-signs with SHA-256 of the received body for non-S3 services.
    let r = signed_request("execute-api", "", PreparedBody::File(file.clone())).await;
    let _ = std::fs::remove_file(&file);
    assert_eq!(r.body, data);
    assert!(r.headers.get("x-amz-content-sha256").is_none());
}

#[tokio::test]
async fn sigv4_s3_keeps_unsigned_payload_for_a_2_mib_file() {
    let file = std::env::temp_dir().join(format!("wisp-sigv4-s3big-{}.bin", std::process::id()));
    std::fs::write(&file, vec![3u8; 2 * 1024 * 1024]).unwrap();
    let r = signed_request("s3", "", PreparedBody::File(file.clone())).await;
    let _ = std::fs::remove_file(&file);
    assert_eq!(hdr(&r, "x-amz-content-sha256"), "UNSIGNED-PAYLOAD");
}

/// `/a` redirects with `status` to `/b`; `/b` answers 200.
async fn redirect_a_to_b(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/a"))
        .respond_with(ResponseTemplate::new(status).insert_header("location", "/b"))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/b"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_post_redirected_with_302_or_303_becomes_a_get_and_is_re_signed_over_no_body() {
    for status in [302u16, 303] {
        let server = redirect_a_to_b(status).await;
        let mut req = request(format!("{}/a", server.uri()), sigv4("execute-api", ""));
        req.method = Method::Post;
        req.body = PreparedBody::Bytes(b"payload".to_vec());
        req.headers = vec![
            Header {
                name: "Content-Type".into(),
                value: "text/plain".into(),
            },
            Header {
                name: "Content-Encoding".into(),
                value: "identity".into(),
            },
        ];
        let evs = run(req).await;
        assert!(
            matches!(evs.last(), Some(Event::Completed { .. })),
            "{evs:?}"
        );
        let got = server.received_requests().await.unwrap();
        assert_eq!(got.len(), 2, "{status}");
        assert_eq!(got[0].method.as_str(), "POST");
        assert_eq!(got[0].body, b"payload");
        assert_signature(&got[0], "execute-api", "");
        let second = &got[1];
        assert_eq!(second.method.as_str(), "GET", "{status}");
        assert!(second.body.is_empty());
        assert!(second.headers.get("content-type").is_none());
        assert!(second.headers.get("content-encoding").is_none());
        assert!(
            second
                .headers
                .get("content-length")
                .is_none_or(|v| v == "0")
        );
        assert!(
            hdr(second, "authorization").contains("SignedHeaders=host;x-amz-date"),
            "{}",
            hdr(second, "authorization")
        );
        assert_signature(second, "execute-api", "");
    }
}

#[tokio::test]
async fn exactly_one_authorization_header_is_sent_and_a_user_one_is_dropped() {
    let user = || {
        vec![Header {
            name: "Authorization".into(),
            value: "Bearer user-token".into(),
        }]
    };
    let count = |r: &wiremock::Request| r.headers.get_all("authorization").iter().count();

    let server = redirect_a_to_b(302).await;
    let mut req = request(format!("{}/a", server.uri()), sigv4("execute-api", ""));
    req.headers = user();
    run(req).await;
    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 2);
    for r in &got {
        assert_eq!(count(r), 1);
        assert!(hdr(r, "authorization").starts_with("AWS4-HMAC-SHA256"));
        assert_signature(r, "execute-api", "");
    }

    let server = digest_server(false).await;
    let mut req = request(server.uri(), digest());
    req.headers = user();
    run(req).await;
    let got = server.received_requests().await.unwrap();
    assert_eq!(got.len(), 2);
    assert!(got[0].headers.get("authorization").is_none());
    assert_eq!(count(&got[1]), 1);
    assert!(hdr(&got[1], "authorization").starts_with("Digest "));
}

#[tokio::test]
async fn digest_is_challenged_again_on_a_same_origin_redirect_hop() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path("/a"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/b"))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path("/b"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(401).insert_header("www-authenticate", CHALLENGE))
        .with_priority(2)
        .mount(&server)
        .await;
    let evs = run(request(format!("{}/a", server.uri()), digest())).await;
    assert_eq!(statuses(&evs), vec![200]);
    let got = server.received_requests().await.unwrap();
    let paths: Vec<&str> = got.iter().map(|r| r.url.path()).collect();
    assert_eq!(paths, ["/a", "/a", "/b", "/b"]);
    let has_auth: Vec<bool> = got
        .iter()
        .map(|r| r.headers.get("authorization").is_some())
        .collect();
    assert_eq!(has_auth, [false, true, false, true]);
    assert!(hdr(&got[3], "authorization").contains("uri=\"/b\""));
}

#[tokio::test]
async fn an_https_to_http_redirect_sends_no_engine_auth() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let plain = ok_server().await;
    let location = format!("{}/x", plain.uri());
    let ck =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        ck.signing_key.serialize_der(),
    ));
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
            let mut buf = [0u8; 8192];
            if s.read(&mut buf).is_err() {
                return;
            }
            let r = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = s.write_all(r.as_bytes());
            s.conn.send_close_notify();
            let _ = s.flush();
        }
    });
    let mut req = request(
        format!("https://127.0.0.1:{port}/a"),
        sigv4("s3", "TOKEN123"),
    );
    req.client.verify_tls = false;
    let evs = run(req).await;
    assert!(
        matches!(evs.last(), Some(Event::Completed { .. })),
        "{evs:?}"
    );
    let got = plain.received_requests().await.unwrap();
    assert_eq!(got.len(), 1);
    assert_no_auth(&got);
}
