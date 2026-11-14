//! Importers into a real collection on disk, through `open_collection` and `resolve`.

use std::path::{Path, PathBuf};

use wisp_core::resolve::resolve;
use wisp_storage::collection::{Node, open_collection};
use wisp_storage::environment::load_environment;
use wisp_storage::import::{ImportResult, curl, openapi, postman, write};
use wisp_storage::request::load_request;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/import")
            .join(name),
    )
    .unwrap()
}

fn collection() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("collection.toml"), "format = 1\n").unwrap();
    d
}

fn request_paths(nodes: &[Node], out: &mut Vec<PathBuf>) {
    for n in nodes {
        match n {
            Node::Folder { children, .. } => request_paths(children, out),
            Node::Request(m) => out.push(m.path.clone()),
            Node::Invalid { path, error } => panic!("invalid {}: {error}", path.display()),
        }
    }
}

/// Import, reopen, and resolve every request against `env`.
fn import_and_resolve(r: &ImportResult, env: Option<&str>) -> usize {
    let d = collection();
    write(d.path(), Path::new(""), r, "imported").unwrap();
    let tree = open_collection(d.path()).unwrap();
    let envr = env.map(|e| load_environment(&tree.root, e).unwrap());
    let mut paths = Vec::new();
    request_paths(&tree.nodes, &mut paths);
    for p in &paths {
        let abs = tree.root.join(p);
        let req = load_request(&abs).unwrap();
        let scope = tree.scope_for(&abs, envr.as_ref());
        resolve(&req, &scope).unwrap_or_else(|e| panic!("{}: {e:?}", p.display()));
    }
    paths.len()
}

#[test]
fn postman_writes_and_resolves() {
    let r = postman::parse(&fixture("postman.json")).unwrap();
    assert_eq!(import_and_resolve(&r, Some("imported")), 5);
}

#[test]
fn postman_secrets_stay_in_secrets_file() {
    let r = postman::parse(&fixture("postman.json")).unwrap();
    let d = collection();
    write(d.path(), Path::new(""), &r, "imported").unwrap();
    let mut stack = vec![d.path().to_path_buf()];
    let mut secrets_seen = false;
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let is_secrets = path.to_string_lossy().ends_with(".secrets.toml");
            if is_secrets {
                secrets_seen = true;
                assert!(text.contains("s3cret-pass"));
            } else {
                assert!(
                    !text.contains("s3cret-pass") && !text.contains("tok-123"),
                    "{}",
                    path.display()
                );
            }
        }
    }
    assert!(secrets_seen);
}

#[test]
fn openapi_json_and_yaml_write_and_resolve() {
    for f in ["openapi.json", "openapi.yaml"] {
        let r = openapi::parse(&fixture(f)).unwrap();
        assert_eq!(import_and_resolve(&r, Some("Production")), 6, "{f}");
    }
}

#[test]
fn curl_writes_and_resolves() {
    let r = curl::parse(
        "curl -X POST -H 'Content-Type: application/json' -d '{\"a\":1}' 'https://h.example/x?q=1'",
    )
    .unwrap();
    assert_eq!(import_and_resolve(&r, None), 1);
}

#[test]
fn second_import_gets_numbered_names() {
    let r = postman::parse(&fixture("postman.json")).unwrap();
    let d = collection();
    let first = write(d.path(), Path::new(""), &r, "imported").unwrap();
    assert!(first.renamed.is_empty());
    let second = write(d.path(), Path::new(""), &r, "imported").unwrap();
    assert!(second.renamed.contains(&("Pets".into(), "Pets 2".into())));
    assert!(second.renamed.contains(&("Login".into(), "Login 2".into())));
    assert!(
        second
            .renamed
            .contains(&("imported".into(), "imported-2".into()))
    );
    assert_eq!(second.requests, first.requests);
    let tree = open_collection(d.path()).unwrap();
    assert!(request_count(&tree.nodes) == 10);
}

fn request_count(nodes: &[Node]) -> usize {
    let mut v = Vec::new();
    request_paths(nodes, &mut v);
    v.len()
}

#[test]
fn leftover_secrets_file_is_not_overwritten() {
    let d = collection();
    let envs = d.path().join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    let kept = "[vars]\ntoken = \"keep-me\"\n";
    std::fs::write(envs.join("dev.secrets.toml"), kept).unwrap();
    let r = openapi::parse(
        br#"{"openapi":"3.0.0","servers":[{"url":"https://x","description":"dev"}],"paths":{}}"#,
    )
    .unwrap();
    let report = write(d.path(), Path::new(""), &r, "imported").unwrap();
    assert_eq!(
        report.renamed,
        vec![("dev".to_string(), "dev-2".to_string())]
    );
    assert_eq!(
        std::fs::read_to_string(envs.join("dev.secrets.toml")).unwrap(),
        kept
    );
}

