use std::fs;
use std::time::{Duration, SystemTime};

use wisp_core::request::{
    ApiKeyLocation, Auth, Body, ClientAuth, KeyValue, Part, PartValue, Request, TextBody, Var,
};
use wisp_core::{ErrorKind, Header};
use wisp_storage::collection::CollectionId;
use wisp_storage::data_dir::DataDir;
use wisp_storage::history::{HistoryStore, NewEntry, Outcome};

fn cid(s: &str) -> CollectionId {
    CollectionId::parse(s).unwrap()
}

fn setup() -> (tempfile::TempDir, DataDir, HistoryStore) {
    let d = tempfile::tempdir().unwrap();
    let data = DataDir::at(d.path());
    let store = HistoryStore::open(&data, &cid("abcdef0123456789"));
    (d, data, store)
}

fn kv(n: &str, v: &str) -> KeyValue {
    KeyValue {
        name: n.into(),
        value: v.into(),
        enabled: true,
    }
}

fn new_entry(req: Request) -> NewEntry {
    NewEntry {
        at: SystemTime::UNIX_EPOCH + Duration::from_secs(10),
        request_path: Some("a.toml".into()),
        request: req,
        outcome: Outcome::Completed,
        status: Some(200),
        ttfb: Some(Duration::from_millis(5)),
        total: Some(Duration::from_millis(9)),
        dns: None,
        connect: None,
        size: Some(3),
        response_headers: vec![Header {
            name: "X".into(),
            value: "y".into(),
        }],
        environment: None,
        assertions: Vec::new(),
        delivery: None,
    }
}

fn plain(url: &str) -> NewEntry {
    new_entry(Request {
        url: url.into(),
        ..Request::default()
    })
}

fn bodies_dir(data: &DataDir) -> std::path::PathBuf {
    data.history_dir(&cid("abcdef0123456789")).join("bodies")
}

#[test]
fn redacts_auth_and_secret_headers() {
    let (_d, data, store) = setup();
    let req = Request {
        auth: Auth::Bearer {
            token: "abc".into(),
        },
        headers: vec![kv("Authorization", "secret"), kv("X-Api-Key", "{{key}}")],
        ..Request::default()
    };
    let e = store.append(new_entry(req), Some(b"hi")).unwrap();
    let raw = fs::read_to_string(bodies_dir(&data).join(format!("{}.req.json", e.id))).unwrap();
    assert!(!raw.contains("abc") && !raw.contains("secret"));
    assert!(raw.contains("{{key}}"));
    assert!(e.redacted.contains(&"auth.token".to_string()));
    assert!(e.redacted.contains(&"Authorization".to_string()));
    assert!(!e.redacted.contains(&"X-Api-Key".to_string()));
    let loaded = store.load(&e.id).unwrap();
    assert_eq!(loaded.request.headers[1].value, "{{key}}");
    assert_eq!(loaded.response_headers.len(), 1);
}

#[test]
fn redacts_the_api_key_auth_header_name() {
    let (_d, _data, store) = setup();
    let req = Request {
        auth: Auth::ApiKey {
            name: "X-Custom".into(),
            value: "v".into(),
            location: ApiKeyLocation::Header,
        },
        headers: vec![kv("x-custom", "leak")],
        ..Request::default()
    };
    let e = store.append(new_entry(req), None).unwrap();
    assert!(e.redacted.contains(&"x-custom".to_string()));
    assert_eq!(store.load(&e.id).unwrap().request.headers[0].value, "");
}

#[test]
fn caps_the_body_at_one_mib() {
    let (_d, _data, store) = setup();
    let big = vec![7u8; 2 * 1024 * 1024];
    let e = store.append(plain("/a"), Some(&big)).unwrap();
    assert!(e.body_truncated && e.body_stored);
    assert_eq!(store.load(&e.id).unwrap().body.len(), 1_048_576);
    let e = store.append(plain("/b"), Some(&[1u8; 10])).unwrap();
    assert!(!e.body_truncated);
    assert_eq!(store.load(&e.id).unwrap().body.len(), 10);
}

#[test]
fn keeps_500_newest_and_removes_old_files() {
    let (_d, data, store) = setup();
    let mut ids = Vec::new();
    for i in 0..505 {
        ids.push(
            store
                .append(plain(&format!("/{i}")), Some(b"x"))
                .unwrap()
                .id,
        );
    }
    let list = store.list().unwrap();
    assert_eq!(list.len(), 500);
    assert_eq!(list[0].url, "/504");
    assert_eq!(list[499].url, "/5");
    for id in &ids[..5] {
        assert!(!bodies_dir(&data).join(format!("{id}.bin")).exists());
        assert!(!bodies_dir(&data).join(format!("{id}.req.json")).exists());
    }
    assert!(bodies_dir(&data).join(format!("{}.bin", ids[5])).exists());
}

#[test]
fn evicts_oldest_over_the_byte_cap() {
    let (_d, _data, store) = setup();
    let store = store.with_caps(500, 3000);
    for i in 0..5 {
        store
            .append(plain(&format!("/{i}")), Some(&[0u8; 1000]))
            .unwrap();
    }
    let list = store.list().unwrap();
    assert!(list.len() < 5 && !list.is_empty());
    assert_eq!(list[0].url, "/4");
}

#[test]
fn failed_entry_is_kept_without_a_body() {
    let (_d, data, store) = setup();
    let mut n = plain("/f");
    n.outcome = Outcome::Failed {
        kind: ErrorKind::Timeout,
    };
    n.status = None;
    let e = store.append(n, None).unwrap();
    assert!(!e.body_stored);
    let list = store.list().unwrap();
    assert_eq!(
        list[0].outcome,
        Outcome::Failed {
            kind: ErrorKind::Timeout
        }
    );
    assert!(!bodies_dir(&data).join(format!("{}.bin", e.id)).exists());
    assert!(store.load(&e.id).unwrap().body.is_empty());
}

#[test]
fn skips_garbage_and_torn_lines_and_repairs_before_append() {
    let (d, data, store) = setup();
    store.append(plain("/1"), None).unwrap();
    let index = data
        .history_dir(&cid("abcdef0123456789"))
        .join("index.jsonl");
    let mut text = fs::read(&index).unwrap();
    text.extend_from_slice(b"garbage\n\xff\xfe\n{\"id\":");
    fs::write(&index, text).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    store.append(plain("/2"), None).unwrap();
    let urls: Vec<_> = store.list().unwrap().into_iter().map(|e| e.url).collect();
    assert_eq!(urls, vec!["/2", "/1"]);
    drop(d);
}

#[test]
fn clear_removes_only_this_collection() {
    let (_d, data, store) = setup();
    let other = HistoryStore::open(&data, &cid("fedcba9876543210"));
    store.append(plain("/1"), None).unwrap();
    other.append(plain("/2"), None).unwrap();
    store.clear().unwrap();
    assert!(!data.history_dir(&cid("abcdef0123456789")).exists());
    assert_eq!(other.list().unwrap().len(), 1);
    store.clear().unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn modes_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, data, store) = setup();
    let e = store.append(plain("/1"), Some(b"b")).unwrap();
    let root = data.history_dir(&cid("abcdef0123456789"));
    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&root), 0o700);
    assert_eq!(mode(&root.join("bodies")), 0o700);
    assert_eq!(mode(&root.join("index.jsonl")), 0o600);
    assert_eq!(mode(&root.join(format!("bodies/{}.bin", e.id))), 0o600);
    assert_eq!(mode(&root.join(format!("bodies/{}.req.json", e.id))), 0o600);
    for i in 0..3 {
        store.append(plain(&format!("/{i}")), None).unwrap();
    }
    let store = store.with_caps(1, u64::MAX);
    store.append(plain("/z"), None).unwrap();
    assert_eq!(mode(&root.join("index.jsonl")), 0o600);
}

