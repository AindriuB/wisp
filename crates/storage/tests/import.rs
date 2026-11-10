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
