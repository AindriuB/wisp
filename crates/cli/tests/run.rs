//! `wisp-cli run` against a local mock server. Every test name carries `run`.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const CANARIES: [&str; 4] = [
    "CANARY_SECRET",
    "CANARY_ENVPLAIN",
    "CANARY_COLLPLAIN",
    "CANARY_KC",
];

struct Sandbox {
    coll: tempfile::TempDir,
    data: tempfile::TempDir,
    tmp: tempfile::TempDir,
}

impl Sandbox {
    fn new(id: bool) -> Self {
        let s = Self {
            coll: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            tmp: tempfile::tempdir().unwrap(),
        };
        let idline = if id {
            "id = \"abcdefgh12345678\"\n"
        } else {
            ""
        };
        s.w(
            "collection.toml",
            &format!("format = 1\n{idline}\n[vars]\napi_token = \"CANARY_COLLPLAIN\"\n"),
        );
        s.w(
            "environments/dev.toml",
            "[vars]\napi_token = \"CANARY_ENVPLAIN\"\n",
        );
        s.w(
            "environments/dev.secrets.toml",
            "[vars]\nk = \"CANARY_SECRET\"\n",
        );
        s.w(
            "environments/prod.toml",
            "secrets = \"keychain\"\nsecret_names = [\"kc\"]\n",
        );
        s
    }

    fn w(&self, rel: &str, text: &str) {
        let p = self.coll.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn root(&self) -> &str {
        self.coll.path().to_str().unwrap()
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_wisp-cli"));
        c.args(args)
            .env("WISP_DATA_DIR", self.data.path())
            .env("TMPDIR", self.tmp.path())
            .env("TMP", self.tmp.path())
            .env("TEMP", self.tmp.path())
            .env_remove("WISP_KEYCHAIN_SERVICE");
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    /// `wisp-cli run <root> <req> <extra...>`.
    fn go(&self, req: &str, extra: &[&str]) -> Output {
        let mut a = vec!["run", self.root(), req];
        a.extend_from_slice(extra);
        self.run(&a)
    }

    fn json(&self, req: &str, extra: &[&str]) -> (i32, Value) {
        let mut a = extra.to_vec();
        a.push("--json");
        let o = self.go(req, &a);
        let v = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
        (o.status.code().unwrap(), v)
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A server answering every connection with `status`, echoing the canaries in a header
/// and the whole request head in the body.
fn serve(status: u16) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let body = format!("{}\n{}", String::from_utf8_lossy(&buf), CANARIES.join(" "));
                let _ = s.write_all(
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nX-Echo: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        CANARIES.join(" "),
                        body.len()
                    )
                    .as_bytes(),
                );
            });
        }
    });
    addr
}

/// A server that accepts and never answers.
fn silent() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in l.incoming().flatten() {
            held.push(s);
        }
    });
    addr
}

/// An address nothing listens on. Port 0 is never connectable: connect() to it fails on
/// Linux (refused), macOS and Windows (address not available). Unlike binding a port and
/// dropping it, no other process or test can take it between the call and the connect.
fn closed() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

/// A request reading every canary kind: secret in path, query, header and auth, a plain
/// variable named `api_token`, with userinfo in the URL.
fn canary_request(addr: SocketAddr, method: &str, extra: &str) -> String {
    format!(
        "name = \"c\"\nmethod = \"{method}\"\nurl = \"http://u:pw@{addr}/{{{{k}}}}/{{{{api_token}}}}?key={{{{k}}}}&t={{{{api_token}}}}\"\n{extra}\n[[headers]]\nname = \"X-Key\"\nvalue = \"{{{{k}}}}\"\nenabled = true\n\n[auth]\ntype = \"bearer\"\ntoken = \"{{{{k}}}}\"\n"
    )
}

fn plain_request(addr: SocketAddr, method: &str, extra: &str) -> String {
    format!("name = \"p\"\nmethod = \"{method}\"\nurl = \"http://{addr}/p\"\n{extra}")
}

const STATUS_201: &str = "[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 201\n";

#[test]
fn run_exit_0_passed() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(200), "GET", ""));
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 0);
    assert_eq!(v["result"], "passed");
    assert_eq!(v["exit"], 0);
    assert_eq!(v["status"], 200);
    assert_eq!(v["status_ok"], true);
    assert_eq!(v["delivery"], "responded");
}

