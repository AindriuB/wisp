use super::*;
use crate::model::{Header, PreparedBody};
use crate::request::{
    ApiKeyLocation, Auth, Body, CollectionDefaults, Environment, FolderDefaults, KeyValue, Request,
    TextBody, Var,
};

fn var(n: &str, v: &str) -> Var {
    Var {
        name: n.into(),
        value: v.into(),
    }
}

fn kv(n: &str, v: &str, enabled: bool) -> KeyValue {
    KeyValue {
        name: n.into(),
        value: v.into(),
        enabled,
    }
}

fn hdr(n: &str, v: &str) -> Header {
    Header {
        name: n.into(),
        value: v.into(),
    }
}

fn req(url: &str) -> Request {
    Request {
        url: url.into(),
        ..Request::default()
    }
}

fn run(r: &Request) -> Result<crate::model::PreparedRequest, ResolveError> {
    let c = CollectionDefaults::default();
    resolve(
        r,
        &Scope {
            collection: &c,
            folders: vec![],
            environment: None,
        },
    )
}

#[test]
fn resolves_the_spec_example() {
    let c = CollectionDefaults {
        vars: vec![var("baseUrl", "https://api.test")],
        ..Default::default()
    };
    let mut r = req("{{baseUrl}}/orgs/{orgId}/files/{name}.{ext}");
    r.path = vec![
        var("orgId", "{{orgId}}"),
        var("name", "report"),
        var("ext", "pdf"),
    ];
    r.vars = vec![var("orgId", "acme")];
    r.params = vec![kv("notify", "true", true), kv("off", "x", false)];
    let p = resolve(
        &r,
        &Scope {
            collection: &c,
            folders: vec![],
            environment: None,
        },
    )
    .unwrap();
    assert_eq!(
        p.url,
        "https://api.test/orgs/acme/files/report.pdf?notify=true"
    );
}

fn path_url(url: &str, value: &str) -> String {
    let mut r = req(url);
    r.path = vec![var("id", value), var("path", value)];
    run(&r).unwrap().url
}

#[test]
fn path_params_are_percent_encoded() {
    assert_eq!(path_url("http://h/{id}", "a/b"), "http://h/a%2Fb");
    assert_eq!(path_url("http://h/{*path}", "a/b"), "http://h/a/b");
    assert_eq!(path_url("http://h/{id}", "a b"), "http://h/a%20b");
}

#[test]
fn path_patterns_warn_on_mismatch_and_invalid_regex() {
    let mut r = req("http://h/{id:\\d+}");
    r.path = vec![var("id", "abc")];
    let p = run(&r).unwrap();
    assert_eq!(p.url, "http://h/abc");
    assert_eq!(p.warnings.len(), 1);
    assert!(matches!(
        p.warnings[0],
        ResolveWarning::PatternMismatch { .. }
    ));
    r.path = vec![var("id", "42")];
    assert_eq!(run(&r).unwrap().warnings, vec![]);
    r.url = "http://h/{id:(}".into();
    assert!(matches!(
        run(&r).unwrap().warnings[0],
        ResolveWarning::InvalidPattern { .. }
    ));
    r.url = "http://h/{id:\\d{2}}".into();
    assert_eq!(run(&r).unwrap().warnings, vec![]);
}

#[test]
fn path_errors_are_reported_and_other_braces_left_untouched() {
    assert_eq!(
        run(&req("http://h/{nope}")).unwrap_err().problems,
        vec![Problem::MissingPathParam {
            name: "nope".into()
        }]
    );
    assert!(matches!(
        run(&req("http://h/{a")).unwrap_err().problems[0],
        Problem::Syntax { .. }
    ));
    assert!(matches!(
        run(&req("http://h/a}")).unwrap_err().problems[0],
        Problem::Syntax { .. }
    ));
    assert_eq!(
        run(&req("http://h/x?q={param}#{f}")).unwrap().url,
        "http://h/x?q={param}#{f}"
    );
    assert_eq!(run(&req("http://{param}")).unwrap().url, "http://{param}");
}

