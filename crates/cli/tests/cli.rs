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
        vec!["validate", p(&d)],
        vec!["validate", p(&d), "--json"],
        vec!["explain", p(&d), "a.toml"],
        vec!["explain", p(&d), "a.toml", "--json"],
        vec!["explain", p(&d), "a.toml", "--env", "dev"],
        vec!["explain", p(&d), "a.toml", "--env", "dev", "--json"],
        vec!["explain", p(&d), "a.toml", "--env", "prod"],
        vec!["explain", p(&d), "a.toml", "--env", "prod", "--json"],
    ] {
        let o = bin(&args);
        let want = i32::from(args[0] == "validate");
        assert_eq!(o.status.code(), Some(want), "{args:?}");
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!all.contains("CANARY"), "{args:?}");
        assert!(!all.contains("pw@"), "{all}");
        if args[0] != "explain" {
            // explain shows query parameters by design; inspect never does
            assert!(!all.contains("q=1"), "{all}");
        }
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
    assert_eq!(bin(&["validate", p(&d)]).status.code(), Some(1));
    assert_eq!(bin(&["explain", p(&d), "a.toml"]).status.code(), Some(0));
    assert_eq!(bin(&["explain", p(&d), "nope.toml"]).status.code(), Some(3));
    assert_eq!(bin(&["explain", p(&d), "../a.toml"]).status.code(), Some(3));
    assert_eq!(bin(&["explain", p(&d)]).status.code(), Some(2));
    assert_eq!(
        bin(&["explain", p(&d), "a.toml", "--env", "nope"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        bin(&["inspect", p(&d), "--env", "nope"]).status.code(),
        Some(2)
    );
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(bin(&["inspect", p(&empty)]).status.code(), Some(3));
    assert_eq!(bin(&["validate", p(&empty)]).status.code(), Some(3));
    assert_eq!(
        bin(&["explain", p(&empty), "a.toml"]).status.code(),
        Some(3)
    );
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

fn out(args: &[&str]) -> String {
    String::from_utf8(bin(args).stdout).unwrap()
}

#[test]
fn validate_reports_everything_in_one_run() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(r, "badfolder/folder.toml", "vars = [[[");
    w(r, "badfolder/ok.toml", &req("Ok", ""));
    w(
        r,
        "broken.toml",
        "name = \"x\"\nmethod = \"NOPE\"\nurl = \"u\"\n",
    );
    w(
        r,
        "assert.toml",
        &req("As", "\n[[assert]]\nkind = \"bogus\"\n"),
    );
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        w(outside.path(), "x.toml", &req("Out", ""));
        std::os::unix::fs::symlink(outside.path(), r.join("escape")).unwrap();
    }
    let o = bin(&["validate", p(&d), "--json"]);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["schema"], "wisp-cli/1");
    assert_eq!(v["command"], "validate");
    let paths: Vec<String> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_owned())
        .collect();
    for want in ["badfolder/folder.toml", "broken.toml", "assert.toml"] {
        assert!(paths.iter().any(|x| x == want), "{want} in {paths:?}");
    }
    #[cfg(unix)]
    assert!(paths.iter().any(|x| x.starts_with("escape")), "{paths:?}");
    assert!(v["findings"][0]["severity"].is_string());
    let t = out(&["validate", p(&d)]);
    assert!(t.contains("error: broken.toml"), "{t}");
    assert!(t.contains("error(s)"));
}

#[test]
fn validate_clean_collection_exits_0() {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 1\n");
    w(d.path(), "a.toml", &req("A", ""));
    assert_eq!(bin(&["validate", p(&d)]).status.code(), Some(0));
}

fn explain_fixture(eol: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let f = |s: &str| s.replace('\n', eol);
    w(
        r,
        "collection.toml",
        &f("format = 1\n\n[vars]\nhost = \"h.test\"\n"),
    );
    w(
        r,
        "sub/r.toml",
        &f(
            "name = \"R\"\nmethod = \"GET\"\nurl = \"https://{{host}}/p/{{$uuid}}?k={{missing}}\"\n\n[[headers]]\nname = \"Authorization\"\nvalue = \"CANARY_AUTH\"\nenabled = true\n",
        ),
    );
    w(
        r,
        "environments/dev.toml",
        &f("[vars]\nhost = \"dev.test\"\n"),
    );
    d
}

#[test]
fn explain_golden() {
    for eol in ["\n", "\r\n"] {
        let d = explain_fixture(eol);
        let t = out(&["explain", p(&d), "sub/r.toml"]);
        let want = "\
request: sub/r.toml
environment: none
GET https://h.test/p/[$uuid, generated at send]?k=
headers:
  Authorization: hidden: credential name  (request)
auth: none  (collection)
settings:
  timeout: default  (default)
  follow_redirects: true  (default)
  verify_tls: true  (default)
body: none, 0 bytes
variables:
  host = h.test  (collection, used in url)
  $uuid = $uuid, generated at send  (dynamic, used in url)
problems:
  unresolved-var missing in url
";
        assert_eq!(t.replace("\r\n", "\n"), want, "eol {eol:?}\n{t}");
        let o = bin(&["explain", p(&d), "sub/r.toml", "--env", "dev", "--json"]);
        assert_eq!(o.status.code(), Some(0));
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["schema"], "wisp-cli/1");
        assert_eq!(v["command"], "explain");
        assert_eq!(v["environment"]["name"], "dev");
        assert_eq!(v["method"], "GET");
        assert!(!String::from_utf8_lossy(&o.stdout).contains("CANARY"));
    }
}

#[test]
fn closed_stdout_pipe_exits_cleanly() {
    use std::process::Stdio;
    let d = explain_fixture("\n");
    let mut c = Command::new(env!("CARGO_BIN_EXE_wisp-cli"))
        .args(["explain", p(&d), "sub/r.toml"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(c.stdout.take());
    let s = c.wait().unwrap();
    assert!(matches!(s.code(), Some(0 | 1)), "{s:?}");
}

#[test]
fn failure_messages_never_carry_raw_escapes() {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 1\n");
    w(
        d.path(),
        "a.toml",
        "name = \"x\"\nmethod = \"\\u001b[31mEVIL\"\nurl = \"u\"\n",
    );
    let o = bin(&["explain", p(&d), "a.toml"]);
    assert!(!o.status.success());
    assert!(
        !o.stderr.contains(&0x1b),
        "{:?}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("EVIL"));
}

#[test]
fn explain_keychain_secret_shadows_lower_scope() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(
        r,
        "collection.toml",
        "format = 1\n\n[vars]\ntok = \"FALLBACK-CANARY\"\n",
    );
    w(
        r,
        "environments/prod.toml",
        "secrets = \"keychain\"\nsecret_names = [\"tok\"]\n",
    );
    w(
        r,
        "a.toml",
        &req("A", "\n[[headers]]\nname = \"X-T\"\nvalue = \"{{tok}}\"\n"),
    );
    for args in [
        vec!["explain", p(&d), "a.toml", "--env", "prod"],
        vec!["explain", p(&d), "a.toml", "--env", "prod", "--json"],
    ] {
        let o = bin(&args);
        let t = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(!t.contains("FALLBACK-CANARY"), "{t}");
        assert!(t.contains("tok"), "{t}");
        assert!(t.contains("secret") || t.contains("Secret"), "{t}");
    }
}
