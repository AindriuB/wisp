//! Postman importer: test translation and hardening.

use serde_json::{Value, json};
use wisp_core::assert::{Assertion, Expected, Op};
use wisp_core::request::{Auth, Body, GraphQlBody, Request};
use wisp_storage::import::{ImportError, ImportItem, ImportResult, postman};

const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

fn collection(items: Value, extra: Value) -> Vec<u8> {
    let mut doc = json!({"info": {"name": "c", "schema": SCHEMA}, "item": items});
    if let Value::Object(e) = extra {
        for (k, v) in e {
            doc[k] = v;
        }
    }
    serde_json::to_vec(&doc).unwrap()
}

fn request_with(request: Value, events: Value) -> Value {
    json!({"name": "r", "request": request, "event": events})
}

fn test_event(lines: &[&str]) -> Value {
    json!([{"listen": "test", "script": {"exec": lines}}])
}

fn import_tests(lines: &[&str]) -> (Vec<Assertion>, ImportResult) {
    let item = request_with(
        json!({"method": "GET", "url": "http://h/x"}),
        test_event(lines),
    );
    let r = postman::parse(&collection(json!([item]), json!({}))).unwrap();
    (first(&r).assertions.clone(), r)
}

fn first(r: &ImportResult) -> &Request {
    match &r.items[0] {
        ImportItem::Request { request, .. } => request,
        ImportItem::Folder { .. } => panic!("folder"),
    }
}

fn one(stmt: &str) -> Assertion {
    let src = format!("pm.test(\"t\", function () {{ {stmt} }});");
    let (a, r) = import_tests(&[&src]);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(a.len(), 1);
    a.into_iter().next().unwrap()
}

#[test]
fn pm_status_have_status() {
    assert_eq!(
        one("pm.response.to.have.status(201);"),
        Assertion::Status {
            op: Op::Equals,
            expected: Expected::Int(201)
        }
    );
}

#[test]
fn pm_status_code_eql_and_equal() {
    let want = Assertion::Status {
        op: Op::Equals,
        expected: Expected::Int(200),
    };
    assert_eq!(one("pm.expect(pm.response.code).to.eql(200);"), want);
    assert_eq!(one("pm.expect(pm.response.code).to.equal(200);"), want);
}

#[test]
fn pm_header_exists() {
    assert_eq!(
        one("pm.response.to.have.header(\"X-Id\");"),
        Assertion::Header {
            name: "X-Id".into(),
            op: Op::Exists,
            expected: Expected::Bool(true)
        }
    );
}

#[test]
fn pm_header_equals_two_forms() {
    let want = Assertion::Header {
        name: "Content-Type".into(),
        op: Op::Equals,
        expected: Expected::Str("text/plain".into()),
    };
    assert_eq!(
        one("pm.response.to.have.header('Content-Type', 'text/plain');"),
        want
    );
    assert_eq!(
        one("pm.expect(pm.response.headers.get(\"Content-Type\")).to.eql(\"text/plain\");"),
        want
    );
}

#[test]
fn pm_header_include() {
    assert_eq!(
        one("pm.expect(pm.response.headers.get(\"Content-Type\")).to.include(\"json\");"),
        Assertion::Header {
            name: "Content-Type".into(),
            op: Op::Contains,
            expected: Expected::Str("json".into())
        }
    );
}

#[test]
fn pm_json_path_literals() {
    let path = "$.a.b[0].c";
    let json = |e| Assertion::Json {
        path: path.into(),
        op: Op::Equals,
        expected: e,
    };
    let t = |lit: &str| {
        one(&format!(
            "pm.expect(pm.response.json().a.b[0].c).to.eql({lit});"
        ))
    };
    assert_eq!(t("5"), json(Expected::Int(5)));
    assert_eq!(t("1.5"), json(Expected::Float(1.5)));
    assert_eq!(t("true"), json(Expected::Bool(true)));
    assert_eq!(t("false"), json(Expected::Bool(false)));
    assert_eq!(
        t("\"a\\\"b\\\\c\\n\""),
        json(Expected::Str("a\"b\\c\n".into()))
    );
    assert_eq!(t("'it\\'s'"), json(Expected::Str("it's".into())));
}

#[test]
fn pm_arrow_function_and_multiple_statements() {
    let (a, r) = import_tests(&[
        "pm.test(\"a\", () => {",
        "  pm.response.to.have.status(200);",
        "  pm.response.to.have.header(\"X\");",
        "});",
        "pm.test(\"b\", function() { pm.expect(pm.response.code).to.eql(404) })",
    ]);
    assert!(r.warnings.is_empty());
    assert_eq!(a.len(), 3);
}