#[test]
fn unknown_and_missing_fields_parse() {
    let (_d, data, store) = setup();
    let dir = data.history_dir(&cid("abcdef0123456789"));
    fs::create_dir_all(&dir).unwrap();
    let a = format!(
        r#"{{"id":"{}","at":1,"method":"Get","url":"/a","outcome":"Completed","future":{{"x":1}}}}"#,
        "a".repeat(32)
    );
    let b = format!(
        r#"{{"id":"{}","at":2,"method":"Get","url":"/b","outcome":"Cancelled","status":200,"size":1}}"#,
        "b".repeat(32)
    );
    fs::write(dir.join("index.jsonl"), format!("{a}\n{b}\n")).unwrap();
    let list = store.list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].ttfb_ms, None);
    assert!(!list[0].body_stored);
}

#[test]
fn open_does_no_io() {
    let (d, _data, store) = setup();
    assert!(store.list().unwrap().is_empty());
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
}

#[test]
fn load_rejects_a_path_like_id() {
    let (_d, _data, store) = setup();
    assert!(store.load("../../etc/passwd").is_err());
}

#[test]
fn index_line_has_the_documented_fields() {
    let (_d, data, store) = setup();
    store.append(plain("{{base}}/x"), Some(b"abc")).unwrap();
    let raw = fs::read_to_string(
        data.history_dir(&cid("abcdef0123456789"))
            .join("index.jsonl"),
    )
    .unwrap();
    for f in [
        "\"at\":10000",
        "\"ttfb_ms\":5",
        "\"total_ms\":9",
        "\"body_stored\":true",
        "\"url\":\"{{base}}/x\"",
    ] {
        assert!(raw.contains(f), "{f} in {raw}");
    }
}

#[test]
fn redacts_set_cookie_values_in_response_headers() {
    let (_d, data, store) = setup();
    let mut n = plain("http://x");
    n.response_headers = vec![
        Header {
            name: "Set-Cookie".into(),
            value: "sid=abc123; Path=/; HttpOnly".into(),
        },
        Header {
            name: "set-cookie".into(),
            value: "theme=dark".into(),
        },
    ];
    let e = store.append(n, None).unwrap();
    let raw = fs::read_to_string(bodies_dir(&data).join(format!("{}.req.json", e.id))).unwrap();
    assert!(!raw.contains("abc123") && !raw.contains("dark"));
    let h = store.load(&e.id).unwrap().response_headers;
    assert_eq!(h[0].value, "sid=; Path=/; HttpOnly");
    assert_eq!(h[1].value, "theme=");
    assert!(
        store
            .load(&e.id)
            .unwrap()
            .response_redacted
            .contains(&"Set-Cookie".to_string())
    );
}