#[test]
fn variable_precedence_is_applied_one_level_at_a_time() {
    let coll = |v: &[Var]| CollectionDefaults {
        vars: v.to_vec(),
        ..Default::default()
    };
    let fold = |v: &[Var]| FolderDefaults {
        vars: v.to_vec(),
        ..Default::default()
    };
    let levels = ["request", "near", "outer", "secret", "env", "coll"];
    for skip in 0..levels.len() {
        let has = |i: usize| -> Vec<Var> {
            if i >= skip {
                vec![var("x", levels[i])]
            } else {
                vec![]
            }
        };
        let mut r = req("http://h/{{x}}");
        r.vars = has(0);
        let near = fold(&has(1));
        let outer = fold(&has(2));
        let env = Environment {
            name: "e".into(),
            secrets: has(3),
            vars: has(4),
        };
        let c = coll(&has(5));
        let p = resolve(
            &r,
            &Scope {
                collection: &c,
                folders: vec![&outer, &near],
                environment: Some(&env),
            },
        )
        .unwrap();
        assert_eq!(p.url, format!("http://h/{}", levels[skip]));
    }
}

#[test]
fn unresolved_variables_are_reported() {
    let mut r = req("http://h/{{missing}}");
    r.headers = vec![
        kv("X-A", "{{missing}}", true),
        kv("X-B", "{{missing}}", false),
    ];
    assert_eq!(
        run(&r).unwrap_err().problems,
        vec![
            Problem::UnresolvedVar {
                name: "missing".into(),
                location: Location::Url
            },
            Problem::UnresolvedVar {
                name: "missing".into(),
                location: Location::Header("X-A".into())
            },
        ]
    );
    r.headers = vec![kv("X-B", "{{missing}}", false)];
    r.url = "http://h".into();
    assert!(run(&r).is_ok());
    assert!(run(&req("http://h/{{$nope}}")).is_err());
}

#[test]
fn whitespace_inside_braces_is_trimmed() {
    let mut r = req("http://h/{{ a }}");
    r.vars = vec![var("a", "ok")];
    assert_eq!(run(&r).unwrap().url, "http://h/ok");
}

#[test]
fn variable_cycles_are_reported() {
    let mut r = req("http://h/{{a}}");
    r.vars = vec![var("a", "{{b}}"), var("b", "{{a}}")];
    assert!(
        run(&r)
            .unwrap_err()
            .problems
            .iter()
            .any(|p| matches!(p, Problem::VarCycle { .. }))
    );
}

#[test]
fn dynamic_variables_are_generated() {
    let re =
        regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
            .unwrap();
    let mut r = req("http://h/{{$uuid}}/{{$uuid}}");
    let u = run(&r).unwrap().url;
    let parts: Vec<&str> = u.split('/').collect();
    assert!(re.is_match(parts[3]) && re.is_match(parts[4]));
    assert_ne!(parts[3], parts[4]);
    r.url = "http://h/{{$timestamp}}".into();
    let u = run(&r).unwrap().url;
    assert!(u["http://h/".len()..].parse::<u64>().unwrap() > 1_600_000_000);
}

#[test]
fn auth_inherit_uses_the_enclosing_scope() {
    let c = CollectionDefaults {
        auth: Auth::Bearer {
            token: "{{t}}".into(),
        },
        vars: vec![var("t", "tok")],
        ..Default::default()
    };
    let f = FolderDefaults::default();
    let r = req("http://h");
    let p = resolve(
        &r,
        &Scope {
            collection: &c,
            folders: vec![&f],
            environment: None,
        },
    )
    .unwrap();
    assert_eq!(p.headers, vec![hdr("Authorization", "Bearer tok")]);

    let c = CollectionDefaults {
        auth: Auth::Inherit,
        ..Default::default()
    };
    let p = resolve(
        &r,
        &Scope {
            collection: &c,
            folders: vec![&f],
            environment: None,
        },
    )
    .unwrap();
    assert_eq!(p.headers, vec![]);
}

#[test]
fn basic_and_api_key_auth_replace_existing_values() {
    let mut r = req("http://h");
    r.headers = vec![kv("authorization", "old", true)];
    r.auth = Auth::Basic {
        username: "user".into(),
        password: "pass".into(),
    };
    assert_eq!(
        run(&r).unwrap().headers,
        vec![hdr("Authorization", "Basic dXNlcjpwYXNz")]
    );
    r.auth = Auth::ApiKey {
        name: "k".into(),
        value: "v v".into(),
        location: ApiKeyLocation::Query,
    };
    r.params = vec![kv("a", "1", true)];
    let p = run(&r).unwrap();
    assert_eq!(p.url, "http://h?a=1&k=v%20v");
    r.auth = Auth::ApiKey {
        name: "X-Key".into(),
        value: "v".into(),
        location: ApiKeyLocation::Header,
    };
    assert!(run(&r).unwrap().headers.contains(&hdr("X-Key", "v")));
}