/// Every file under `dir`, with its text.
fn all_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                out.push((path, text));
            }
        }
    }
    out
}

#[test]
fn file_parts_never_carry_a_path_and_round_trip() {
    let r = curl::parse("curl -F f=@environments/prod.secrets.toml http://h/up").unwrap();
    let pm = postman::parse(
        br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item":[{"name":"Up","request":{"method":"POST","url":"http://h/up",
        "body":{"mode":"formdata","formdata":[{"key":"f","type":"file","src":"environments/prod.secrets.toml"}]}}}]}"#,
    )
    .unwrap();
    for r in [r, pm] {
        let d = collection();
        write(d.path(), Path::new(""), &r, "imported").unwrap();
        for (path, text) in all_files(d.path()) {
            assert!(!text.contains("prod.secrets"), "{}", path.display());
        }
        let tree = open_collection(d.path()).unwrap();
        let mut paths = Vec::new();
        request_paths(&tree.nodes, &mut paths);
        let abs = tree.root.join(&paths[0]);
        let before = std::fs::read_to_string(&abs).unwrap();
        let req = load_request(&abs).unwrap();
        wisp_storage::request::save_request(&abs, &req).unwrap();
        assert_eq!(std::fs::read_to_string(&abs).unwrap(), before);
    }
}

#[test]
fn credentials_never_reach_committed_files() {
    let curl_r = curl::parse(
        "curl -u bob:hunter2-pw -H 'Authorization: Bearer cred-aaa' -H 'Cookie: sid=cred-bbb' http://h/x",
    )
    .unwrap();
    let pm = postman::parse(
        br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "variable":[{"key":"apiKey","value":"cred-ccc"}],
        "item":[{"name":"A","request":{"method":"GET","url":"http://h/a",
        "header":[{"key":"Authorization","value":"Bearer cred-ddd"},{"key":"Cookie","value":"sid=cred-eee"}]}}]}"#,
    )
    .unwrap();
    for (r, needles) in [
        (curl_r, vec!["hunter2-pw", "cred-aaa", "cred-bbb"]),
        (pm, vec!["cred-ccc", "cred-ddd", "cred-eee"]),
    ] {
        let d = collection();
        write(d.path(), Path::new(""), &r, "imported").unwrap();
        let files = all_files(d.path());
        for needle in &needles {
            for (path, text) in &files {
                if path.to_string_lossy().ends_with(".secrets.toml") {
                    continue;
                }
                assert!(!text.contains(needle), "{needle} in {}", path.display());
            }
            assert!(
                files
                    .iter()
                    .any(|(p, t)| p.to_string_lossy().ends_with(".secrets.toml")
                        && t.contains(needle)),
                "{needle}"
            );
        }
    }
}

/// Import `cmd` with curl, write it, and assert no TOML other than the secrets
/// file contains any needle; at least one needle must be in the secrets file.
fn assert_no_leak(cmd: &str, needles: &[&str]) {
    let r = curl::parse(cmd).unwrap();
    let d = collection();
    write(d.path(), Path::new(""), &r, "imported").unwrap();
    let files = all_files(d.path());
    for needle in needles {
        for (path, text) in &files {
            if path.to_string_lossy().ends_with(".secrets.toml") {
                continue;
            }
            assert!(
                !text.contains(needle),
                "{needle} in {} ({cmd})",
                path.display()
            );
        }
    }
}

#[test]
fn header_with_secret_and_reference_is_not_kept_literally() {
    assert_no_leak(
        "curl -H 'Authorization: Bearer SECRETVAL {{x}}' http://h/x",
        &["SECRETVAL"],
    );
}

#[test]
fn password_with_url_delimiters_is_not_written() {
    for url in [
        "https://u:SEC/RET@h/x",
        "https://u:SEC?RET@h/x",
        "https://u:SEC#RET@h/x",
    ] {
        assert_no_leak(&format!("curl '{url}'"), &["SEC", "RET"]);
    }
}

#[test]
fn lenient_scheme_userinfo_is_not_written() {
    for url in ["https:///u:SECRETVAL@h/x", "http:/u:SECRETVAL@h/x"] {
        assert_no_leak(&format!("curl '{url}'"), &["SECRETVAL"]);
    }
}

