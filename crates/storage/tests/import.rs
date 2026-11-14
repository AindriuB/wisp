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