#[test]
fn run_exit_1_assertion_failed() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(200), "GET", STATUS_201));
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 1);
    assert_eq!(v["result"], "assertion_failed");
    assert_eq!(v["assertions"][0]["state"], "fail");
    assert_eq!(v["assertions"][0]["label"], "status equals");
}

#[test]
fn run_exit_4_not_prepared() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &plain_request(
            closed(),
            "GET",
            "\n[[headers]]\nname = \"X\"\nvalue = \"{{missing}}\"\nenabled = true\n",
        ),
    );
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 4);
    assert_eq!(v["result"], "not_prepared");
    assert_eq!(v["delivery"], "not_sent");
    assert_eq!(v["retry_safe"], true);
    assert_eq!(v["status"], Value::Null);
    assert_eq!(v["problems"][0]["kind"], "unresolved-var");
    assert_eq!(v["problems"][0]["name"], "missing");
}

#[test]
fn run_exit_5_transport_failed_not_sent() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(closed(), "POST", ""));
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 5);
    assert_eq!(v["result"], "transport_failed");
    assert_eq!(v["delivery"], "not_sent");
    assert_eq!(v["retry_safe"], true);
    assert_eq!(v["error"]["kind"], "connect");
}

#[test]
fn run_exit_6_outcome_unknown_and_retry_safe_follows_the_method() {
    let s = Sandbox::new(true);
    let addr = silent();
    let slow = "\n[settings]\ntimeout_ms = 300\n";
    s.w("post.toml", &plain_request(addr, "POST", slow));
    s.w("get.toml", &plain_request(addr, "GET", slow));
    let (code, v) = s.json("post.toml", &[]);
    assert_eq!(code, 6);
    assert_eq!(v["result"], "outcome_unknown");
    assert_eq!(v["delivery"], "unknown");
    assert_eq!(v["retry_safe"], false);
    assert_eq!(v["error"]["kind"], "timeout");
    let (code, v) = s.json("get.toml", &[]);
    assert_eq!(code, 6);
    assert_eq!(v["retry_safe"], true);
}

#[test]
fn run_exit_7_status_unexpected() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(500), "GET", ""));
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 7);
    assert_eq!(v["result"], "status_unexpected");
    assert_eq!(v["status_ok"], false);
    assert_eq!(v["delivery"], "responded");
}

#[test]
fn run_absent_expected_status_with_404_exits_7() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(404), "GET", ""));
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 7);
    assert_eq!(v["expected_status"], serde_json::json!([]));
}

#[test]
fn run_declared_expected_status_makes_404_acceptable() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &format!(
            "expected_status = [404]\n{}",
            plain_request(serve(404), "GET", "")
        ),
    );
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 0);
    assert_eq!(v["expected_status"], serde_json::json!([404]));
    assert_eq!(v["status_ok"], true);
}

#[test]
fn run_bad_status_and_failed_assertion_exits_7_with_both_in_json() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(500), "GET", STATUS_201));
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 7);
    assert_eq!(v["status_ok"], false);
    assert_eq!(v["assertions"][0]["state"], "fail");
}

#[test]
fn run_json_golden_field_set() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &plain_request(
            serve(200),
            "GET",
            &format!("{STATUS_201}\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 200\n"),
        ),
    );
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 1);
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut want = vec![
        "schema",
        "command",
        "request",
        "environment",
        "method",
        "url",
        "result",
        "exit",
        "delivery",
        "retry_safe",
        "status",
        "expected_status",
        "status_ok",
        "timing_ms",
        "size",
        "error",
        "assertions",
        "dynamic",
        "history",
        "problems",
    ];
    want.sort_unstable();
    assert_eq!(keys, want);
    assert_eq!(v["schema"], "wisp-cli/1");
    assert_eq!(v["command"], "run");
    assert_eq!(v["request"], "a.toml");
    assert_eq!(v["environment"], Value::Null);
    assert_eq!(v["method"], "GET");
    assert!(v["url"].as_str().unwrap().ends_with("/p"));
    assert_eq!(v["result"], "assertion_failed");
    assert_eq!(v["retry_safe"], true);
    assert_eq!(v["error"], Value::Null);
    assert_eq!(v["history"], "off");
    assert_eq!(v["problems"], serde_json::json!([]));
    assert_eq!(v["dynamic"], serde_json::json!([]));
    let t = v["timing_ms"].as_object().unwrap();
    assert!(t["ttfb"].is_u64() && t["total"].is_u64());
    assert!(v["size"].is_u64());
    assert_eq!(
        v["assertions"],
        serde_json::json!([
            {"label": "status equals", "state": "fail"},
            {"label": "status equals", "state": "pass"},
        ])
    );
}

