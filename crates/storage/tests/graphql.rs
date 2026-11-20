use std::fs;
use std::time::{Duration, SystemTime};

use wisp_core::request::{Body, GraphQlBody, Request};
use wisp_storage::collection::CollectionId;
use wisp_storage::data_dir::DataDir;
use wisp_storage::history::{BODY_CAP, HistoryStore, NewEntry, Outcome};
use wisp_storage::request::{load_request, requests_equal, save_request};

fn gql(query: &str, variables: &str) -> Request {
    Request {
        name: "g".into(),
        url: "https://x.test/graphql".into(),
        body: Body::GraphQl(GraphQlBody {
            query: query.into(),
            variables: variables.into(),
        }),
        ..Request::default()
    }
}

fn stored(req: Request) -> (Request, Vec<String>, bool) {
    let d = tempfile::tempdir().unwrap();
    let data = DataDir::at(d.path());
    let store = HistoryStore::open(&data, &CollectionId::parse("abcdef0123456789").unwrap());
    let e = store
        .append(
            NewEntry {
                at: SystemTime::UNIX_EPOCH + Duration::from_secs(10),
                request_path: None,
                request: req,
                outcome: Outcome::Completed,
                status: Some(200),
                ttfb: None,
                total: None,
                dns: None,
                connect: None,
                size: Some(0),
                response_headers: Vec::new(),
                environment: None,
                assertions: Vec::new(),
                delivery: None,
            },
            None,
        )
        .unwrap();
    let s = store.load_without_body(&e.id).unwrap();
    let redacted = store
        .list()
        .unwrap()
        .into_iter()
        .find(|x| x.id == e.id)
        .unwrap()
        .redacted;
    (s.request, redacted, s.request_truncated)
}

fn parts(r: &Request) -> (&str, &str) {
    match &r.body {
        Body::GraphQl(g) => (&g.query, &g.variables),
        other => panic!("{other:?}"),
    }
}

#[test]
fn graphql_file_round_trips_byte_for_byte() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("g.toml");
    let text = "name = \"g\"\nmethod = \"POST\"\nurl = \"https://x.test/graphql\"\n\n[body]\ntype = \"graphql\"\nquery = '''\n{\n  user(id: 1) { name }\n}\n'''\nvariables = '''\n{\n  \"id\": 1\n}\n'''\n";
    fs::write(&path, text).unwrap();
    let r = load_request(&path).unwrap();
    assert_eq!(parts(&r).0, "{\n  user(id: 1) { name }\n}\n");
    assert_eq!(parts(&r).1, "{\n  \"id\": 1\n}\n");
    save_request(&path, &r).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn graphql_file_missing_variables_reads_blank() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("g.toml");
    fs::write(
        &path,
        "name = \"g\"\nmethod = \"POST\"\nurl = \"u\"\n\n[body]\ntype = \"graphql\"\nquery = \"{ a }\"\n",
    )
    .unwrap();
    let r = load_request(&path).unwrap();
    assert_eq!(parts(&r), ("{ a }", ""));
}

#[test]
fn graphql_file_write_then_read_round_trips() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("g.toml");
    let r = gql("query Q {\n  a\n}", "{\"k\": \"it's\"}");
    save_request(&path, &r).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("type = \"graphql\""), "{text}");
    assert!(!text.contains("content"), "{text}");
    assert!(requests_equal(&load_request(&path).unwrap(), &r));
}

#[test]
fn graphql_file_switching_from_json_drops_content() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("g.toml");
    let mut r = gql("", "");
    r.body = Body::Json(wisp_core::request::TextBody {
        content: "{}".into(),
        sidecar: None,
    });
    save_request(&path, &r).unwrap();
    save_request(&path, &gql("{ a }", "")).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("content"), "{text}");
    save_request(&path, &r).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("query"), "{text}");
}

#[test]
fn graphql_file_requests_equal_covers_both_fields() {
    let a = gql("{ a }", "{}");
    assert!(requests_equal(&a, &gql("{ a }", "{}")));
    assert!(!requests_equal(&a, &gql("{ b }", "{}")));
    assert!(!requests_equal(&a, &gql("{ a }", "{\"x\":1}")));
}

#[test]
fn graphql_file_invalid_variables_type_is_rejected() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("g.toml");
    fs::write(
        &path,
        "name = \"g\"\nmethod = \"POST\"\nurl = \"u\"\n\n[body]\ntype = \"graphql\"\nvariables = 3\n",
    )
    .unwrap();
    assert!(load_request(&path).is_err());
}

#[test]
fn graphql_history_redacts_variables_like_json() {
    let (r, names, _) = stored(gql(
        "{ a }",
        "{\"id\": 1, \"password\": \"hunter2\", \"x\": \"{{t}}\"}",
    ));
    let (q, v) = parts(&r);
    assert_eq!(q, "{ a }");
    assert!(!v.contains("hunter2"), "{v}");
    assert!(v.contains("\"password\":\"\""), "{v}");
    assert!(v.contains("{{t}}"), "{v}");
    assert!(!names.is_empty());
}

#[test]
fn graphql_history_clears_a_query_holding_a_credential() {
    let (r, names, _) = stored(gql("{ a(password: \"hunter2\") }", "{}"));
    assert_eq!(parts(&r).0, "");
    assert!(names.iter().any(|n| n == "body"), "{names:?}");
}

#[test]
fn graphql_history_clears_variables_that_are_not_json() {
    let (r, _, _) = stored(gql("{ a }", "password=hunter2"));
    assert_eq!(parts(&r).1, "");
}

#[test]
fn graphql_history_caps_the_pair_and_marks_truncated() {
    let (r, _, truncated) = stored(gql(
        &"q".repeat(BODY_CAP - 10),
        &format!("{{\"k\":\"{}\"}}", "v".repeat(100)),
    ));
    let (q, v) = parts(&r);
    assert!(truncated);
    assert_eq!(q.len() + v.len(), BODY_CAP);
    assert_eq!(q.len(), BODY_CAP - 10);
}

#[test]
fn graphql_history_within_the_cap_is_not_truncated() {
    let (r, _, truncated) = stored(gql("{ a }", "{\"a\":1}"));
    assert!(!truncated);
    assert_eq!(parts(&r), ("{ a }", "{\"a\":1}"));
}
