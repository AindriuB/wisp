use std::fs;
use std::time::{Duration, SystemTime};

use wisp_core::request::{ApiKeyLocation, Auth, KeyValue, Request};
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