#[test]
fn run_unreadable_inputs_exit_3_to_stderr_only() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(closed(), "GET", ""));
    s.w("bad.toml", "not toml [[[");
    for args in [
        vec!["run", s.root(), "nope.toml"],
        vec!["run", s.root(), "../a.toml"],
        vec!["run", s.root(), "bad.toml"],
        vec!["run", s.root(), "a.toml", "--env", "nope"],
        vec!["run", "/definitely/not/here", "a.toml"],
    ] {
        for json in [false, true] {
            let mut a = args.clone();
            if json {
                a.push("--json");
            }
            let o = s.run(&a);
            assert_eq!(o.status.code(), Some(3), "{a:?}");
            assert!(o.stdout.is_empty(), "{a:?}");
            assert!(!o.stderr.is_empty(), "{a:?}");
        }
    }
}

#[test]
fn run_usage_errors_exit_2_to_stderr_only() {
    let s = Sandbox::new(true);
    for args in [
        vec!["run", s.root()],
        vec!["run", s.root(), "a.toml", "--bogus"],
        vec!["explain", s.root(), "a.toml", "--keychain"],
        vec!["inspect", s.root(), "--history"],
    ] {
        let o = s.run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn run_keychain_env_without_flag_exits_4_with_the_message() {
    let s = Sandbox::new(true);
    let addr = serve(200);
    s.w(
        "a.toml",
        &plain_request(
            addr,
            "GET",
            "\n[[headers]]\nname = \"X\"\nvalue = \"{{kc}}\"\nenabled = true\n",
        ),
    );
    let (code, v) = s.json("a.toml", &["--env", "prod"]);
    assert_eq!(code, 4);
    assert_eq!(v["problems"][0]["kind"], "keychain-secret");
    assert_eq!(
        v["problems"][0]["message"],
        "secret kc is in the keychain; pass --keychain"
    );
    let o = s.go("a.toml", &["--env", "prod"]);
    assert_eq!(o.status.code(), Some(4));
    assert!(text(&o).contains("secret kc is in the keychain; pass --keychain"));
}

#[test]
fn run_history_text_says_skipped_for_not_sent_and_for_no_id() {
    let missing = "\n[[headers]]\nname = \"X\"\nvalue = \"{{missing}}\"\nenabled = true\n";
    for (id, want) in [
        (true, "history: skipped (not sent)"),
        (false, "history: skipped (collection has no id)"),
    ] {
        let s = Sandbox::new(id);
        s.w("a.toml", &plain_request(closed(), "GET", missing));
        let o = s.go("a.toml", &["--history"]);
        assert_eq!(o.status.code(), Some(4));
        assert!(text(&o).contains(want), "id={id}: {}", text(&o));
        assert!(!text(&o).contains("history: failed"));
    }
}

#[test]
fn run_text_status_line_has_reason_size_and_no_delivery_line() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &plain_request(serve(200), "GET", "expected_status = [200]\n"),
    );
    let o = s.go("a.toml", &[]);
    let t = text(&o);
    let line = t.lines().find(|l| l.starts_with("status ")).unwrap();
    assert!(
        line.starts_with("status 200 OK (expected 200)   "),
        "{line}"
    );
    assert!(line.contains(" ms   ") && line.ends_with(" B"), "{line}");
    assert!(!t.contains("delivery:"), "{t}");
}

#[test]
fn run_oauth2_authorization_code_exits_4() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &plain_request(
            closed(),
            "GET",
            "\n[auth]\ntype = \"oauth2_auth_code\"\nauth_url = \"http://127.0.0.1:1/a\"\ntoken_url = \"http://127.0.0.1:1/t\"\nclient_id = \"c\"\n",
        ),
    );
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 4);
    assert_eq!(v["problems"][0]["kind"], "interactive-auth");
    assert_eq!(v["delivery"], "not_sent");
}