#[test]
fn digest_ntlm_negotiate_credentials_stay_in_secrets() {
    for opt in ["--digest", "--ntlm", "--negotiate"] {
        let cmd = format!("curl {opt} -u user:SECRETVAL http://h/x");
        assert_no_leak(&cmd, &["SECRETVAL"]);
        let r = curl::parse(&cmd).unwrap();
        let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
            panic!()
        };
        if opt == "--digest" {
            assert!(matches!(
                request.auth,
                wisp_core::request::Auth::Digest { .. }
            ));
        } else {
            assert!(matches!(request.auth, wisp_core::request::Auth::Inherit));
            assert!(r.warnings.iter().any(|w| w.message.contains(opt)));
        }
        assert!(r.warnings.iter().all(|w| !w.message.contains("SECRETVAL")));
    }
}

#[test]
fn empty_port_password_goes_to_secrets() {
    for (url, needle) in [
        ("https://u:/SECPW@h/x", "SECPW"),
        ("https://u:?SECPW@h/x", "SECPW"),
        ("https://u:#SECPW@h/x", "SECPW"),
    ] {
        assert_no_leak(&format!("curl '{url}'"), &[needle]);
    }
}

/// Write `r` and assert no TOML other than the secrets file holds `needle`.
fn assert_result_no_leak(r: &ImportResult, needle: &str) {
    let d = collection();
    write(d.path(), Path::new(""), r, "imported").unwrap();
    for (path, text) in &all_files(d.path()) {
        if !path.to_string_lossy().ends_with(".secrets.toml") {
            assert!(!text.contains(needle), "{needle} in {}", path.display());
        }
    }
}

fn postman_url(url: &str) -> ImportResult {
    postman::parse(
        format!(
            r#"{{"info":{{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},
            "item":[{{"name":"A","request":{{"method":"GET","url":"{url}"}}}}]}}"#
        )
        .as_bytes(),
    )
    .unwrap()
}

fn openapi_url(url: &str) -> ImportResult {
    openapi::parse(
        format!(
            r#"{{"openapi":"3.0.0","info":{{"title":"t","version":"1"}},
            "servers":[{{"url":"{url}"}}],"paths":{{"/p":{{"get":{{"responses":{{"200":{{"description":"ok"}}}}}}}}}}}}"#
        )
        .as_bytes(),
    )
    .unwrap()
}

fn may_contain_warnings(r: &ImportResult) -> Vec<&str> {
    r.warnings
        .iter()
        .map(|w| w.message.as_str())
        .filter(|m| m.contains("may contain credentials"))
        .collect()
}

#[test]
fn postman_and_openapi_empty_port_password_goes_to_secrets() {
    assert_result_no_leak(&postman_url("https://u:/SECPW@h/x"), "SECPW");
    assert_result_no_leak(&openapi_url("https://u:/SECPW@h/x"), "SECPW");
}

#[test]
fn postman_and_openapi_host_port_with_later_at_warns() {
    let url = "http://localhost:8080/users/me@example.com";
    for r in [postman_url(url), openapi_url(url)] {
        let w = may_contain_warnings(&r);
        assert_eq!(w.len(), 1);
        assert!(!w[0].contains("8080") && !w[0].contains("example"));
    }
    let r = postman_url(url);
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert_eq!(request.url, url);
    assert_eq!(openapi_url(url).environments[0].vars[0].value, url);
}

#[test]
fn host_port_cut_by_query_or_fragment_is_kept_with_warning() {
    for (url, base, param) in [
        (
            "http://localhost:8080?email=a@b.com",
            "http://localhost:8080",
            ("email", "a@b.com"),
        ),
        ("https://u:12?34@h/x", "https://u:12", ("34@h/x", "")),
    ] {
        let r = curl::parse(&format!("curl '{url}'")).unwrap();
        let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
            panic!()
        };
        assert_eq!(request.url, base, "{url}");
        let got: Vec<_> = request
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.value.as_str()))
            .collect();
        assert_eq!(got, vec![param], "{url}");
        assert!(matches!(request.auth, wisp_core::request::Auth::Inherit));
        let n = r
            .warnings
            .iter()
            .filter(|w| w.message.contains("may contain credentials"))
            .count();
        assert_eq!(n, 1, "{url}");
    }
}