#[test]
fn concurrent_appends_leave_no_orphans() {
    let (_d, data, store) = setup();
    let store = store.with_caps(5, u64::MAX);
    let mut hs = Vec::new();
    for _ in 0..8 {
        let s = HistoryStore::open(&data, &cid("abcdef0123456789")).with_caps(5, u64::MAX);
        hs.push(std::thread::spawn(move || {
            for _ in 0..10 {
                s.append(plain("http://x"), Some(b"b")).unwrap();
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
    let n = store.list().unwrap().len();
    assert_eq!(n, 5);
    let files = fs::read_dir(bodies_dir(&data)).unwrap().count();
    assert_eq!(files, 2 * n);
}

fn raw_files(data: &DataDir) -> String {
    let dir = data.history_dir(&cid("abcdef0123456789"));
    let mut all = fs::read_to_string(dir.join("index.jsonl")).unwrap_or_default();
    for f in fs::read_dir(dir.join("bodies")).unwrap() {
        let p = f.unwrap().path();
        if p.to_string_lossy().ends_with(".req.json") {
            all.push_str(&fs::read_to_string(p).unwrap());
        }
    }
    all
}

#[test]
fn redacts_secrets_in_url_params_vars_and_bodies() {
    let (_d, data, store) = setup();
    let json = r#"{"user":"bob","password":"JSONPW","nested":{"api_token":"JSONTOK","ok":"fine","ref_token":"{{t}}"},"list":[{"secret":"JSONSEC"}],"pin_key":1234}"#;
    let req = Request {
        url: "https://uuser:UPASS@host/p?api_key=QKEY&x=1&token={{t}}#f".into(),
        params: vec![kv("access_token", "PARAMTOK"), kv("page", "2")],
        vars: vec![
            Var {
                name: "client_secret".into(),
                value: "VARSEC".into(),
            },
            Var {
                name: "region".into(),
                value: "eu".into(),
            },
        ],
        body: Body::Json(TextBody {
            content: json.into(),
            sidecar: None,
        }),
        ..Request::default()
    };
    let e = store.append(new_entry(req), None).unwrap();
    let raw = raw_files(&data);
    for lit in [
        "UPASS", "uuser", "QKEY", "PARAMTOK", "VARSEC", "JSONPW", "JSONTOK", "JSONSEC", "1234",
    ] {
        assert!(!raw.contains(lit), "{lit} leaked");
    }
    for keep in ["{{t}}", "bob", "fine", "x=1", "page", "eu", "host/p"] {
        assert!(raw.contains(keep), "{keep} lost");
    }
    assert_eq!(e.url, "https://:@host/p?api_key=&x=1&token={{t}}#f");
    for n in [
        "url.username",
        "url.password",
        "query.api_key",
        "params.access_token",
        "vars.client_secret",
    ] {
        assert!(e.redacted.contains(&n.to_string()), "{n} not recorded");
    }
}

#[test]
fn redacts_secrets_in_form_and_multipart_bodies() {
    let (_d, data, store) = setup();
    let form = Request {
        body: Body::Form(vec![kv("password", "FORMPW"), kv("name", "n")]),
        ..Request::default()
    };
    store.append(new_entry(form), None).unwrap();
    let multi = Request {
        body: Body::Multipart(vec![
            Part {
                name: "client_secret".into(),
                value: PartValue::Text("MPSEC".into()),
                enabled: true,
            },
            Part {
                name: "title".into(),
                value: PartValue::Text("hello".into()),
                enabled: true,
            },
        ]),
        ..Request::default()
    };
    store.append(new_entry(multi), None).unwrap();
    let raw = raw_files(&data);
    assert!(!raw.contains("FORMPW") && !raw.contains("MPSEC"));
    assert!(raw.contains("hello") && raw.contains("\"n\""));
}

#[test]
fn scheme_less_userinfo_is_redacted() {
    let (_d, data, store) = setup();
    store.append(plain("bob:SCHEMELESS@host/x"), None).unwrap();
    assert!(!raw_files(&data).contains("SCHEMELESS"));
}

#[test]
fn header_name_heuristic_table() {
    let (_d, _data, store) = setup();
    let secret = [
        "Authorization",
        "PROXY-AUTHORIZATION",
        "X-Auth-Token",
        "Private-Token",
        "Api-Key",
        "x-api-key",
        "X-Amz-Security-Token",
        "Cookie",
        "X-Session-Id",
        "X-Client-Secret",
        "X-Password",
        "X-Passwd",
        "X-Credentials",
        "X-Signature",
        "x-Amz-SIGNATURE",
    ];
    let plain_names = ["Accept", "Content-Type", "User-Agent", "X-Request-Id"];
    let headers: Vec<KeyValue> = secret
        .iter()
        .chain(plain_names.iter())
        .map(|n| kv(n, "LITERAL"))
        .collect();
    let e = store
        .append(
            new_entry(Request {
                headers,
                ..Request::default()
            }),
            None,
        )
        .unwrap();
    let loaded = store.load(&e.id).unwrap().request.headers;
    for h in &loaded {
        let expect = if secret.contains(&h.name.as_str()) {
            ""
        } else {
            "LITERAL"
        };
        assert_eq!(h.value, expect, "{}", h.name);
    }
    let kept = Request {
        headers: vec![kv("X-Auth-Token", "{{tok}}")],
        ..Request::default()
    };
    let e = store.append(new_entry(kept), None).unwrap();
    assert_eq!(
        store.load(&e.id).unwrap().request.headers[0].value,
        "{{tok}}"
    );
}

#[test]
fn next_append_removes_orphan_bodies() {
    let (_d, data, store) = setup();
    store.append(plain("/a"), Some(b"x")).unwrap();
    let orphan = bodies_dir(&data).join(format!("{}.bin", "a".repeat(32)));
    fs::write(&orphan, b"orphan").unwrap();
    store.append(plain("/b"), Some(b"y")).unwrap();
    assert!(!orphan.exists());
    assert_eq!(fs::read_dir(bodies_dir(&data)).unwrap().count(), 4);
}

#[test]
fn file_lock_serialises_appends() {
    let (_d, data, store) = setup();
    store.append(plain("/a"), None).unwrap();
    let lock_path = data
        .history_dir(&cid("abcdef0123456789"))
        .with_extension("lock");
    // Windows `File::lock` needs read or write access, not append-only.
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    held.lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let s = HistoryStore::open(&data, &cid("abcdef0123456789"));
    let h = std::thread::spawn(move || {
        s.append(plain("/b"), None).unwrap();
        tx.send(()).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    held.unlock().unwrap();
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    h.join().unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn eviction_failure_still_returns_the_entry() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, data, store) = setup();
    let store = store.with_caps(1, u64::MAX);
    store.append(plain("/a"), None).unwrap();
    let dir = data.history_dir(&cid("abcdef0123456789"));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let r = store.append(plain("/b"), None);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let e = r.unwrap();
    assert!(store.list().unwrap().iter().any(|x| x.id == e.id));
}

fn json_req(content: &str) -> Request {
    Request {
        body: Body::Json(TextBody {
            content: content.into(),
            sidecar: None,
        }),
        ..Request::default()
    }
}

fn raw_req(data: &DataDir, id: &str) -> String {
    fs::read_to_string(bodies_dir(data).join(format!("{id}.req.json"))).unwrap()
}

#[test]
fn templated_json_body_is_redacted_and_refs_survive() {
    let (_d, data, store) = setup();
    let e = store
        .append(
            new_entry(json_req(r#"{"password":"HUNTER2","n":{{n}},"s":"a{{m}}"}"#)),
            None,
        )
        .unwrap();
    let raw = raw_req(&data, &e.id);
    assert!(!raw.contains("HUNTER2"));
    assert!(raw.contains(r#"\"n\":{{n}},"#), "{raw}");
    assert!(raw.contains("a{{m}}"), "{raw}");
    assert!(e.redacted.contains(&"body.password".to_string()));
}

#[test]
fn unparseable_json_body_is_not_stored() {
    let (_d, data, store) = setup();
    let e = store
        .append(new_entry(json_req(r#"{"password": HUNTER2 oops"#)), None)
        .unwrap();
    assert!(!raw_req(&data, &e.id).contains("HUNTER2"));
    assert!(e.redacted.contains(&"body".to_string()));
}

#[test]
fn plain_json_body_without_credentials_is_unchanged() {
    let (_d, data, store) = setup();
    let e = store
        .append(new_entry(json_req(r#"{"a": 1,  "b":[true]}"#)), None)
        .unwrap();
    assert!(raw_req(&data, &e.id).contains(r#"{\"a\": 1,  \"b\":[true]}"#));
    assert!(!e.redacted.contains(&"body".to_string()));
}

fn stored_text(req_body: Body) -> (String, Vec<String>) {
    let (_d, data, store) = setup();
    let e = store
        .append(
            new_entry(Request {
                body: req_body,
                ..Request::default()
            }),
            None,
        )
        .unwrap();
    (raw_files(&data), e.redacted)
}

fn text_body(s: &str) -> TextBody {
    TextBody {
        content: s.into(),
        sidecar: None,
    }
}

#[test]
fn unclosed_ref_in_json_body_fails_closed() {
    let (raw, redacted) = stored_text(Body::Json(text_body(
        r#"{"a":{{x,"password":"HUNTER2","b":"}}"}"#,
    )));
    assert!(!raw.contains("HUNTER2"));
    assert!(redacted.contains(&"body".to_string()));
    let (raw, redacted) = stored_text(Body::Json(text_body(
        r#"{"a":{{x,"password":"HUNTER2"}},"b":1}"#,
    )));
    assert!(!raw.contains("HUNTER2"));
    assert!(redacted.contains(&"body".to_string()));
}

#[test]
fn text_and_xml_bodies_with_secret_words_are_dropped() {
    let (raw, redacted) = stored_text(Body::Text(text_body("password=HUNTER2")));
    assert!(!raw.contains("HUNTER2") && redacted.contains(&"body".to_string()));
    let (raw, redacted) = stored_text(Body::Xml(text_body("<token>HUNTER2</token>")));
    assert!(!raw.contains("HUNTER2") && redacted.contains(&"body".to_string()));
    let (raw, redacted) = stored_text(Body::Text(text_body("hello plain world")));
    assert!(raw.contains("hello plain world") && redacted.is_empty());
}

#[test]
fn sweep_keeps_entries_with_unparseable_index_lines() {
    let (_d, data, store) = setup();
    let first = store.append(plain("/1"), Some(b"keepme")).unwrap();
    let dir = data.history_dir(&cid("abcdef0123456789"));
    let index = dir.join("index.jsonl");
    let line = format!("{{\"id\":\"{}\",\"method\":{{\"future\":1}}}}", first.id);
    fs::write(&index, format!("{line}\n")).unwrap();
    let body = fs::read_dir(dir.join("bodies"))
        .unwrap()
        .flatten()
        .map(|f| f.path())
        .collect::<Vec<_>>();
    assert!(!body.is_empty());
    store.append(plain("/2"), None).unwrap();
    for p in &body {
        assert!(p.exists(), "{p:?} swept");
    }
    assert!(fs::read_to_string(&index).unwrap().contains(&line));
}

#[test]
fn dollar_builtin_refs_keep_a_json_body() {
    let (_d, data, store) = setup();
    let req = Request {
        body: Body::Json(TextBody {
            content: r#"{"id":{{$uuid}},"at":{{$timestamp}},"password":"HUNTER2"}"#.into(),
            sidecar: None,
        }),
        ..Request::default()
    };
    store.append(new_entry(req), None).unwrap();
    let raw = raw_files(&data);
    assert!(!raw.contains("HUNTER2"), "secret leaked");
    assert!(raw.contains("{{$uuid}}"), "uuid ref lost");
    assert!(raw.contains("{{$timestamp}}"), "timestamp ref lost");
    assert!(!raw.contains("\"{{$uuid}}\""), "ref got quoted");
    assert!(!raw.contains("\"{{$timestamp}}\""), "ref got quoted");
}

#[test]
fn redacts_oauth2_client_secret_but_keeps_var_refs() {
    let (_d, data, store) = setup();
    let req = Request {
        auth: Auth::OAuth2AuthCode {
            auth_url: "https://idp.test/auth".into(),
            token_url: "https://idp.test/token".into(),
            client_id: "cid-literal".into(),
            client_secret: "SECRETVAL".into(),
            scope: "read".into(),
            client_auth: ClientAuth::Basic,
            redirect_port: 0,
        },
        ..Request::default()
    };
    let e = store.append(new_entry(req), None).unwrap();
    for entry in fs::read_dir(bodies_dir(&data)).unwrap() {
        let raw = fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("SECRETVAL"));
    }
    let index = fs::read_to_string(
        data.history_dir(&cid("abcdef0123456789"))
            .join("index.jsonl"),
    )
    .unwrap();
    assert!(!index.contains("SECRETVAL"));
    assert!(e.redacted.contains(&"auth.client_secret".to_string()));
    let loaded = store.load(&e.id).unwrap();
    match loaded.request.auth {
        Auth::OAuth2AuthCode {
            client_secret,
            token_url,
            client_id,
            ..
        } => {
            assert_eq!(client_secret, "");
            assert_eq!(token_url, "https://idp.test/token");
            assert_eq!(client_id, "cid-literal");
            assert_eq!(e.redacted, vec!["auth.client_secret".to_string()]);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn timings_roundtrip_and_old_index_lines_load_as_none() {
    let (_d, data, store) = setup();
    let mut n = plain("/t");
    n.dns = Some(Duration::from_millis(3));
    n.connect = Some(Duration::from_millis(7));
    let e = store.append(n, None).unwrap();
    assert_eq!((e.dns_ms, e.connect_ms), (Some(3), Some(7)));
    let listed = store.list().unwrap();
    assert_eq!((listed[0].dns_ms, listed[0].connect_ms), (Some(3), Some(7)));

    let dir = data.history_dir(&cid("abcdef0123456789"));
    let old = format!(
        r#"{{"id":"{}","at":1,"method":"Get","url":"/a","outcome":"Completed","total_ms":4}}"#,
        "c".repeat(32)
    );
    fs::write(dir.join("index.jsonl"), format!("{old}\n")).unwrap();
    let listed = store.list().unwrap();
    assert_eq!((listed[0].dns_ms, listed[0].connect_ms), (None, None));
}

fn hdr(n: &str, v: &str) -> Header {
    Header {
        name: n.into(),
        value: v.into(),
    }
}

#[test]
fn credential_named_response_headers_are_emptied() {
    let (_d, data, store) = setup();
    let mut n = plain("http://x");
    n.response_headers = vec![
        hdr("X-Auth-Token", "SECRETVAL"),
        hdr("X-Subject-Token", "SECRETVAL"),
        hdr("Content-Type", "text/plain"),
    ];
    let e = store.append(n, None).unwrap();
    assert!(!raw_files(&data).contains("SECRETVAL"));
    assert!(
        store
            .load(&e.id)
            .unwrap()
            .response_redacted
            .contains(&"X-Auth-Token".to_string())
    );
    let h = store.load(&e.id).unwrap().response_headers;
    assert_eq!(h[0].value, "");
    assert_eq!(h[2].value, "text/plain");
}

#[test]
fn set_cookie_without_name_is_emptied() {
    let (_d, data, store) = setup();
    let mut n = plain("http://x");
    n.response_headers = vec![
        hdr("Set-Cookie", "SECRETVAL; Path=/"),
        hdr("Set-Cookie", "SECRETVAL"),
    ];
    let e = store.append(n, None).unwrap();
    assert!(!raw_files(&data).contains("SECRETVAL"));
    let h = store.load(&e.id).unwrap().response_headers;
    assert_eq!(h[0].value, "; Path=/");
}

#[test]
fn path_vars_are_redacted() {
    let (_d, data, store) = setup();
    let req = Request {
        path: vec![
            Var {
                name: "token".into(),
                value: "SECRETVAL".into(),
            },
            Var {
                name: "id".into(),
                value: "42".into(),
            },
            Var {
                name: "api_key".into(),
                value: "{{k}}".into(),
            },
        ],
        ..Request::default()
    };
    let e = store.append(new_entry(req), None).unwrap();
    let raw = raw_files(&data);
    assert!(!raw.contains("SECRETVAL") && raw.contains("42"));
    assert!(e.redacted.contains(&"path.token".to_string()));
    assert!(!e.redacted.contains(&"path.api_key".to_string()));
}

#[test]
fn json_body_with_duplicate_keys_is_dropped() {
    let (_d, data, store) = setup();
    for body in [
        r#"{"token":"SECRETVAL","token":"{{t}}"}"#,
        r#"{"a":[{"x":"SECRETVAL","x":1}]}"#,
    ] {
        let req = Request {
            body: Body::Json(TextBody {
                content: body.into(),
                ..TextBody::default()
            }),
            ..Request::default()
        };
        let e = store.append(new_entry(req), None).unwrap();
        assert!(!raw_files(&data).contains("SECRETVAL"));
        assert!(e.redacted.contains(&"body".to_string()));
    }
}

#[test]
fn max_json_body_equals_body_cap() {
    assert_eq!(
        wisp_core::assert::MAX_JSON_BODY,
        wisp_storage::history::BODY_CAP
    );
}

#[test]
fn assertion_record_json_shape_is_pinned() {
    use wisp_core::assert::{Assertion, AssertionResult, Expected, Op, Verdict};
    use wisp_storage::history::AssertionRecord;
    let a = Assertion::Status {
        op: Op::Equals,
        expected: Expected::Int(200),
    };
    let r = AssertionResult {
        label: a.label(),
        verdict: Verdict::Pass,
    };
    let rec = AssertionRecord::from_result(&a, &r);
    let json = serde_json::to_string(&rec).unwrap();
    assert_eq!(
        json,
        format!(
            r#"{{"label":"status equals","hash":"{}","state":"pass"}}"#,
            a.fingerprint()
        )
    );
    assert_eq!(rec.hash.len(), 16);
    let failed = AssertionResult {
        label: a.label(),
        verdict: Verdict::Fail {
            actual: Some("404".into()),
        },
    };
    assert!(
        serde_json::to_string(&AssertionRecord::from_result(&a, &failed))
            .unwrap()
            .ends_with(r#""state":"fail"}"#)
    );
}

#[test]
fn stored_request_carries_no_assertions_or_their_values() {
    use wisp_core::assert::{Assertion, Expected, Op};
    let (_d, data, store) = setup();
    let req = Request {
        url: "/a".into(),
        headers: vec![kv("X-Trace", "plain")],
        assertions: vec![
            Assertion::Json {
                path: "$.token".into(),
                op: Op::Equals,
                expected: Expected::Str("CANARY-EXPECTED".into()),
            },
            Assertion::Header {
                name: "x-trace".into(),
                op: Op::Equals,
                expected: Expected::Str("CANARY-HEADER-EXPECTED".into()),
            },
        ],
        ..Request::default()
    };
    let e = store.append(new_entry(req), Some(b"hi")).unwrap();
    assert!(store.load(&e.id).unwrap().request.assertions.is_empty());
    let root = data.history_dir(&cid("abcdef0123456789"));
    let index = fs::read_to_string(root.join("index.jsonl")).unwrap();
    let stored = fs::read_to_string(bodies_dir(&data).join(format!("{}.req.json", e.id))).unwrap();
    for canary in ["CANARY-EXPECTED", "CANARY-HEADER-EXPECTED"] {
        assert!(
            !index.contains(canary) && !stored.contains(canary),
            "{canary}"
        );
    }
}

#[test]
fn entry_from_0_2_0_line_loads_without_provenance() {
    let line = r#"{"id":"e1","at":5,"method":"Get","url":"u","outcome":"Completed","status":200,"body_stored":false,"body_truncated":false,"redacted":[]}"#;
    let e: wisp_storage::history::HistoryEntry = serde_json::from_str(line).unwrap();
    assert_eq!(e.environment, None);
    assert!(e.assertions.is_empty());
}

fn rec(
    label: &str,
    state: wisp_storage::history::AssertionState,
) -> wisp_storage::history::AssertionRecord {
    wisp_storage::history::AssertionRecord {
        label: label.into(),
        hash: "0123456789abcdef".into(),
        state,
    }
}

#[test]
fn provenance_round_trips() {
    use wisp_storage::history::AssertionState::*;
    let (_d, _data, store) = setup();
    let mut n = plain("http://x/");
    n.environment = Some("staging".into());
    n.assertions = vec![rec("a", Pass), rec("b", Fail), rec("c", Skipped)];
    let written = store.append(n, None).unwrap();
    let read = store.list().unwrap();
    assert_eq!(read, vec![written.clone()]);
    assert_eq!(read[0].environment.as_deref(), Some("staging"));
    assert_eq!(read[0].assertions.len(), 3);
}

#[test]
fn provenance_canary_values_never_stored() {
    use wisp_core::assert::{Assertion, AssertionResult, Expected, Op, Verdict};
    use wisp_storage::history::AssertionRecord;
    let (_d, data, store) = setup();
    let a = Assertion::Header {
        name: "X-Check".into(),
        op: Op::Equals,
        expected: Expected::Str("EXPECTED-CANARY".into()),
    };
    let status = Assertion::Status {
        op: Op::Equals,
        expected: Expected::Int(424242),
    };
    let r = AssertionResult {
        label: a.label(),
        verdict: Verdict::Fail {
            actual: Some("ACTUAL-CANARY".into()),
        },
    };
    let rs = AssertionResult {
        label: status.label(),
        verdict: Verdict::Fail {
            actual: Some("STATUS-ACTUAL-CANARY".into()),
        },
    };
    let mut n = new_entry(Request {
        url: "http://x/".into(),
        assertions: vec![a.clone(), status.clone()],
        ..Request::default()
    });
    n.assertions = vec![
        AssertionRecord::from_result(&a, &r),
        AssertionRecord::from_result(&status, &rs),
    ];
    n.environment = Some("staging".into());
    store.append(n, None).unwrap();
    let dir = data.history_dir(&cid("abcdef0123456789"));
    for d in [dir.clone(), dir.join("bodies")] {
        for f in std::fs::read_dir(&d).unwrap() {
            let p = f.unwrap().path();
            if p.is_file() {
                let t = std::fs::read_to_string(&p).unwrap();
                for c in [
                    "424242",
                    "EXPECTED-CANARY",
                    "ACTUAL-CANARY",
                    "STATUS-ACTUAL-CANARY",
                ] {
                    assert!(!t.contains(c), "{c} in {p:?}");
                }
            }
        }
    }
}

fn hist_dir(data: &DataDir) -> std::path::PathBuf {
    data.history_dir(&cid("abcdef0123456789"))
}

#[test]
fn evict_keeps_files_of_an_id_that_survives() {
    let (_d, data, store) = setup();
    let store = store.with_caps(2, u64::MAX);
    let e = store.append(plain("http://a/"), Some(b"hi")).unwrap();
    let index = hist_dir(&data).join("index.jsonl");
    let line = fs::read_to_string(&index).unwrap();
    // Oldest and newest share an id; a middle line forces the oldest out.
    let mid = line.replace(&e.id, &"1".repeat(32));
    fs::write(&index, format!("{line}{mid}{line}")).unwrap();
    store.append(plain("http://b/"), None).unwrap();
    assert!(store.load(&e.id).is_ok());
    assert!(bodies_dir(&data).join(format!("{}.bin", e.id)).exists());
}

#[test]
fn append_times_out_when_the_lock_is_held() {
    let (_d, data, store) = setup();
    let store = store.with_lock_wait(Duration::from_millis(200));
    store.append(plain("http://a/"), None).unwrap();
    let lock = hist_dir(&data).with_extension("lock");
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    f.lock().unwrap();
    let index = hist_dir(&data).join("index.jsonl");
    let before = fs::read(&index).unwrap();
    let err = store.append(plain("http://b/"), None).unwrap_err();
    assert!(err.to_string().contains("timed out"), "{err}");
    assert_eq!(fs::read(&index).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn append_refuses_a_symlinked_lock() {
    let (d, data, store) = setup();
    let target = d.path().join("outside");
    fs::write(&target, b"keep").unwrap();
    let mtime = fs::metadata(&target).unwrap().modified().unwrap();
    let lock = hist_dir(&data).with_extension("lock");
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &lock).unwrap();
    assert!(store.append(plain("http://a/"), None).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), mtime);
}

#[test]
fn clear_and_append_race_leaves_no_orphans() {
    let (_d, data, store) = setup();
    for _ in 0..10 {
        let a = store.clone();
        let b = store.clone();
        let t1 = std::thread::spawn(move || {
            for _ in 0..5 {
                let _ = a.append(plain("http://a/"), Some(b"x"));
            }
        });
        let t2 = std::thread::spawn(move || {
            for _ in 0..5 {
                let _ = b.clear();
            }
        });
        t1.join().unwrap();
        t2.join().unwrap();
        let listed: Vec<String> = store.list().unwrap().into_iter().map(|e| e.id).collect();
        if let Ok(rd) = fs::read_dir(bodies_dir(&data)) {
            for f in rd.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let stem = name.split('.').next().unwrap().to_string();
                assert!(listed.contains(&stem), "orphan {name}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn load_works_for_listed_entries_when_the_index_rewrite_fails() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, data, store) = setup();
    let store = store.with_caps(2, u64::MAX);
    store.append(plain("http://a/"), Some(b"1")).unwrap();
    store.append(plain("http://b/"), Some(b"2")).unwrap();
    let dir = hist_dir(&data);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let r = store.append(plain("http://c/"), Some(b"3"));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    r.unwrap();
    for e in store.list().unwrap() {
        store.load(&e.id).unwrap();
    }
}

#[test]
fn location_headers_are_redacted() {
    let (_d, data, store) = setup();
    let mut n = plain("http://a/");
    n.response_headers = vec![Header {
        name: "Location".into(),
        value: "http://user:pw123@h/cb?access_token=tok789&x=1".into(),
    }];
    let e = store.append(n, None).unwrap();
    let raw = fs::read_to_string(bodies_dir(&data).join(format!("{}.req.json", e.id))).unwrap();
    assert!(!raw.contains("tok789") && !raw.contains("pw123") && !raw.contains("user:"));
    assert!(raw.contains("x=1"));
}

fn all_text(dir: &std::path::Path, out: &mut String) {
    for f in fs::read_dir(dir).unwrap().flatten() {
        let p = f.path();
        if p.is_dir() {
            all_text(&p, out);
        } else {
            out.push_str(&String::from_utf8_lossy(&fs::read(&p).unwrap()));
        }
    }
}

#[test]
fn open_scrubs_old_entries_once() {
    let (_d, data, store) = setup();
    let e = store.append(plain("http://a/"), Some(b"b")).unwrap();
    let dir = hist_dir(&data);
    let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
    v["request"]["headers"] = serde_json::json!([
        {"name": "X-Api-Key", "value": "hdrsecret1", "enabled": true}
    ]);
    v["response_headers"] = serde_json::json!([
        {"name": "Location", "value": "http://h/cb?token=locsecret2"}
    ]);
    fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
    let index = dir.join("index.jsonl");
    let line = fs::read_to_string(&index)
        .unwrap()
        .replace("http://a/", "http://a/?api_key=idxsecret3");
    fs::write(&index, line).unwrap();

    let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
    let mut text = String::new();
    all_text(&dir, &mut text);
    for s in ["hdrsecret1", "locsecret2", "idxsecret3"] {
        assert!(!text.contains(s), "{s} survived");
    }
    let m = |p: &std::path::Path| fs::metadata(p).unwrap().modified().unwrap();
    let (mi, mr) = (m(&index), m(&req));
    std::thread::sleep(Duration::from_millis(20));
    let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
    assert_eq!((m(&index), m(&req)), (mi, mr));
}

const SR_HEADERS: [(&str, &str); 22] = [
    ("Location", "//alice:secret@example.com/path"),
    ("Location", "\\\\alice:secret@example.com/path"),
    ("Location", "/\\alice:secret@example.com/path"),
    ("Content-Location", "//alice:secret@example.com/path"),
    ("Refresh", "0; url=//alice:secret@h/"),
    ("Refresh", "0;URL='//alice:secret@h'"),
    ("Refresh", "0; url=\"//alice:secret@h/\""),
    ("Refresh", "0; URL = //alice:secret@h"),
    ("Refresh", "0; url= //alice:secret@h"),
    ("Refresh", "0;//alice:secret@h"),
    ("Refresh", "5, https://alice:secret@h/x"),
    ("Refresh", "0 https://alice:secret@h/"),
    ("Refresh", "0 url=https://alice:secret@h/"),
    ("Refresh", "0 //alice:secret@h/"),
    ("Refresh", "0 url=https://alice:secret@h/a;b"),
    ("Refresh", "0;url='https://h/' https://alice:secret@h2/"),
    ("Refresh", "0; url=a; url=//alice:secret@h"),
    ("Refresh", "0; url //alice:secret@h"),
    ("Refresh", "0 \\\\alice:secret@h/"),
    ("Refresh", "0; url=/\\alice:secret@h"),
    ("Refresh", "0; url=https:/alice:secret@h/"),
    ("Refresh", "0, url = 'https://alice:secret@h/x'"),
];

#[test]
fn refresh_without_credentials_is_unchanged() {
    for value in [
        "5",
        "0; url=https://h/a;b?x=a@b",
        "3, URL='//h/p'",
        "0 https://h/",
    ] {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: "Refresh".into(),
            value: value.into(),
        }];
        store.append(ne, Some(b"b")).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(text.contains(value), "{value}");
    }
}

#[test]
fn scheme_relative_userinfo_is_redacted_on_append() {
    for (name, value) in SR_HEADERS {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: name.into(),
            value: value.into(),
        }];
        store.append(ne, Some(b"b")).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value}");
    }
    let (_d, data, store) = setup();
    store.append(plain("//alice:secret@h/p"), None).unwrap();
    let mut text = String::new();
    all_text(&hist_dir(&data), &mut text);
    assert!(!text.contains("secret"));
}

#[test]
fn scheme_relative_userinfo_is_redacted_by_legacy_scrub() {
    for (name, value) in SR_HEADERS {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), Some(b"b")).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["response_headers"] = serde_json::json!([{"name": name, "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
        let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value}");
    }
}

const SPECIAL_SCHEME_HEADERS: [(&str, &str); 6] = [
    ("Location", "https:/alice:secret@example.com/"),
    ("Location", "https:\\alice:secret@example.com/"),
    ("Location", "HTTPS:alice:secret@example.com/"),
    ("Location", "wss:/u:secret@h/"),
    ("Refresh", "0; https:/alice:secret@h/"),
    ("Refresh", "0; url=https:alice:secret@h/"),
];

#[test]
fn special_scheme_with_few_slashes_is_redacted_on_append() {
    for (name, value) in SPECIAL_SCHEME_HEADERS {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: name.into(),
            value: value.into(),
        }];
        store.append(ne, Some(b"b")).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value}");
    }
}

#[test]
fn special_scheme_with_few_slashes_is_redacted_by_legacy_scrub() {
    for (name, value) in SPECIAL_SCHEME_HEADERS {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), Some(b"b")).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["response_headers"] = serde_json::json!([{"name": name, "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
        let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value}");
    }
}

#[test]
fn non_special_scheme_location_with_one_slash_is_unchanged() {
    let (_d, data, store) = setup();
    let mut ne = plain("http://a/");
    ne.response_headers = vec![Header {
        name: "Location".into(),
        value: "custom:/a:b@h".into(),
    }];
    store.append(ne, Some(b"b")).unwrap();
    let mut text = String::new();
    all_text(&hist_dir(&data), &mut text);
    assert!(text.contains("custom:/a:b@h"));
}

#[test]
fn mailto_location_is_handled_by_the_existing_rule() {
    // `mailto:alice@example.com` has no slash, so the existing rule already treats the
    // whole value as an authority and redacts `mailto:alice`. Pinned, not changed.
    let (_d, data, store) = setup();
    let mut ne = plain("http://a/");
    ne.response_headers = vec![Header {
        name: "Location".into(),
        value: "mailto:alice@example.com".into(),
    }];
    store.append(ne, Some(b"b")).unwrap();
    let mut text = String::new();
    all_text(&hist_dir(&data), &mut text);
    assert!(!text.contains("alice"));
}

#[cfg(unix)]
#[test]
fn scrub_failure_leaves_marker_absent_and_retries() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, data, store) = setup();
    let e = store.append(plain("http://a/"), Some(b"b")).unwrap();
    let bodies = bodies_dir(&data);
    let req = bodies.join(format!("{}.req.json", e.id));
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
    v["response_headers"] =
        serde_json::json!([{"name": "Location", "value": "http://h/?token=locsecret"}]);
    fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
    let marker = hist_dir(&data).join(".scrubbed-1");
    let _ = fs::remove_file(&marker);

    fs::set_permissions(&bodies, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&bodies).is_ok() {
        fs::set_permissions(&bodies, fs::Permissions::from_mode(0o700)).unwrap();
        return; // running as root
    }
    let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
    assert!(!marker.exists(), "marker written after failed scrub");
    fs::set_permissions(&bodies, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(fs::read_to_string(&req).unwrap().contains("locsecret"));

    let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
    assert!(marker.exists());
    assert!(!fs::read_to_string(&req).unwrap().contains("locsecret"));
}

const REFRESH_QUERY_SECRETS: [&str; 8] = [
    "0; url=https://example.com/?access_token=secret",
    "0; url='https://e.com/?a=1&token=secret'",
    "0; url=/next?api_key=secret",
    "0; url=https://e.com/?a=1; url=/x?password=secret",
    "0; url=https://e.com/?a=1&b=2&refresh_token=secret#frag",
    "0; url=https://e.com/?name=O'Brien&access_token=secret",
    "0; url=https://e.com/?x=a;b&access_token=secret",
    "0; url=https://e.com/?q=a b&access_token=secret",
];

#[test]
fn refresh_query_credentials_are_redacted_on_append() {
    for value in REFRESH_QUERY_SECRETS {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: "Refresh".into(),
            value: value.into(),
        }];
        let e = store.append(ne, Some(b"b")).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{value}");
        assert!(
            store
                .load(&e.id)
                .unwrap()
                .response_redacted
                .contains(&"Refresh".to_string()),
            "{value}"
        );
    }
}

#[test]
fn refresh_query_without_credentials_is_unchanged() {
    for value in [
        "0; url=https://h/p?a=1&b=2",
        "0; url='/n?page=2&sort=x#top'",
        "0; url=https://h/p?q=O'Brien;x=a b&c=1",
    ] {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: "Refresh".into(),
            value: value.into(),
        }];
        store.append(ne, Some(b"b")).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(text.contains(value), "{value}");
    }
}

#[test]
fn refresh_query_credentials_are_redacted_by_legacy_scrub() {
    for value in REFRESH_QUERY_SECRETS {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), Some(b"b")).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["response_headers"] = serde_json::json!([{"name": "Refresh", "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
        let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{value}");
        assert!(hist_dir(&data).join(".scrubbed-1").exists());
    }
}

const CONTROL_CHAR_HEADERS: [(&str, &str); 6] = [
    ("Location", "https:\t/alice:secret@h/"),
    ("Location", "https:/\t/alice:secret@h/"),
    ("Location", "ht\ttps:/alice:secret@h/"),
    ("Refresh", "0; url=https:\t/alice:secret@h/"),
    ("Location", "https:\r/alice:secret@h/"),
    ("Content-Location", "https:\n//alice:secret@h/"),
];

#[test]
fn tab_cr_lf_inside_a_url_do_not_hide_userinfo_on_append() {
    for (name, value) in CONTROL_CHAR_HEADERS {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: name.into(),
            value: value.into(),
        }];
        store.append(ne, Some(b"b")).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value:?}");
        assert!(!text.contains("alice"), "{name}: {value:?}");
    }
}

#[test]
fn tab_cr_lf_inside_a_url_do_not_hide_userinfo_in_legacy_scrub() {
    for (name, value) in CONTROL_CHAR_HEADERS {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), Some(b"b")).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["response_headers"] = serde_json::json!([{"name": name, "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
        let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value:?}");
        assert!(!text.contains("alice"), "{name}: {value:?}");
    }
}

#[test]
fn credential_free_location_with_a_tab_is_byte_identical() {
    let (_d, data, store) = setup();
    let mut ne = plain("http://a/");
    ne.response_headers = vec![Header {
        name: "Location".into(),
        value: "https://h/a\tb".into(),
    }];
    store.append(ne, Some(b"b")).unwrap();
    let mut text = String::new();
    all_text(&hist_dir(&data), &mut text);
    assert!(text.contains(r"https://h/a\tb"));
}

fn leading_whitespace_headers() -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    for prefix in (0u8..=b' ')
        .map(|b| char::from(b).to_string())
        .chain([" \t \r\n".to_string()])
    {
        for name in ["Location", "Content-Location"] {
            for url in [
                "https:/alice:secret@h/",
                "https:alice:secret@h/",
                "https://alice:secret@h/",
                "//alice:secret@h/",
                r"\\alice:secret@h/",
            ] {
                headers.push((name, format!("{prefix}{url}")));
            }
        }
    }
    headers
}

#[test]
fn leading_whitespace_does_not_hide_userinfo_on_append() {
    for (name, value) in leading_whitespace_headers() {
        let (_d, data, store) = setup();
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: name.into(),
            value: value.clone(),
        }];
        let entry = store.append(ne, None).unwrap();
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value:?}");
        assert!(!text.contains("alice"), "{name}: {value:?}");
        let names = store.load(&entry.id).unwrap().response_redacted;
        assert!(names.iter().any(|n| n == name));
    }
}

#[test]
fn leading_whitespace_does_not_hide_userinfo_in_legacy_scrub() {
    for (name, value) in leading_whitespace_headers() {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), None).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["response_headers"] = serde_json::json!([{"name": name, "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let marker = hist_dir(&data).join(".scrubbed-1");
        if marker.exists() {
            fs::remove_file(&marker).unwrap();
        }
        let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value:?}");
        assert!(!text.contains("alice"), "{name}: {value:?}");
        assert!(hist_dir(&data).join(".scrubbed-1").exists());
    }
}

#[test]
fn leading_whitespace_in_credential_free_locations_is_unchanged() {
    let (_d, _data, store) = setup();
    for value in [" https:/h/path", " \t\r\nhttps://h/p?q=a@b", " //h/path"] {
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: "Location".into(),
            value: value.into(),
        }];
        let e = store.append(ne, None).unwrap();
        let loaded = store.load(&e.id).unwrap();
        assert_eq!(loaded.response_headers[0].value, value);
    }
}

fn encoded_query_headers() -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    for name in [
        "%74oken",
        "to%6ben",
        "%54%4F%4b%45%4e",
        "access%5Ftoken",
        "api%5fkey",
        "%FF%74oken",
        "%GG%74oken",
        "%26token%3D",
    ] {
        for header in ["Location", "Content-Location", "Refresh"] {
            let prefix = if header == "Refresh" { "0; url=" } else { "" };
            headers.push((
                header,
                format!("{prefix}https://example.com/?{name}=secret&x=%2f+z#frag"),
            ));
        }
    }
    headers
}

#[test]
fn encoded_query_names_are_matched_on_append() {
    for (name, value) in encoded_query_headers() {
        let (_d, data, store) = setup();
        let mut ne = plain("https://example.com/?%74oken=secret");
        ne.response_headers = vec![Header {
            name: name.into(),
            value: value.clone(),
        }];
        let e = store.append(ne, None).unwrap();
        let loaded = store.load(&e.id).unwrap();
        assert_eq!(
            loaded.response_headers[0].value,
            value.replace("=secret", "=")
        );
        let names = store.load(&e.id).unwrap().response_redacted;
        assert!(names.iter().any(|n| n == name));
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value}");
    }
}

#[test]
fn encoded_query_names_are_matched_in_legacy_scrub() {
    for (name, value) in encoded_query_headers() {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), None).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["request"]["url"] = serde_json::json!("https://example.com/?%74oken=secret");
        v["response_headers"] = serde_json::json!([{"name": name, "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let index = hist_dir(&data).join("index.jsonl");
        let raw = fs::read_to_string(&index)
            .unwrap()
            .replace("http://a/", "https://example.com/?%74oken=secret");
        fs::write(&index, raw).unwrap();
        let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
        let reopened = HistoryStore::open(&data, &cid("abcdef0123456789"));
        let loaded = reopened.load(&e.id).unwrap();
        assert_eq!(
            loaded.response_headers[0].value,
            value.replace("=secret", "=")
        );
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        assert!(!text.contains("secret"), "{name}: {value}");
        assert!(hist_dir(&data).join(".scrubbed-1").exists());
    }
}

#[test]
fn query_name_decoding_preserves_other_pairs_and_variable_references() {
    let (_d, _data, store) = setup();
    for query in [
        "%70age=2&x=%2f+z",
        "%GG=value&%=end&%7=short&%FF=value",
        "to+ken=value&%2574oken=value",
        "%74oken={{token}}&api%5fkey={{key}}",
    ] {
        let value = format!("https://example.com/?{query}");
        let mut ne = plain("http://a/");
        ne.response_headers = vec![Header {
            name: "Location".into(),
            value: value.clone(),
        }];
        let e = store.append(ne, None).unwrap();
        assert_eq!(store.load(&e.id).unwrap().response_headers[0].value, value);
    }
}

fn custom_scheme_backslash_headers() -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    for url in [
        r"custom://alice:secret\part@example.com/",
        r"custom://alice\part:secret@example.com/",
        r"custom://\alice:secret@example.com/",
        r"custom://alice:secret\\part@example.com/",
        r"CUSTOM://alice:secret\part@example.com/",
    ] {
        for header in ["Location", "Content-Location", "Refresh"] {
            let prefix = if header == "Refresh" { "0; url=" } else { "" };
            headers.push((header, format!("{prefix}{url}")));
        }
    }
    headers.push((
        "Refresh",
        r"0; custom://alice:secret\part@example.com/".into(),
    ));
    headers.push((
        "Refresh",
        r"0 url='custom://alice:secret\part@example.com/'".into(),
    ));
    headers
}

#[test]
fn custom_scheme_backslashes_do_not_hide_userinfo_on_append() {
    for (name, value) in custom_scheme_backslash_headers() {
        let (_d, data, store) = setup();
        let mut ne = plain(r"custom://alice:secret\part@example.com/");
        ne.response_headers = vec![Header {
            name: name.into(),
            value: value.clone(),
        }];
        let e = store.append(ne, None).unwrap();
        let names = store.load(&e.id).unwrap().response_redacted;
        assert!(names.iter().any(|n| n == name));
        assert!(
            store.load(&e.id).unwrap().response_headers[0]
                .value
                .contains(":@example.com/")
        );
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        for credential in ["alice", "secret", "part"] {
            assert!(!text.contains(credential), "{name}: {value}");
        }
    }
}

#[test]
fn custom_scheme_backslashes_do_not_hide_userinfo_in_legacy_scrub() {
    for (name, value) in custom_scheme_backslash_headers() {
        let (_d, data, store) = setup();
        let e = store.append(plain("http://a/"), None).unwrap();
        let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
        let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
        v["request"]["url"] = serde_json::json!(r"custom://alice:secret\part@example.com/");
        v["response_headers"] = serde_json::json!([{"name": name, "value": value}]);
        fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
        let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
        let reopened = HistoryStore::open(&data, &cid("abcdef0123456789"));
        assert!(
            reopened.load(&e.id).unwrap().response_headers[0]
                .value
                .contains(":@example.com/")
        );
        let mut text = String::new();
        all_text(&hist_dir(&data), &mut text);
        for credential in ["alice", "secret", "part"] {
            assert!(!text.contains(credential), "{name}: {value}");
        }
    }
}

#[test]
fn backslashes_end_special_authorities_and_custom_paths_stay_unchanged() {
    let (_d, _data, store) = setup();
    for url in [
        r"https://example.com\alice:secret@example.com/",
        r"custom://example.com/path\alice:secret@example.com/",
        r"custom://example.com/",
        r"custom:///alice:secret@example.com/",
    ] {
        for name in ["Location", "Content-Location", "Refresh"] {
            let prefix = if name == "Refresh" { "0; url=" } else { "" };
            let value = format!("{prefix}{url}");
            let mut ne = plain("http://a/");
            ne.response_headers = vec![Header {
                name: name.into(),
                value: value.clone(),
            }];
            let e = store.append(ne, None).unwrap();
            assert_eq!(store.load(&e.id).unwrap().response_headers[0].value, value);
        }
    }
}

const FILE_BACKSLASH_URLS: [&str; 3] = [
    r"file:\\alice:secret@h/",
    r"file:/\alice:secret@h/",
    r" file:\\alice:secret@h/",
];

#[test]
fn file_scheme_backslashes_do_not_hide_userinfo_on_append() {
    for url in FILE_BACKSLASH_URLS {
        for name in ["Location", "Content-Location"] {
            let (_d, data, store) = setup();
            let mut ne = plain(r"file:\\alice:secret@h/");
            ne.response_headers = vec![Header {
                name: name.into(),
                value: url.into(),
            }];
            store.append(ne, None).unwrap();
            let mut text = String::new();
            all_text(&hist_dir(&data), &mut text);
            for credential in ["alice", "secret"] {
                assert!(!text.contains(credential), "{name}: {url}");
            }
        }
    }
}

#[test]
fn file_scheme_backslashes_do_not_hide_userinfo_in_legacy_scrub() {
    for url in FILE_BACKSLASH_URLS {
        for name in ["Location", "Content-Location"] {
            let (_d, data, store) = setup();
            let e = store.append(plain("http://a/"), None).unwrap();
            let req = bodies_dir(&data).join(format!("{}.req.json", e.id));
            let mut v: serde_json::Value =
                serde_json::from_slice(&fs::read(&req).unwrap()).unwrap();
            v["request"]["url"] = serde_json::json!(r"file:\\alice:secret@h/");
            v["response_headers"] = serde_json::json!([{"name": name, "value": url}]);
            fs::write(&req, serde_json::to_vec(&v).unwrap()).unwrap();
            let _ = fs::remove_file(hist_dir(&data).join(".scrubbed-1"));
            let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
            let mut text = String::new();
            all_text(&hist_dir(&data), &mut text);
            for credential in ["alice", "secret"] {
                assert!(!text.contains(credential), "{name}: {url}");
            }
        }
    }
}

#[test]
fn file_scheme_without_userinfo_stays_unchanged() {
    let (_d, _data, store) = setup();
    for url in ["file:///etc/x", "file://h/p"] {
        for name in ["Location", "Content-Location"] {
            let mut ne = plain("http://a/");
            ne.response_headers = vec![Header {
                name: name.into(),
                value: url.into(),
            }];
            let e = store.append(ne, None).unwrap();
            assert_eq!(store.load(&e.id).unwrap().response_headers[0].value, url);
        }
    }
}

#[test]
fn history_index_line_without_delivery_loads_as_none() {
    let line = r#"{"id":"e1","at":1,"method":"Get","url":"http://x/","outcome":"Completed"}"#;
    let e: wisp_storage::history::HistoryEntry = serde_json::from_str(line).unwrap();
    assert_eq!(e.delivery, None);
}

#[test]
fn history_delivery_is_written_only_when_present() {
    use wisp_storage::history::Delivery;
    let (_d, _data, store) = setup();
    let none = store.append(plain("http://x/"), None).unwrap();
    assert!(!serde_json::to_string(&none).unwrap().contains("delivery"));
    let mut n = plain("http://x/");
    n.delivery = Some(Delivery::NotSent);
    let some = store.append(n, None).unwrap();
    assert!(
        serde_json::to_string(&some)
            .unwrap()
            .contains(r#""delivery":"not_sent""#)
    );
    let listed = store.list().unwrap();
    assert!(listed.iter().any(|e| e.delivery == Some(Delivery::NotSent)));
}

fn json_request(content: String) -> Request {
    Request {
        url: "https://x.test/".into(),
        body: Body::Json(TextBody {
            content,
            sidecar: None,
        }),
        ..Request::default()
    }
}

fn stored_json(store: &HistoryStore, id: &str) -> String {
    match store.load_without_body(id).unwrap().request.body {
        Body::Json(b) => b.content,
        other => panic!("{other:?}"),
    }
}

#[test]
fn h2_unterminated_refs_body_is_stored() {
    let (_d, _data, store) = setup();
    let e = store
        .append(new_entry(json_request("{{".repeat(512 * 1024))), None)
        .unwrap();
    // Not valid JSON, so the redactor drops it and names it; the point is that it finishes.
    assert!(stored_json(&store, &e.id).is_empty());
    assert!(e.redacted.contains(&"body".to_string()));
}

#[test]
fn h2_ten_thousand_refs_round_trip() {
    let (_d, _data, store) = setup();
    let mut body = String::from("{\"token\":\"x\",\"a\":[");
    for i in 0..10_000 {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!("{{{{v{i}}}}}"));
        body.push_str(&" ".repeat(94));
    }
    body.push_str("]}");
    assert!(body.len() > 1_000_000 && body.len() < 1024 * 1024 + 4096);
    let e = store
        .append(new_entry(json_request(body.clone())), None)
        .unwrap();
    let got = stored_json(&store, &e.id);
    for i in 0..10_000 {
        assert!(got.contains(&format!("{{{{v{i}}}}}")), "ref {i}");
    }
    assert!(!got.contains("wispref"));
    assert!(got.contains("\"token\":\"\""));
}

#[test]
fn h2_request_body_is_capped_and_flagged() {
    let (_d, data, store) = setup();
    let cap = wisp_storage::history::BODY_CAP;
    let req = Request {
        url: "https://x.test/".into(),
        body: Body::Text(TextBody {
            content: "a".repeat(cap * 2),
            sidecar: None,
        }),
        ..Request::default()
    };
    let e = store.append(new_entry(req), None).unwrap();
    let stored = store.load_without_body(&e.id).unwrap();
    assert!(stored.request_truncated);
    match stored.request.body {
        Body::Text(b) => assert_eq!(b.content.len(), cap),
        other => panic!("{other:?}"),
    }
    let raw = fs::read_to_string(bodies_dir(&data).join(format!("{}.req.json", e.id))).unwrap();
    assert!(raw.contains("\"truncated\":true"));
}

#[test]
fn h2_old_req_file_without_flag_loads() {
    let (_d, data, store) = setup();
    let e = store.append(plain("https://x.test/"), None).unwrap();
    let path = bodies_dir(&data).join(format!("{}.req.json", e.id));
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("truncated");
    v.as_object_mut().unwrap().remove("response_redacted");
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let stored = store.load_without_body(&e.id).unwrap();
    assert!(!stored.request_truncated);
    assert!(stored.response_redacted.is_empty());
}

#[test]
fn h2_response_header_names_stay_out_of_request_redacted() {
    let (_d, _data, store) = setup();
    let mut req = Request {
        url: "https://x.test/".into(),
        ..Request::default()
    };
    req.headers.push(kv("X-Foo", ""));
    let mut new = new_entry(req);
    new.response_headers = vec![Header {
        name: "X-Foo".into(),
        value: "x".into(),
    }];
    new.response_headers.push(Header {
        name: "X-Foo-Token".into(),
        value: "secret".into(),
    });
    let e = store.append(new, None).unwrap();
    assert!(!e.redacted.iter().any(|n| n == "X-Foo-Token"));
    assert!(!e.redacted.iter().any(|n| n == "X-Foo"));
    let stored = store.load_without_body(&e.id).unwrap();
    assert_eq!(stored.response_redacted, vec!["X-Foo-Token".to_string()]);
    // A 0.3.0 index line (no new fields) still loads.
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 1);
}

