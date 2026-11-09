//! Digest and SigV4 through the engine, against wiremock.

use wiremock::matchers::{header_exists, method};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wisp_core::auth::EngineAuth;
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
    reqs.remove(0)
}

fn hdr<'a>(r: &'a wiremock::Request, name: &str) -> &'a str {
    r.headers.get(name).unwrap().to_str().unwrap()
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