#[test]
fn pm_mixed_test_not_translated() {
    let (a, r) = import_tests(&[
        "pm.test(\"ok\", function () { pm.response.to.have.status(200); });",
        "pm.test(\"mixed\", function () {",
        "  pm.response.to.have.status(200);",
        "  pm.environment.set(\"k\", \"v\");",
        "});",
    ]);
    assert!(a.is_empty(), "{a:?}");
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(
        r.warnings[0].message,
        "2 of 2 tests not translated; scripts are not run"
    );
}

#[test]
fn pm_unsupported_forms_not_translated() {
    for stmt in [
        "pm.response.to.have.status(\"OK\");",
        "pm.response.to.be.ok;",
        "pm.expect(pm.response.code).to.be.oneOf([200]);",
        "pm.expect(pm.response.json().a).to.eql(1e3);",
        "pm.expect(pm.response.json().a).to.eql(x);",
        "pm.response.to.have.status(200).and.have.header(\"a\");",
        "pm.response.to.have.status(99);",
    ] {
        let src = format!("pm.test(\"t\", function () {{ {stmt} }});");
        let (a, r) = import_tests(&[&src]);
        assert!(a.is_empty(), "{stmt}");
        assert_eq!(r.warnings.len(), 1, "{stmt}");
    }
}

#[test]
fn pm_warning_never_echoes_script_text() {
    let (_, r) = import_tests(&[
        "pm.test(\"CANARY-NAME\", function () { pm.environment.set(\"CANARY-KEY\", \"CANARY-VAL\"); });",
        "console.log(\"CANARY-LOG\");",
    ]);
    assert_eq!(r.warnings.len(), 1);
    assert!(!r.warnings[0].message.contains("CANARY"));
    assert!(!r.warnings[0].at.contains("CANARY"));
    assert_eq!(
        r.warnings[0].message,
        "1 of 1 tests not translated; scripts are not run"
    );
}

#[test]
fn pm_prerequest_and_folder_scripts_keep_warning() {
    let req = request_with(
        json!({"method": "GET", "url": "http://h/x"}),
        json!([{"listen": "prerequest", "script": {"exec": ["x();"]}}]),
    );
    let folder = json!({"name": "F", "item": [req],
        "event": [{"listen": "test", "script": {"exec": ["pm.test('a', () => {});"]}}]});
    let r = postman::parse(&collection(json!([folder]), json!({}))).unwrap();
    let msgs: Vec<&str> = r.warnings.iter().map(|w| w.message.as_str()).collect();
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs.iter().all(|m| m.contains("are not imported")));
}

#[test]
fn pm_script_over_64kib_is_not_parsed() {
    let mut lines =
        vec!["pm.test(\"a\", function () { pm.response.to.have.status(200); });".to_string()];
    lines.push(format!("// {}", "x".repeat(64 * 1024)));
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let (a, r) = import_tests(&refs);
    assert!(a.is_empty());
    assert_eq!(
        r.warnings[0].message,
        "1 of 1 tests not translated; scripts are not run"
    );
}

#[test]
fn pm_hostile_scripts_terminate() {
    for src in [
        "pm.test(\"a\", function () { ".repeat(2000),
        "{{{{{{{{{{".repeat(5000),
        "\"unterminated".to_string(),
        "pm.test(\"a\", function () { /* open".to_string(),
        "/".repeat(10_000),
    ] {
        let (a, _) = import_tests(&[&src]);
        assert!(a.is_empty());
    }
}

#[test]
fn pm_graphql_body_imports() {
    let body_obj = json!({"mode": "graphql", "graphql": {"query": "{ a }", "variables": {"x": 1}}});
    let body_str =
        json!({"mode": "graphql", "graphql": {"query": "{ b }", "variables": "{\"y\":2}"}});
    let mk = |b: Value| {
        let item =
            json!({"name": "r", "request": {"method": "POST", "url": "http://h/x", "body": b}});
        postman::parse(&collection(json!([item]), json!({}))).unwrap()
    };
    let r = mk(body_obj);
    assert!(r.warnings.is_empty());
    assert_eq!(
        first(&r).body,
        Body::GraphQl(GraphQlBody {
            query: "{ a }".into(),
            variables: "{\"x\":1}".into()
        })
    );
    let r = mk(body_str);
    assert_eq!(
        first(&r).body,
        Body::GraphQl(GraphQlBody {
            query: "{ b }".into(),
            variables: "{\"y\":2}".into()
        })
    );
}

