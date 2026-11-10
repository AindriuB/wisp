use std::fs;
use std::time::{Duration, SystemTime};

use wisp_core::request::{
    ApiKeyLocation, Auth, Body, KeyValue, Part, PartValue, Request, TextBody, Var,
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
        size: Some(3),
        response_headers: vec![Header {
            name: "X".into(),
            value: "y".into(),
        }],
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
    assert!(e.redacted.contains(&"Set-Cookie".to_string()));
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