#[test]
fn run_text_output_has_one_line_per_item() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &format!(
            "expected_status = [200]\n{}",
            plain_request(
                serve(200),
                "GET",
                &format!(
                    "{STATUS_201}\n[[headers]]\nname = \"X\"\nvalue = \"{{{{$uuid}}}}\"\nenabled = true\n"
                )
            )
        ),
    );
    let o = s.go("a.toml", &["--env", "dev"]);
    assert_eq!(o.status.code(), Some(1));
    let t = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines: Vec<&str> = t.lines().collect();
    assert!(lines[0].starts_with("GET http://127.0.0.1:"), "{t}");
    assert!(lines[0].ends_with("   env: dev"), "{t}");
    assert!(lines[1].starts_with("status 200 OK (expected 200)"), "{t}");
    assert!(lines.contains(&"checks 0/1: fail \"status equals\""), "{t}");
    assert!(
        lines.iter().any(|l| l.starts_with("dynamic $uuid = ")),
        "{t}"
    );
    assert!(lines.contains(&"history: off"), "{t}");
    assert_eq!(*lines.last().unwrap(), "result: assertion_failed (exit 1)");
}

#[test]
fn run_url_shows_secret_markers_without_userinfo_or_query() {
    let s = Sandbox::new(true);
    let addr = serve(200);
    s.w(
        "a.toml",
        &format!("name = \"c\"\nmethod = \"GET\"\nurl = \"http://u:pw@{addr}/{{{{k}}}}/x?key={{{{k}}}}\"\n"),
    );
    let (code, v) = s.json("a.toml", &["--env", "dev"]);
    assert_eq!(code, 0);
    let url = v["url"].as_str().unwrap();
    assert!(url.contains("{secret k}"), "{url}");
    assert!(!url.contains("pw"), "{url}");
    assert!(!url.contains('?') && !url.contains("key="), "{url}");
    assert!(!url.contains("CANARY"), "{url}");
}

#[test]
fn run_dynamic_values_are_listed() {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &plain_request(
            serve(200),
            "GET",
            "\n[[headers]]\nname = \"X\"\nvalue = \"{{$uuid}}\"\nenabled = true\n",
        ),
    );
    let (_, v) = s.json("a.toml", &[]);
    assert_eq!(v["dynamic"][0]["name"], "$uuid");
    assert_eq!(v["dynamic"][0]["value"].as_str().unwrap().len(), 36);
}

#[test]
fn run_history_without_an_id_is_skipped_and_assigns_none() {
    let s = Sandbox::new(false);
    s.w("a.toml", &plain_request(serve(200), "GET", ""));
    let (code, v) = s.json("a.toml", &["--history"]);
    assert_eq!(code, 0);
    assert_eq!(v["history"], "skipped (collection has no id)");
    assert!(
        !fs::read_to_string(s.coll.path().join("collection.toml"))
            .unwrap()
            .contains("id =")
    );
    assert!(fs::read_dir(s.data.path()).unwrap().next().is_none());
    let o = s.go("a.toml", &["--history"]);
    assert!(text(&o).contains("history: skipped (collection has no id)"));
}

#[test]
fn run_history_appends_one_parseable_entry() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(200), "GET", ""));
    let (_, v) = s.json("a.toml", &["--history"]);
    assert_eq!(v["history"], "recorded");
    let index = s.data.path().join("history/abcdefgh12345678/index.jsonl");
    let lines: Vec<String> = fs::read_to_string(index)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(lines.len(), 1);
    let e: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(e["delivery"], "responded");
}

type Snap = Vec<(PathBuf, u64, Option<std::time::SystemTime>)>;

fn tree(root: &Path, out: &mut Snap) {
    let mut es: Vec<_> = fs::read_dir(root).unwrap().map(|e| e.unwrap()).collect();
    es.sort_by_key(|e| e.path());
    for e in es {
        let m = fs::metadata(e.path()).unwrap();
        out.push((e.path(), m.len(), m.modified().ok()));
        if m.is_dir() {
            tree(&e.path(), out);
        }
    }
}

fn snap(s: &Sandbox) -> [Snap; 3] {
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut c = Vec::new();
    tree(s.coll.path(), &mut a);
    tree(s.data.path(), &mut b);
    tree(s.tmp.path(), &mut c);
    [a, b, c]
}