#[test]
fn headers_merge_across_scopes() {
    let c = CollectionDefaults {
        headers: vec![kv("X-A", "1", true), kv("X-C", "c", true)],
        ..Default::default()
    };
    let mut r = req("http://h");
    r.headers = vec![kv("x-a", "2", true), kv("X-C", "z", false)];
    let p = resolve(
        &r,
        &Scope {
            collection: &c,
            folders: vec![],
            environment: None,
        },
    )
    .unwrap();
    assert_eq!(p.headers, vec![hdr("x-a", "2"), hdr("X-C", "c")]);
}

#[test]
fn bodies_are_resolved_per_kind() {
    let mut r = req("http://h");
    r.vars = vec![var("n", "v")];
    r.body = Body::Json(TextBody {
        content: "{\"a\":\"{{n}}\"}".into(),
        sidecar: None,
    });
    let p = run(&r).unwrap();
    assert_eq!(p.body, PreparedBody::Bytes(b"{\"a\":\"v\"}".to_vec()));
    assert_eq!(p.headers, vec![hdr("Content-Type", "application/json")]);

    r.headers = vec![kv("content-type", "application/vnd.x+json", true)];
    assert_eq!(
        run(&r).unwrap().headers,
        vec![hdr("content-type", "application/vnd.x+json")]
    );

    r.headers = vec![];
    r.body = Body::Form(vec![kv("a b", "{{n}}&", true), kv("off", "{{zz}}", false)]);
    let p = run(&r).unwrap();
    assert_eq!(p.body, PreparedBody::Bytes(b"a+b=v%26".to_vec()));
    assert_eq!(
        p.headers,
        vec![hdr("Content-Type", "application/x-www-form-urlencoded")]
    );
}

#[test]
fn settings_are_mapped_to_client_settings() {
    let mut r = req("http://h");
    r.settings.verify_tls = false;
    let p = run(&r).unwrap();
    assert_eq!(
        (
            p.client.verify_tls,
            p.client.follow_redirects,
            p.client.proxy
        ),
        (false, true, None)
    );
}

#[test]
fn all_problems_are_collected_not_just_the_first() {
    let mut r = req("http://h/{p}");
    r.headers = vec![kv("X", "{{m}}", true)];
    assert_eq!(run(&r).unwrap_err().problems.len(), 2);
}

#[test]
fn exponential_fan_out_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    let mut vars: Vec<Var> = (0..8)
        .map(|i| var(&format!("v{i}"), &format!("{{{{v{}}}}}", i + 1).repeat(8)))
        .collect();
    vars.push(var("v8", "x"));
    r.vars = vars;
    let start = std::time::Instant::now();
    let err = run(&r).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert!(err.problems.iter().any(|p| matches!(
        p,
        Problem::Syntax { location: Location::Url, message } if message.contains("too large")
    )));
}

#[test]
fn a_value_that_grows_past_the_size_cap_is_reported_at_its_header() {
    let mut r = req("http://h");
    r.vars = vec![
        var("chunk", &"y".repeat(4096)),
        var("big", &"{{chunk}}".repeat(17)),
    ];
    r.headers = vec![kv("X-Big", "{{big}}", true)];
    let err = run(&r).unwrap_err();
    assert!(err.problems.iter().any(|p| matches!(
        p,
        Problem::Syntax { location: Location::Header(n), message }
            if n == "X-Big" && message.contains("too large")
    )));
}

#[test]
fn a_three_level_chain_within_limits_resolves_exactly() {
    let mut r = req("http://h/{{a}}");
    r.vars = vec![var("a", "{{b}}/x"), var("b", "{{c}}"), var("c", "ok")];
    assert_eq!(run(&r).unwrap().url, "http://h/ok/x");
}

#[test]
fn a_catch_all_path_param_encodes_each_segment() {
    assert_eq!(path_url("http://h/{*path}", "a b/c"), "http://h/a%20b/c");
}

