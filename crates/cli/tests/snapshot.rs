//! `wisp-cli snapshot`: shape, bounds, history and read-only guarantees.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const ID: &str = "abcdefgh12345678";

struct Sandbox {
    coll: tempfile::TempDir,
    data: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Sandbox {
        let s = Sandbox {
            coll: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        };
        s.w(
            "collection.toml",
            &format!("format = 1\nid = \"{ID}\"\n\n[vars]\napi_token = \"CANARY_PLAIN\"\n"),
        );
        s.w(
            "environments/dev.toml",
            "[vars]\nhost = \"CANARY_ENVPLAIN\"\n",
        );
        s.w(
            "environments/dev.secrets.toml",
            "[vars]\nk = \"CANARY_SECRET\"\n",
        );
        s.w("environments/prod.toml", "secrets = \"keychain\"\n");
        s.w(
            "a.toml",
            "name = \"A\"\nmethod = \"GET\"\nurl = \"https://u:pw@h.test/p?q=qv\"\ndescription = \"about A\"\nexpected_status = [200, 201]\n\n[[headers]]\nname = \"Authorization\"\nvalue = \"CANARY_AUTH\"\nenabled = true\n\n[auth]\ntype = \"bearer\"\ntoken = \"CANARY_TOKEN\"\n",
        );
        s.w(
            "sub/b.toml",
            "name = \"B\"\nmethod = \"POST\"\nurl = \"https://h.test/b\"\n",
        );
        s.w("sub/bad.toml", "not toml [[[");
        s
    }