#[test]
fn host_port_with_later_at_is_kept_with_warning() {
    let r = curl::parse("curl 'http://localhost:8080/users/me@example.com'").unwrap();
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert_eq!(request.url, "http://localhost:8080/users/me@example.com");
    assert!(matches!(request.auth, wisp_core::request::Auth::Inherit));
    let w: Vec<_> = r
        .warnings
        .iter()
        .filter(|w| w.message.contains("may contain credentials"))
        .collect();
    assert_eq!(w.len(), 1);
    assert!(!w[0].message.contains("8080") && !w[0].message.contains("example"));
}

#[test]
fn ipv6_host_with_at_in_path_is_not_userinfo() {
    let r = curl::parse("curl 'http://[::1]:8080/a@b'").unwrap();
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert!(request.url.contains("[::1]:8080"), "{}", request.url);
    assert!(matches!(request.auth, wisp_core::request::Auth::Inherit));
}

#[test]
fn postman_password_with_raw_question_mark_leaks_nothing() {
    assert_result_no_leak(&postman_url("https://u:?SECPW@h/x"), "SECPW");
    assert_result_no_leak(&postman_url("https://u:ab?SECPW@h/x"), "SECPW");
    assert_result_no_leak(&postman_url("https://u:ab?SECPW@h/x"), "ab");
}

#[test]
fn postman_host_port_cut_by_question_mark_is_kept_with_warning() {
    let r = postman_url("https://u:12?34@h/x");
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert_eq!(request.url, "https://u:12?34@h/x");
    assert_eq!(may_contain_warnings(&r).len(), 1);
}

#[test]
fn postman_host_port_then_path_keeps_params_and_path_var() {
    let r = postman_url("http://localhost:8080/users/:id?email=a@b.com&x=1");
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert_eq!(request.url, "http://localhost:8080/users/{id}");
    let names: Vec<_> = request.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["email", "x"]);
    assert_eq!(request.path.len(), 1);
    assert_eq!(request.path[0].name, "id");
    assert_eq!(may_contain_warnings(&r).len(), 1);
}

#[test]
fn postman_host_port_slash_then_query_at_warns() {
    let r = postman_url("https://u:1234/ab?cd@h/x");
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert_eq!(request.url, "https://u:1234/ab");
    assert_eq!(may_contain_warnings(&r).len(), 1);
}

#[test]
fn postman_object_url_kept_whole_ignores_explicit_query() {
    let r = postman::parse(
        br#"{"info":{"name":"c","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item":[{"name":"r","request":{"method":"GET","url":{"raw":"http://localhost:8080?email=a@b.com&x=1",
        "query":[{"key":"email","value":"a@b.com"},{"key":"x","value":"1"}]}}}]}"#,
    )
    .unwrap();
    let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
        panic!()
    };
    assert_eq!(request.url, "http://localhost:8080?email=a@b.com&x=1");
    assert!(request.params.is_empty());
    assert_eq!(may_contain_warnings(&r).len(), 1);
}

#[test]
fn templated_port_is_host_port_not_userinfo() {
    for url in [
        "http://localhost:{{port}}/api?email=a@b.com",
        "http://{{host}}:{{port}}/users/me@example.com",
    ] {
        let r = curl::parse(&format!("curl '{url}'")).unwrap();
        assert_eq!(may_contain_warnings(&r).len(), 1, "{url}");
        let r = postman_url(url);
        assert_eq!(may_contain_warnings(&r).len(), 1, "{url}");
    }
}

#[test]
fn templated_urls_are_kept_without_auth_or_secrets() {
    for url in [
        "{{protocol}}://{{host}}/users/me@example.com",
        "{{protocol}}://{{host}}:{{port}}/api?email=a@b.com",
        "{{base}}/users/me@example.com",
        "http://{{host}}/users/me@example.com",
        "http://localhost:{{port}}/api?email=a@b.com",
        "{{base}}?email=a@b.com",
    ] {
        let path = url.split('?').next().unwrap();
        for r in [
            curl::parse(&format!("curl '{url}'")).unwrap(),
            postman_url(url),
        ] {
            let wisp_storage::import::ImportItem::Request { request, .. } = &r.items[0] else {
                panic!()
            };
            assert!(request.url.starts_with(path), "{url}: {}", request.url);
            assert!(!request.url.contains("example.com") || path.contains("example.com"));
            assert!(
                matches!(request.auth, wisp_core::request::Auth::Inherit),
                "{url}"
            );
            assert!(r.environments.is_empty(), "{url}");
            if !url.contains('?') {
                assert_eq!(request.url, url, "{url}");
            }
        }
    }
}

