use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wisp-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn w(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn req(name: &str, extra: &str) -> String {
    format!("name = \"{name}\"\nmethod = \"GET\"\nurl = \"https://u:pw@h.test/p?q=1\"\n{extra}")
}

fn fixture(id: bool) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let idline = if id {
        "id = \"abcdefgh12345678\"\n"
    } else {
        ""
    };
    w(
        r,
        "collection.toml",
        &format!("format = 1\n{idline}\n[vars]\napi_token = \"CANARY_PLAIN\"\n"),
    );
    w(
        r,
        "environments/dev.toml",
        "[vars]\napi_token = \"CANARY_ENVPLAIN\"\n",
    );
    w(
        r,
        "environments/dev.secrets.toml",
        "[vars]\nk = \"CANARY_SECRET\"\n",
    );
    w(r, "environments/prod.toml", "secrets = \"keychain\"\n");
    w(
        r,
        "a.toml",
        &req(
            "A",
            "\n[[headers]]\nname = \"Authorization\"\nvalue = \"CANARY_AUTH\"\nenabled = true\n\n[auth]\ntype = \"bearer\"\ntoken = \"CANARY_TOKEN\"\n",
        ),
    );
    w(r, "sub/b.toml", &req("B", ""));
    w(r, "sub/bad.toml", "not toml [[[");
    d
}

fn p(d: &tempfile::TempDir) -> &str {
    d.path().to_str().unwrap()
}

#[test]
fn canaries_never_printed() {
    let d = fixture(true);
    for args in [
        vec!["inspect", p(&d)],
        vec!["inspect", p(&d), "--json"],
        vec!["inspect", p(&d), "--env", "dev", "--json"],
        vec!["inspect", p(&d), "--env", "prod"],
    ] {
        let o = bin(&args);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!all.contains("CANARY"), "{args:?}");
        assert!(!all.contains("pw@") && !all.contains("q=1"), "{all}");
    }
}

#[test]
fn json_shape() {
    let d = fixture(true);
    let o = bin(&["inspect", p(&d), "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["schema"], "wisp-cli/1");
    assert_eq!(v["command"], "inspect");
    assert_eq!(v["collection"]["id"], "abcdefgh12345678");
    assert_eq!(v["collection"]["invalid"], 1);
    assert_eq!(v["truncated"], false);
    assert_eq!(v["environments"], serde_json::json!(["dev", "prod"]));
    let a = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "a.toml")
        .unwrap();
    assert_eq!(a["url"], "https://h.test/p");
    assert_eq!(a["auth"], "bearer");
    assert_eq!(a["headers"], serde_json::json!(["Authorization"]));
}

#[test]
fn exit_codes() {
    let d = fixture(true);
    assert_eq!(bin(&["--version"]).status.code(), Some(0));
    assert_eq!(bin(&["--help"]).status.code(), Some(0));
    assert_eq!(bin(&[]).status.code(), Some(2));
    assert_eq!(bin(&["validate", p(&d)]).status.code(), Some(2));
    assert_eq!(bin(&["explain", p(&d), "a.toml"]).status.code(), Some(2));
    assert_eq!(
        bin(&["inspect", p(&d), "--env", "nope"]).status.code(),
        Some(2)
    );
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(bin(&["inspect", p(&empty)]).status.code(), Some(3));
    let missing: PathBuf = empty.path().join("missing");
    assert_eq!(
        bin(&["inspect", missing.to_str().unwrap()]).status.code(),
        Some(3)
    );
}

fn snapshot(root: &Path, out: &mut Vec<(PathBuf, std::time::SystemTime, u64)>) {
    let mut es: Vec<_> = fs::read_dir(root).unwrap().map(|e| e.unwrap()).collect();
    es.sort_by_key(|e| e.path());
    for e in es {
        // not DirEntry::metadata: on Windows that is stale for directories
        let m = fs::metadata(e.path()).unwrap();
        out.push((e.path(), m.modified().unwrap(), m.len()));
        if m.is_dir() {
            snapshot(&e.path(), out);
        }
    }
}

#[test]
fn read_only_and_no_id_added() {
    let d = fixture(false);
    let mut before = Vec::new();
    snapshot(d.path(), &mut before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut stack = vec![d.path().to_path_buf()];
        let mut all = Vec::new();
        while let Some(x) = stack.pop() {
            all.push(x.clone());
            if x.is_dir() {
                for e in fs::read_dir(&x).unwrap() {
                    stack.push(e.unwrap().path());
                }
            }
        }
        for x in &all {
            let mode = if x.is_dir() { 0o555 } else { 0o444 };
            fs::set_permissions(x, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    for args in [
        vec!["inspect", p(&d)],
        vec!["inspect", p(&d), "--json"],
        vec!["validate", p(&d)],
        vec!["explain", p(&d), "a.toml"],
    ] {
        bin(&args);
    }
    let o = bin(&["inspect", p(&d), "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v["collection"]["id"].is_null());
    let mut after = Vec::new();
    snapshot(d.path(), &mut after);
    assert_eq!(before, after);
    assert!(
        !fs::read_to_string(d.path().join("collection.toml"))
            .unwrap()
            .contains("id =")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // let the tempdir clean up
        let mut stack = vec![d.path().to_path_buf()];
        while let Some(x) = stack.pop() {
            fs::set_permissions(&x, fs::Permissions::from_mode(0o755)).unwrap();
            if x.is_dir() {
                for e in fs::read_dir(&x).unwrap() {
                    stack.push(e.unwrap().path());
                }
            }
        }
    }
}

#[test]
fn bounded_at_2000_nodes() {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 1\n");
    for i in 0..2001 {
        w(d.path(), &format!("r{i:05}.toml"), &req("R", ""));
    }
    let o = bin(&["inspect", p(&d), "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["truncated"], true);
    assert_eq!(v["nodes"].as_array().unwrap().len(), 2000);
    assert_eq!(v["collection"]["requests"], 2001);
    assert!(v["collection"]["invalid"].is_null());
    let t = String::from_utf8(bin(&["inspect", p(&d)]).stdout).unwrap();
    assert!(t.contains("invalid: unknown (truncated)"));
}

#[test]
fn names_are_capped() {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 1\n");
    let long = "n".repeat(500);
    w(d.path(), "x.toml", &req(&long, ""));
    let o = bin(&["inspect", p(&d), "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["nodes"][0]["name"].as_str().unwrap().len(), 200);
}

#[test]
fn text_output_escapes_controls() {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 1\n");
    w(
        d.path(),
        "x.toml",
        &req("A\\u001b[2Jfake\\nforged line", ""),
    );
    let t = String::from_utf8(bin(&["inspect", p(&d)]).stdout).unwrap();
    assert!(!t.contains('\u{1b}'));
    assert!(t.contains("\\u{001b}[2Jfake\\u{000a}forged line"));
}