    fn w(&self, rel: &str, text: &str) {
        let p = self.coll.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn c(&self) -> &str {
        self.coll.path().to_str().unwrap()
    }

    fn go(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_wisp-cli"))
            .arg("snapshot")
            .arg(self.c())
            .args(args)
            .env("WISP_DATA_DIR", self.data.path())
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut a = args.to_vec();
        a.push("--json");
        let o = self.go(&a);
        assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
        serde_json::from_slice(&o.stdout).unwrap()
    }

    fn history_dir(&self) -> PathBuf {
        self.data.path().join("history").join(ID)
    }

    /// A history dir as a migrated store lays it out, with entries whose unmapped
    /// fields carry canaries.
    fn history(&self, migrated: bool) {
        let d = self.history_dir();
        fs::create_dir_all(&d).unwrap();
        if migrated {
            fs::write(d.join(".scrubbed-1"), "").unwrap();
        }
        let e1 = r#"{"id":"00000000000000000000000000000001","at":1763380000000,"request":"a.toml","method":"Post","url":"https://api.test/orders?token=CANARY_URLQ","outcome":"Completed","status":201,"environment":"staging","redacted":["CANARY_REDACTED"],"assertions":[{"label":"status equals","hash":"CANARY_HASH","state":"pass"}],"delivery":"responded","note":"CANARY_UNKNOWN"}"#;
        let e2 = r#"{"id":"00000000000000000000000000000002","at":1763380001000,"request":"sub/b.toml","method":"Get","url":"https://api.test/x","outcome":{"Failed":{"kind":"Dns"}},"redacted":["CANARY_REDACTED2"]}"#;
        fs::write(d.join("index.jsonl"), format!("{e1}\n{e2}\n")).unwrap();
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

type Listing = Vec<(PathBuf, std::time::SystemTime, u64)>;

fn listing(root: &Path, out: &mut Listing) {
    let mut es: Vec<_> = fs::read_dir(root).unwrap().map(|e| e.unwrap()).collect();
    es.sort_by_key(|e| e.path());
    for e in es {
        let m = fs::metadata(e.path()).unwrap();
        out.push((e.path(), m.modified().unwrap(), m.len()));
        if m.is_dir() {
            listing(&e.path(), out);
        }
    }
}

fn snap(root: &Path) -> Listing {
    let mut v = Vec::new();
    listing(root, &mut v);
    v
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn snapshot_json_has_exactly_the_documented_keys() {
    let s = Sandbox::new();
    let v = s.json(&[]);
    assert_eq!(
        keys(&v),
        [
            "collection",
            "command",
            "environments",
            "schema",
            "source",
            "tree",
            "tree_truncated",
            "truncated",
            "validation",
            "wisp_cli"
        ]
    );
    assert_eq!(v["schema"], "wisp-cli/1");
    assert_eq!(v["command"], "snapshot");
    assert_eq!(
        v["source"],
        "persisted files only; unsaved edits in the app are not included"
    );
    assert_eq!(v["truncated"], serde_json::json!([]));
    assert_eq!(v["collection"]["id"], ID);
    assert_eq!(v["validation"]["errors"], 1);
    let a = v["tree"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "a.toml")
        .unwrap();
    assert_eq!(a["has_description"], true);
    assert_eq!(a["expected_status"], serde_json::json!([200, 201]));
    assert_eq!(v["environments"][0]["name"], "dev");
    assert_eq!(
        v["environments"][0]["variables"],
        serde_json::json!(["host", "k"])
    );
}

#[test]
fn snapshot_request_and_recent_keys_appear_only_when_asked() {
    let s = Sandbox::new();
    s.history(true);
    let v = s.json(&["--request", "a.toml", "--recent", "5", "--env", "dev"]);
    assert_eq!(v["request"]["description"], "about A");
    assert_eq!(
        v["request"]["expected_status"],
        serde_json::json!([200, 201])
    );
    assert_eq!(v["request"]["environment"]["name"], "dev");
    assert_eq!(v["recent"].as_array().unwrap().len(), 2);
    let v = s.json(&["--recent", "1"]);
    assert!(v.get("request").is_none());
    assert_eq!(v["recent"].as_array().unwrap().len(), 1);
}

#[test]
fn snapshot_text_has_a_section_per_command() {
    let s = Sandbox::new();
    s.history(true);
    let o = s.go(&["--request", "a.toml", "--recent", "5"]);
    assert_eq!(o.status.code(), Some(0));
    let t = String::from_utf8_lossy(&o.stdout);
    for h in [
        "== inspect ==",
        "== validate ==",
        "== explain ==",
        "== recent ==",
    ] {
        assert!(t.contains(h), "{h}\n{t}");
    }
    assert!(t.contains("description: about A"), "{t}");
}

#[test]
fn snapshot_twice_is_byte_identical() {
    let s = Sandbox::new();
    s.history(true);
    for args in [
        vec!["--json", "--request", "a.toml", "--recent", "5"],
        vec!["--request", "a.toml", "--recent", "5"],
    ] {
        let a = s.go(&args);
        let b = s.go(&args);
        assert_eq!(a.stdout, b.stdout);
        assert!(!a.stdout.is_empty());
    }
}

#[test]
fn snapshot_usage_errors_exit_2() {
    let s = Sandbox::new();
    for args in [
        vec!["--recent", "0"],
        vec!["--recent", "51"],
        vec!["--recent", "x"],
        vec!["--keychain"],
        vec!["--history"],
    ] {
        assert_eq!(s.go(&args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(s.go(&["--env", "nope"]).status.code(), Some(2));
    assert_eq!(s.go(&["--request", "nope.toml"]).status.code(), Some(3));
}

#[test]
fn snapshot_unmigrated_history_gives_a_note_and_changes_nothing() {
    let s = Sandbox::new();
    s.history(false);
    let before = snap(s.data.path());
    let before_c = snap(s.coll.path());
    let o = s.go(&["--recent", "5", "--json"]);
    assert_eq!(o.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["recent"], serde_json::json!([]));
    assert!(stderr(&o).contains("history not yet migrated; open Wisp once"));
    assert_eq!(before, snap(s.data.path()));
    assert_eq!(before_c, snap(s.coll.path()));
    assert!(!s.history_dir().join(".scrubbed-1").exists());
}

#[test]
fn snapshot_migrated_history_lists_entries_and_changes_nothing() {
    let s = Sandbox::new();
    s.history(true);
    let before = snap(s.data.path());
    let v = s.json(&["--recent", "5"]);
    let r = v["recent"].as_array().unwrap();
    assert_eq!(r.len(), 2);
    // newest first
    assert_eq!(r[0]["request"], "sub/b.toml");
    assert_eq!(r[0]["outcome"], "failed");
    assert!(r[0].get("delivery").is_none());
    assert_eq!(r[1]["method"], "POST");
    assert_eq!(r[1]["url"], "https://api.test/orders");
    assert_eq!(r[1]["delivery"], "responded");
    assert_eq!(r[1]["status"], 201);
    assert_eq!(r[1]["environment"], "staging");
    assert_eq!(r[1]["assertions"][0]["label"], "status equals");
    assert_eq!(r[1]["assertions"][0]["state"], "pass");
    assert_eq!(before, snap(s.data.path()));
}

#[test]
fn snapshot_without_history_dir_or_id_says_so_and_creates_nothing() {
    let s = Sandbox::new();
    let before = snap(s.data.path());
    let o = s.go(&["--recent", "3", "--json"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stderr(&o).contains("no history"));
    assert_eq!(before, snap(s.data.path()));
    let s = Sandbox::new();
    s.w("collection.toml", "format = 1\n");
    let o = s.go(&["--recent", "3", "--json"]);
    assert!(stderr(&o).contains("no id"));
    assert!(
        !fs::read_to_string(s.coll.path().join("collection.toml"))
            .unwrap()
            .contains("id =")
    );
}

#[test]
fn snapshot_canaries_never_printed_and_nothing_written() {
    let s = Sandbox::new();
    s.history(true);
    let (c0, d0) = (snap(s.coll.path()), snap(s.data.path()));
    for args in [
        vec!["--recent", "50", "--request", "a.toml"],
        vec!["--recent", "50", "--request", "a.toml", "--json"],
        vec!["--recent", "50", "--request", "a.toml", "--env", "dev"],
        vec![
            "--recent",
            "50",
            "--request",
            "a.toml",
            "--env",
            "dev",
            "--json",
        ],
        vec![
            "--recent",
            "50",
            "--request",
            "a.toml",
            "--env",
            "prod",
            "--json",
        ],
    ] {
        let o = s.go(&args);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        let all = format!("{}{}", String::from_utf8_lossy(&o.stdout), stderr(&o));
        assert!(!all.contains("CANARY"), "{args:?}\n{all}");
        assert!(!all.contains("pw@"), "{all}");
    }
    assert_eq!((c0, d0), (snap(s.coll.path()), snap(s.data.path())));
}

#[test]
fn snapshot_stays_under_one_mebibyte_with_every_cap_exceeded() {
    let s = Sandbox::new();
    s.history(true);
    // tree: more than 2,000 nodes, each with many long header names
    let headers: String = (0..100)
        .map(|i| {
            format!(
                "\n[[headers]]\nname = \"{}{i}\"\nvalue = \"v\"\nenabled = true\n",
                "H".repeat(190)
            )
        })
        .collect();
    for i in 0..2100 {
        s.w(
            &format!("many/r{i}.toml"),
            &format!("name = \"R{i}\"\nmethod = \"GET\"\nurl = \"https://h.test/{i}\"\n{headers}"),
        );
    }
    // findings: more than 500
    for i in 0..600 {
        s.w(&format!("bad/b{i}.toml"), "not toml [[[");
    }
    // environments: more than 200, one with more than 500 variables
    for i in 0..210 {
        s.w(&format!("environments/e{i:03}.toml"), "[vars]\nx = \"1\"\n");
    }
    let vars: String = (0..600).map(|i| format!("v{i} = \"1\"\n")).collect();
    s.w("environments/a_big.toml", &format!("[vars]\n{vars}"));
    // the explained request: more than 500 variables and headers
    let rv: String = (0..600).map(|i| format!("rv{i} = \"1\"\n")).collect();
    let rh: String = (0..600)
        .map(|i| {
            format!("\n[[headers]]\nname = \"X-{i}\"\nvalue = \"{{{{rv{i}}}}}\"\nenabled = true\n")
        })
        .collect();
    s.w(
        "big.toml",
        &format!("name = \"Big\"\nmethod = \"GET\"\nurl = \"https://h.test/\"\n\n[vars]\n{rv}{rh}"),
    );
    let o = s.go(&["--json", "--request", "big.toml", "--recent", "50"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(o.stdout.len() <= 1024 * 1024, "{}", o.stdout.len());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let t: Vec<&str> = v["truncated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    for want in [
        "environments",
        "environment_variables",
        "findings",
        "tree",
        "request_variables",
        "request_headers",
    ] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    assert_eq!(v["tree_truncated"], true);
    assert_eq!(v["validation"]["findings_truncated"], true);
    assert!(v["environments"].as_array().unwrap().len() <= 200);
    assert!(v["tree"].as_array().unwrap().len() <= 2000);
}