#[test]
fn run_without_history_leaves_collection_data_and_spill_dirs_unchanged() {
    for id in [true, false] {
        let s = Sandbox::new(id);
        s.w("a.toml", &canary_request(serve(200), "GET", ""));
        let before = snap(&s);
        for args in [vec!["--env", "dev"], vec!["--env", "dev", "--json"], vec![]] {
            s.go("a.toml", &args);
        }
        assert_eq!(snap(&s), before, "id={id}");
        assert!(before[2].is_empty());
    }
}

#[test]
fn run_with_history_adds_files_only_under_history_id() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(200), "GET", ""));
    let before = snap(&s);
    s.go("a.toml", &["--history"]);
    let after = snap(&s);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[2], before[2]);
    let prefix = s.data.path().join("history/abcdefgh12345678");
    let have: Vec<&PathBuf> = before[1].iter().map(|(p, _, _)| p).collect();
    for (p, _, _) in &after[1] {
        assert!(
            have.contains(&p) || p.starts_with(s.data.path().join("history")),
            "{p:?}"
        );
    }
    assert!(after[1].iter().any(|(p, _, _)| p.starts_with(&prefix)));
    assert!(
        after[1]
            .iter()
            .filter(|(p, _, _)| !have.contains(&p) && p.is_file())
            .all(|(p, _, _)| p.starts_with(&prefix) || p.extension().is_some_and(|e| e == "lock")),
    );
}

fn scan(s: &Sandbox, o: &Output, what: &str) {
    let all = text(o);
    for c in CANARIES {
        assert!(!all.contains(c), "{what}: {c} in output:\n{all}");
    }
    assert!(!all.contains("pw@"), "{what}: userinfo");
    let mut stack = vec![s.data.path().to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if name == "index.jsonl" || name.ends_with(".req.json") {
                let body = fs::read_to_string(&p).unwrap();
                for c in CANARIES {
                    assert!(!body.contains(c), "{what}: {c} in {name}:\n{body}");
                }
            }
        }
    }
}

#[test]
fn run_canaries_never_reach_output_or_history() {
    let s = Sandbox::new(true);
    let ok = serve(200);
    let bad = serve(500);
    let quiet = silent();
    s.w("c0.toml", &canary_request(ok, "GET", ""));
    s.w("c1.toml", &canary_request(ok, "GET", STATUS_201));
    s.w(
        "c4.toml",
        &canary_request(
            ok,
            "GET",
            "\n[[headers]]\nname = \"X-M\"\nvalue = \"{{missing}}\"\nenabled = true\n",
        ),
    );
    s.w("c5.toml", &canary_request(closed(), "POST", ""));
    s.w(
        "c6.toml",
        &canary_request(quiet, "POST", "\n[settings]\ntimeout_ms = 300\n"),
    );
    s.w("c7.toml", &canary_request(bad, "GET", ""));
    for n in [0, 1, 4, 5, 6, 7] {
        let req = format!("c{n}.toml");
        for json in [false, true] {
            let mut extra = vec!["--env", "dev", "--history"];
            if json {
                extra.push("--json");
            }
            let o = s.go(&req, &extra);
            assert_eq!(o.status.code(), Some(n), "{req} json={json}");
            scan(&s, &o, &format!("{req} json={json}"));
        }
    }
}