fn snapshot_dir(dir: &std::path::Path) -> Vec<(std::path::PathBuf, u64, SystemTime)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for f in fs::read_dir(&d).unwrap().flatten() {
            let m = f.metadata().unwrap();
            if m.is_dir() {
                stack.push(f.path());
            }
            out.push((f.path(), m.len(), m.modified().unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn h2_read_only_unmigrated_gives_note_and_changes_nothing() {
    let (d, data, store) = setup();
    store.append(plain("https://x.test/a"), None).unwrap();
    let hd = hist_dir(&data);
    let _ = fs::remove_file(hd.join(".scrubbed-1"));
    let before = snapshot_dir(d.path());
    let ro = HistoryStore::open_read_only(&hd).unwrap();
    let got = ro.list(5).unwrap();
    assert!(got.entries.is_empty());
    assert_eq!(
        got.note.as_deref(),
        Some("history not yet migrated; open Wisp once")
    );
    assert_eq!(snapshot_dir(d.path()), before);
    assert!(HistoryStore::open_read_only(&d.path().join("missing")).is_none());
}

#[test]
fn h2_read_only_migrated_lists_newest_up_to_limit() {
    let (d, data, store) = setup();
    for i in 0..4 {
        store
            .append(plain(&format!("https://x.test/{i}")), None)
            .unwrap();
    }
    // Reopening migrates the dir and writes the marker.
    let _ = HistoryStore::open(&data, &cid("abcdef0123456789"));
    let before = snapshot_dir(d.path());
    let ro = HistoryStore::open_read_only(&hist_dir(&data)).unwrap();
    let got = ro.list(2).unwrap();
    assert!(got.note.is_none());
    let urls: Vec<&str> = got.entries.iter().map(|e| e.url.as_str()).collect();
    assert_eq!(urls, ["https://x.test/3", "https://x.test/2"]);
    assert_eq!(snapshot_dir(d.path()), before);
}