#[test]
fn a_pattern_cannot_break_out_of_its_anchoring_group() {
    let mut r = req("http://h/{id:x)|(}");
    r.path = vec![var("id", "abc")];
    assert!(matches!(
        run(&r).unwrap().warnings[0],
        ResolveWarning::InvalidPattern { .. }
    ));
}

#[test]
fn auth_inherit_walks_the_nearest_folder_first() {
    let c = CollectionDefaults::default();
    let outer = FolderDefaults {
        auth: Auth::Bearer { token: "A".into() },
        ..Default::default()
    };
    let near = FolderDefaults {
        auth: Auth::Bearer { token: "B".into() },
        ..Default::default()
    };
    let p = resolve(
        &req("http://h"),
        &Scope {
            collection: &c,
            folders: vec![&outer, &near],
            environment: None,
        },
    )
    .unwrap();
    assert_eq!(p.headers, vec![hdr("Authorization", "Bearer B")]);
}

fn assert_too_large_promptly(r: &Request) {
    let start = std::time::Instant::now();
    let result = run(r);
    let elapsed = start.elapsed();
    eprintln!("elapsed: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(1));
    let err = result.unwrap_err();
    assert!(err.problems.iter().any(|p| matches!(
        p,
        Problem::Syntax { message, .. } if message.contains("too large")
    )));
}

#[test]
fn a_large_literal_body_without_variables_resolves_unchanged() {
    let mut r = req("http://h");
    let content = "x".repeat(70_000);
    r.body = Body::Json(TextBody {
        content: content.clone(),
        sidecar: None,
    });
    let p = run(&r).unwrap();
    assert!(matches!(p.body, PreparedBody::Bytes(ref b) if b == content.as_bytes()));
}

#[test]
fn a_large_literal_body_with_one_variable_substitutes_only_that_variable() {
    let mut r = req("http://h");
    r.vars = vec![var("who", "me")];
    let content = format!("{}{{{{who}}}}{}", "a".repeat(70_000), "b".repeat(10));
    r.body = Body::Json(TextBody {
        content,
        sidecar: None,
    });
    let expected = format!("{}me{}", "a".repeat(70_000), "b".repeat(10));
    let p = run(&r).unwrap();
    assert!(matches!(p.body, PreparedBody::Bytes(ref b) if b == expected.as_bytes()));
}

#[test]
fn fan_out_over_unresolved_leaves_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    r.vars = vec![
        var("v0", &"{{v1}}".repeat(10_000)),
        var("v1", &"{{m}}".repeat(10_000)),
    ];
    assert_too_large_promptly(&r);
}

#[test]
fn fan_out_over_cycle_leaves_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    r.vars = vec![
        var("v0", &"{{v1}}".repeat(10_000)),
        var("v1", &"{{v0}}".repeat(10_000)),
    ];
    assert_too_large_promptly(&r);
}

#[test]
fn fan_out_over_many_distinct_unresolved_leaves_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    let leaves: String = (0..2000).map(|i| format!("{{{{m{i}}}}}")).collect();
    r.vars = vec![var("v0", &"{{v1}}".repeat(100)), var("v1", &leaves)];
    assert_too_large_promptly(&r);
}

#[test]
fn fan_out_over_empty_name_leaves_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    r.vars = vec![
        var("v0", &"{{v1}}".repeat(10_000)),
        var("v1", &"{{}}".repeat(10_000)),
    ];
    assert_too_large_promptly(&r);
}

#[test]
fn fan_out_over_blank_name_leaves_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    r.vars = vec![
        var("v0", &"{{v1}}".repeat(10_000)),
        var("v1", &"{{ }}".repeat(10_000)),
    ];
    assert_too_large_promptly(&r);
}

#[test]
fn fan_out_over_unknown_dynamic_leaves_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    r.vars = vec![
        var("v0", &"{{v1}}".repeat(10_000)),
        var("v1", &"{{$nope}}".repeat(10_000)),
    ];
    assert_too_large_promptly(&r);
}

#[test]
fn fan_out_past_the_depth_limit_is_reported_as_too_large_promptly() {
    let mut r = req("http://h/{{v0}}");
    r.vars = (0..12)
        .map(|i| var(&format!("v{i}"), &format!("{{{{v{}}}}}", i + 1).repeat(10)))
        .collect();
    assert_too_large_promptly(&r);
}