#[test]
fn pm_folders_count_toward_node_cap() {
    let n = wisp_storage::import::MAX_REQUESTS + 1;
    let folders: Vec<Value> = (0..n)
        .map(|i| json!({"name": format!("f{i}"), "item": []}))
        .collect();
    let err = postman::parse(&collection(Value::Array(folders), json!({}))).unwrap_err();
    assert!(matches!(err, ImportError::TooLarge(_)), "{err:?}");
}

#[test]
fn pm_generated_secret_names_avoid_collection_variable_keys() {
    let item = json!({"name": "r", "request": {"method": "GET", "url": "http://h/x",
        "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "s3cret"}]}}});
    let extra = json!({"variable": [{"key": "r_token", "value": "plain"}]});
    let r = postman::parse(&collection(json!([item]), extra)).unwrap();
    let keys: Vec<&str> = r.collection_vars.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(keys, ["r_token"]);
    let secrets = &r.environments[0].secrets;
    assert!(secrets.iter().all(|s| s.name != "r_token"), "{secrets:?}");
    assert_eq!(secrets.len(), 2);
}

#[test]
fn pm_userinfo_becomes_basic_only_without_auth_object() {
    let mk = |auth: Option<Value>| {
        let mut req = json!({"method": "GET", "url": "http://u:p@h/x"});
        if let Some(a) = auth {
            req["auth"] = a;
        }
        let item = json!({"name": "r", "request": req});
        postman::parse(&collection(json!([item]), json!({}))).unwrap()
    };
    assert!(matches!(first(&mk(None)).auth, Auth::Basic { .. }));
    let r = mk(Some(json!({"type": "ntlm", "ntlm": []})));
    assert_eq!(first(&r).auth, Auth::Inherit);
    assert!(!r.environments.iter().any(|e| !e.secrets.is_empty()));
}

#[test]
fn pm_unsupported_method_warning_has_no_raw_text() {
    let item =
        json!({"name": "r", "request": {"method": "TOKEN-sk_live_abc123", "url": "http://h/x"}});
    let r = postman::parse(&collection(json!([item]), json!({}))).unwrap();
    assert!(r.warnings.iter().any(|w| w.message.contains("method")));
    assert!(r.warnings.iter().all(|w| !w.message.contains("sk_live")));
}

#[test]
fn pm_failing_units_parse_in_linear_time() {
    let unit = "pm.test('a', function () { ));";
    let src = unit.repeat(60 * 1024 / unit.len());
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let (a, r) = import_tests(&[&src]);
        assert!(a.is_empty());
        assert!(r.warnings[0].message.contains("tests not translated"));
    }
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

#[test]
fn pm_non_path_json_expressions_not_translated() {
    for stmt in [
        "pm.expect(pm.response.json().items.length).to.eql(3);",
        "pm.expect(pm.response.json().total-1).to.eql(9);",
    ] {
        let (a, r) = import_tests(&[&format!("pm.test(\"x\", function () {{ {stmt} }});")]);
        assert!(a.is_empty(), "{stmt}");
        assert_eq!(r.warnings.len(), 1);
    }
}

#[test]
fn pm_failing_units_with_balanced_body_parse_in_linear_time() {
    let n = 1920;
    let src = format!(
        "{}{}",
        "pm.test('a', function () { ));".repeat(n),
        "})".repeat(n)
    );
    let start = std::time::Instant::now();
    for _ in 0..20 {
        let (a, r) = import_tests(&[&src]);
        assert!(a.is_empty());
        assert!(r.warnings[0].message.contains("tests not translated"));
    }
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

#[test]
fn pm_pathological_single_line_scripts_parse_in_linear_time() {
    let n = 21_300;
    let wrapped = format!("pm.test('a', function () {{ {} }});", "(/[".repeat(n));
    let shapes = [
        wrapped,
        "(/[".repeat(n),
        "=/[".repeat(n),
        "a/b(".repeat(n),
        "if (s) /[".repeat(n / 3),
    ];
    for src in &shapes {
        let start = std::time::Instant::now();
        let (a, _) = import_tests(&[src]);
        assert!(a.is_empty());
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }
}

const STATUS_TEST: &str = "pm.test('ok', function () { pm.response.to.have.status(200); });";

fn assert_untranslated(lines: &[&str], n: usize) {
    let (a, r) = import_tests(lines);
    assert!(a.is_empty(), "{lines:?}: {a:?}");
    assert_eq!(r.warnings.len(), 1, "{lines:?}");
    assert_eq!(
        r.warnings[0].message,
        format!("{n} of {n} tests not translated; scripts are not run"),
        "{lines:?}"
    );
}

#[test]
fn pm_strict_comments_and_separators_translate() {
    let (a, r) = import_tests(&[
        "// leading",
        "/* block */ pm.test('a', /* c */ function () { // c",
        "  pm.response.to.have.status(200) // no semicolon",
        "  /* x */ pm.response.to.have.header('X'); /* y */",
        "}) // no semicolon",
        "pm.test('b', () => { pm.response.to.have.status(201) /* c */ });",
        "pm.test('c', () => {",
        "  pm.expect(pm.response.code).to.eql(202);",
        "}); // trailing",
    ]);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(a.len(), 4);
}

#[test]
fn pm_strict_if_nested_test_left_untranslated() {
    assert_untranslated(
        &[
            "if (pm.response.code === 500) {",
            "  pm.test('err', function () { pm.response.to.have.status(500); });",
            "}",
            STATUS_TEST,
        ],
        2,
    );
}

#[test]
fn pm_strict_else_do_slash_left_untranslated() {
    assert_untranslated(
        &[
            "if (a) { x(); } else { if (b) { y(); } else /}x/.test(s);",
            "  pm.test('err', function () { pm.response.to.have.status(500); });",
            "}",
            STATUS_TEST,
        ],
        2,
    );
    assert_untranslated(
        &[
            "if (a) { do /}/.test(s); while (0);",
            "  pm.test('err', function () { pm.response.to.have.status(500); });",
            "}",
            STATUS_TEST,
        ],
        2,
    );
}

#[test]
fn pm_strict_nested_template_left_untranslated() {
    assert_untranslated(
        &[
            "if (a) { var t = `${`}`}`;",
            "  pm.test('err', function () { pm.response.to.have.status(500); });",
            "}",
            STATUS_TEST,
        ],
        2,
    );
    assert_untranslated(
        &["pm.test(`name`, function () { pm.response.to.have.status(200); });"],
        1,
    );
}

#[test]
fn pm_strict_division_left_untranslated() {
    assert_untranslated(
        &["var secs = pm.response.responseTime / 1000;", STATUS_TEST],
        1,
    );
    assert_untranslated(
        &["pm.test('d', function () { pm.expect(pm.response.json().a / 1000).to.eql(1); });"],
        1,
    );
}

#[test]
fn pm_strict_regex_left_untranslated() {
    for line in [
        "const ok = ['a)'].some(x => /\\)$/.test(x));",
        "const ok = ['a'].some(x => /}/.test(x));",
        "if (s) /}/.test(s);",
        "const ok = [1].some(function (x) { return /}/.test(x) });",
        "if (pm.response.code === 500) { /}/.test('x');",
    ] {
        assert_untranslated(&[line, STATUS_TEST], 1);
        assert_untranslated(
            &[
                "if (s) {",
                line,
                "  pm.test('err', function () { pm.response.to.have.status(500); });",
                "}",
                STATUS_TEST,
            ],
            2,
        );
    }
}

#[test]
fn pm_strict_one_bad_test_untranslates_whole_script() {
    assert_untranslated(
        &[
            STATUS_TEST,
            "pm.test('bad', function () { console.log(1); });",
        ],
        2,
    );
}

#[test]
fn pm_strict_unterminated_input_left_untranslated() {
    assert_untranslated(
        &["pm.test('a', function () { pm.response.to.have.header('x); });"],
        1,
    );
    assert_untranslated(&[STATUS_TEST, "/* open"], 1);
    assert_untranslated(
        &["pm.test('a', function () { pm.response.to.have.status(200); }"],
        1,
    );
    assert_untranslated(
        &["pm.test('a', function () { pm.response.to.have.status(200); /* open"],
        1,
    );
}

#[test]
fn pm_strict_scripts_judged_independently() {
    let item = request_with(
        json!({"method": "GET", "url": "http://h/x"}),
        json!([
            {"listen": "test", "script": {"exec": [STATUS_TEST]}},
            {"listen": "test", "script": {"exec": ["var x = 1;", "pm.test('b', () => { pm.response.to.have.status(500); });"]}},
        ]),
    );
    let r = postman::parse(&collection(json!([item]), json!({}))).unwrap();
    assert_eq!(first(&r).assertions.len(), 1);
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(
        r.warnings[0].message,
        "1 of 2 tests not translated; scripts are not run"
    );
}

#[test]
fn pm_strict_grammar_violations_untranslated() {
    for src in [
        "pm.test('a', function (x) { pm.response.to.have.status(200); });",
        "pm.test('a', (x) => { pm.response.to.have.status(200); });",
        "pm.test('a', function () { });",
        "pm.test('a', function () { pm.response.to.have.status(200); pm.test('b', () => {}); });",
        "pm.test('a', function () { pm.response.to.have.status(200); }) pm.test('b', () => {});",
        "pm.test('a', function () { pm.response.to.have.status(200) pm.response.to.have.status(201) });",
        "var x = 1; pm.test('a', () => { pm.response.to.have.status(200); });",
        "return;",
    ] {
        assert_untranslated(&[src], src.matches("pm.test(").count().max(1));
    }
}

#[test]
fn pm_strict_empty_and_comment_only_scripts_are_clean() {
    let (a, r) = import_tests(&["// nothing here", "/* or here */"]);
    assert!(a.is_empty());
    assert!(r.warnings.is_empty());
}

#[test]
fn pm_strict_unicode_line_separator_ends_comment() {
    for sep in ["\u{2028}", "\u{2029}"] {
        let c = format!("// note{sep}if (pm.response.code === 500)");
        assert_untranslated(
            &[
                &c,
                "pm.test('err', function () { pm.response.to.have.status(500); });",
            ],
            1,
        );
    }
}

#[test]
fn pm_strict_non_string_exec_entry() {
    let events = json!([{"listen": "test", "script": {"exec": [
        ["if (pm.response.code === 500)"],
        "pm.test('err', function () { pm.response.to.have.status(500); });"
    ]}}]);
    let item = request_with(json!({"method": "GET", "url": "http://h/x"}), events);
    let r = postman::parse(&collection(json!([item]), json!({}))).unwrap();
    assert!(first(&r).assertions.is_empty());
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(
        r.warnings[0].message,
        "1 of 1 tests not translated; scripts are not run"
    );
}

#[test]
fn pm_strict_raw_line_break_in_string_or_before_arrow() {
    for lines in [
        vec![
            "pm.test('a",
            "b', function () { pm.response.to.have.status(200); });",
        ],
        vec![
            "pm.test('a', () ",
            "=> { pm.response.to.have.status(200); });",
        ],
        vec!["pm.test('a', () /* x\n */ => { pm.response.to.have.status(200); });"],
        vec!["pm.test('a', function () { pm.expect(pm.response.json().a).to.eql('x\r y'); });"],
    ] {
        assert_untranslated(&lines, lines.join("\n").matches("pm.test(").count().max(1));
    }
}

#[test]
fn pm_strict_leading_zero_integer() {
    for lit in ["010", "00", "-012", "01.5"] {
        let src = format!(
            "pm.test('a', function () {{ pm.expect(pm.response.json().a).to.eql({lit}); }});"
        );
        assert_untranslated(&[&src], 1);
    }
}

#[test]
fn pm_strict_leading_zero_json_index() {
    for idx in ["010", "00"] {
        let src = format!(
            "pm.test('a', function () {{ pm.expect(pm.response.json().items[{idx}]).to.eql(1); }});"
        );
        assert_untranslated(&[&src], 1);
    }
    let (a, _) = import_tests(&[
        "pm.test('a', function () { pm.expect(pm.response.json().items[0]).to.eql(1); });",
    ]);
    assert_eq!(a.len(), 1);
}

#[test]
fn pm_strict_digit_leading_member_left_untranslated() {
    for tail in [".0", ".a.0", ".1b", ".items.0"] {
        let s = format!(
            "pm.test('a', function () {{ pm.expect(pm.response.json(){tail}).to.eql(1); }});"
        );
        assert_untranslated(&[s.as_str()], 1);
    }
    for tail in [".a1", "._x"] {
        let s = format!(
            "pm.test('a', function () {{ pm.expect(pm.response.json(){tail}).to.eql(1); }});"
        );
        let (a, r) = import_tests(&[s.as_str()]);
        assert_eq!(a.len(), 1, "{tail}");
        assert!(r.warnings.is_empty(), "{tail}");
    }
}