#[test]
fn run_two_processes_both_append_to_one_index() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(200), "GET", ""));
    let children: Vec<_> = (0..2)
        .map(|_| {
            s.cmd(&["run", s.root(), "a.toml", "--history"])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for c in children {
        let o = c.wait_with_output().unwrap();
        assert_eq!(o.status.code(), Some(0));
    }
    let index = s.data.path().join("history/abcdefgh12345678/index.jsonl");
    let lines: Vec<String> = fs::read_to_string(index)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(lines.len(), 2);
    for l in lines {
        serde_json::from_str::<Value>(&l).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn run_output_failure_exits_8() {
    let s = Sandbox::new(true);
    s.w("a.toml", &plain_request(serve(200), "GET", ""));
    let full = fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let o = s
        .cmd(&["run", s.root(), "a.toml"])
        .stdout(full)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(8));
}

#[test]
fn run_help_lists_the_command() {
    let o = Sandbox::new(true).run(&["--help"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("run <collection> <request-path>"));
}

// ---- TLS trust ----

struct TlsServer {
    port: u16,
    /// TCP connections accepted, counted before any handshake.
    connections: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ca_pem: String,
}

/// A TLS server whose leaf (SAN 127.0.0.1, serverAuth, valid now-1h to now+30d) is signed by a
/// fresh private CA. Counts the requests that complete a handshake.
fn serve_private_ca() -> TlsServer {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering};
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let at = |offset: i64| {
        let t = secs + offset;
        let days = t.div_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
        let sod = t.rem_euclid(86_400);
        rcgen::date_time_ymd(y, m, d) + std::time::Duration::from_secs(sod as u64)
    };
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Wisp test CA");
    ca_params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    ca_params.not_before = at(-3600);
    ca_params.not_after = at(30 * 86_400);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();

    let mut params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()]).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = at(-3600);
    params.not_after = at(30 * 86_400);
    let key = rcgen::KeyPair::generate().unwrap();
    let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
    let leaf = params.signed_by(&key, &issuer).unwrap();
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap(),
    );
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted = connections.clone();
    std::thread::spawn(move || {
        for tcp in l.incoming().flatten() {
            accepted.fetch_add(1, Ordering::SeqCst);
            let config = config.clone();
            std::thread::spawn(move || {
                let conn = rustls::ServerConnection::new(config).unwrap();
                let mut s = rustls::StreamOwned::new(conn, tcp);
                let mut buf = [0u8; 4096];
                match s.read(&mut buf) {
                    Ok(n) if n > 0 => {}
                    _ => return,
                }
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
                s.conn.send_close_notify();
                let _ = s.flush();
            });
        }
    });
    TlsServer {
        port,
        connections,
        ca_pem: pem(ca_cert.der()),
    }
}

/// PEM-encode a certificate (rcgen's `pem` feature is not enabled in the workspace).
fn pem(der: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for c in der.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                b64.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                b64.push('=');
            }
        }
    }
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).unwrap());
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

fn trust_sandbox(srv: &TlsServer) -> (Sandbox, String) {
    let s = Sandbox::new(true);
    s.w(
        "a.toml",
        &format!(
            "name = \"t\"\nmethod = \"GET\"\nurl = \"https://127.0.0.1:{}/\"\n",
            srv.port
        ),
    );
    let ca = s.tmp.path().join("ca.pem");
    fs::write(&ca, &srv.ca_pem).unwrap();
    (s, ca.to_str().unwrap().to_owned())
}

#[test]
fn run_trust_no_flags_fails_transport() {
    let srv = serve_private_ca();
    let (s, _) = trust_sandbox(&srv);
    let (code, v) = s.json("a.toml", &[]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["result"], "transport_failed");
}

#[test]
fn run_trust_ca_bundle_passes() {
    let srv = serve_private_ca();
    let (s, ca) = trust_sandbox(&srv);
    let (code, v) = s.json("a.toml", &["--ca-bundle", &ca]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn run_trust_no_system_trust_with_bundle_passes() {
    let srv = serve_private_ca();
    let (s, ca) = trust_sandbox(&srv);
    let (code, v) = s.json("a.toml", &["--no-system-trust", "--ca-bundle", &ca]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn run_trust_no_system_trust_without_file_fails_transport() {
    let srv = serve_private_ca();
    let (s, _) = trust_sandbox(&srv);
    let (code, v) = s.json("a.toml", &["--no-system-trust"]);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn run_trust_bad_bundle_exits_3_without_sending() {
    let srv = serve_private_ca();
    let (s, _) = trust_sandbox(&srv);
    let bad = s.tmp.path().join("bad.pem");
    fs::write(&bad, "CANARY_GARBAGE not a pem").unwrap();
    let o = s.go("a.toml", &["--ca-bundle", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("bad.pem"), "{t}");
    assert!(!t.contains("CANARY_GARBAGE"), "{t}");
    assert_eq!(srv.connections.load(std::sync::atomic::Ordering::SeqCst), 0);
    let o = s.go("a.toml", &["--ca-bundle", bad.to_str().unwrap(), "--json"]);
    assert_eq!(o.status.code(), Some(3));
}

#[test]
fn run_trust_flag_on_inspect_exits_2() {
    let s = Sandbox::new(true);
    let o = s.run(&["inspect", s.root(), "--ca-bundle", "x.pem"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    let o = s.run(&["inspect", s.root(), "--no-system-trust"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn run_trust_help_lists_flags() {
    let o = Sandbox::new(true).run(&["--help"]);
    let t = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(t.contains("--ca-bundle") && t.contains("--no-system-trust"));
}