/// url, params, path vars, auth and secrets of every request in `r`.
fn describe(r: &ImportResult) -> String {
    use wisp_storage::import::ImportItem;
    fn walk(items: &[ImportItem], out: &mut Vec<String>) {
        for i in items {
            match i {
                ImportItem::Folder { items, .. } => walk(items, out),
                ImportItem::Request { request: q, .. } => {
                    let kv = |v: &[wisp_core::request::KeyValue]| -> Vec<String> {
                        v.iter()
                            .map(|p| format!("{}={}", p.name, p.value))
                            .collect()
                    };
                    let path: Vec<String> = q
                        .path
                        .iter()
                        .map(|p| format!("{}={}", p.name, p.value))
                        .collect();
                    out.push(format!(
                        "url={} params={:?} path={path:?} auth={:?}",
                        q.url,
                        kv(&q.params),
                        q.auth
                    ));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&r.items, &mut out);
    for e in &r.environments {
        let s: Vec<String> = e
            .secrets
            .iter()
            .map(|v| format!("{}={}", v.name, v.value))
            .collect();
        out.push(format!("secrets={s:?}"));
    }
    out.join(" | ")
}

/// AC5: URL shapes import exactly as on main (7ec7505), through curl and
/// Postman. Rows are (input, curl output, Postman output).
#[test]
fn url_shapes_import_as_on_main() {
    let table: &[(&str, &str, &str)] = &[
        (
            "{{base}}/x",
            "url={{base}}/x params=[] path=[] auth=Inherit",
            "url={{base}}/x params=[] path=[] auth=Inherit",
        ),
        (
            "{{base}}?email=a@b.com",
            "url={{base}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
            "url={{base}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
        ),
        (
            "{{protocol}}://{{host}}:{{port}}/p?q=a@b",
            "url={{protocol}}://{{host}}:{{port}}/p params=[\"q=a@b\"] path=[] auth=Inherit",
            "url={{protocol}}://{{host}}:{{port}}/p params=[\"q=a@b\"] path=[] auth=Inherit",
        ),
        (
            "http://localhost:{{port}}?email=a@b.com",
            "url=http://localhost:{{port}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
            "url=http://localhost:{{port}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
        ),
        (
            "http://{{host}}:{{port}}?email=a@b.com",
            "url=http://{{host}}:{{port}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
            "url=http://{{host}}:{{port}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
        ),
        (
            "{{host}}:{{port}}?email=a@b.com",
            "url={{host}}:{{port}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
            "url={{host}}:{{port}} params=[\"email=a@b.com\"] path=[] auth=Inherit",
        ),
        (
            "http://h/p@q",
            "url=http://h/p@q params=[] path=[] auth=Inherit",
            "url=http://h/p@q params=[] path=[] auth=Inherit",
        ),
        (
            "//h/x",
            "url=//h/x params=[] path=[] auth=Inherit",
            "url=//h/x params=[] path=[] auth=Inherit",
        ),
        (
            "http://user@h/x",
            "url=http://h/x params=[] path=[] auth=Basic { username: \"{{get_x_username}}\", password: \"<redacted>\" } | secrets=[\"get_x_username=user\"]",
            "url=http://h/x params=[] path=[] auth=Basic { username: \"{{a_username}}\", password: \"<redacted>\" } | secrets=[\"a_username=user\"]",
        ),
        (
            "http://[::1]:8080/a@b",
            "url=http://[::1]:8080/a@b params=[] path=[] auth=Inherit",
            "url=http://[::1]:8080/a@b params=[] path=[] auth=Inherit",
        ),
        (
            "http://h:8080/a@b",
            "url=http://h:8080/a@b params=[] path=[] auth=Inherit",
            "url=http://h:8080/a@b params=[] path=[] auth=Inherit",
        ),
        (
            "http:h/x@y",
            "url=http:h/x@y params=[] path=[] auth=Inherit",
            "url=http:h/x@y params=[] path=[] auth=Inherit",
        ),
        (
            "http://localhost:8080/users/:id?email=a@b.com&x=1",
            "url=http://localhost:8080/users/:id params=[\"email=a@b.com\", \"x=1\"] path=[] auth=Inherit",
            "url=http://localhost:8080/users/{id} params=[\"email=a@b.com\", \"x=1\"] path=[\"id=\"] auth=Inherit",
        ),
    ];
    for (input, curl_want, postman_want) in table {
        let c = curl::parse(&format!("curl '{input}'")).unwrap();
        assert_eq!(describe(&c), *curl_want, "curl {input}");
        let p = postman_url(input);
        assert_eq!(describe(&p), *postman_want, "postman {input}");
    }
}
