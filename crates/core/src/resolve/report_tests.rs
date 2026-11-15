use super::tests::{kv, req, var};
use super::*;
use crate::model::PreparedBody;
use crate::request::ClientAuth;
use crate::request::{
    ApiKeyLocation, Auth, Body, CollectionDefaults, Environment, FolderDefaults, Request, TextBody,
    Var,
};

struct Fx {
    req: Request,
    coll: CollectionDefaults,
    folders: Vec<FolderDefaults>,
    env: Option<Environment>,
}

impl Fx {
    fn new(req: Request) -> Self {
        Fx {
            req,
            coll: CollectionDefaults::default(),
            folders: vec![],
            env: None,
        }
    }

    fn scope(&self) -> Scope<'_> {
        Scope {
            collection: &self.coll,
            folders: self.folders.iter().collect(),
            environment: self.env.as_ref(),
        }
    }

    fn report(&self) -> ResolutionReport {
        explain(&self.req, &self.scope())
    }
}

fn text(r: &Shown) -> String {
    r.plain_text().expect("plain text")
}

fn flat(r: &Shown) -> String {
    r.0.iter()
        .map(|p| match p {
            ShownPart::Text(t) => t.clone(),
            other => format!("<{other:?}>"),
        })
        .collect()
}

fn folder_vars(v: &[Var]) -> FolderDefaults {
    FolderDefaults {
        vars: v.to_vec(),
        ..Default::default()
    }
}

// ---- parity ----

/// Fixtures modelled on `resolve/tests.rs`, built from its helpers.
fn fixtures() -> Vec<Fx> {
    let mut out = Vec::new();
    let mut f = Fx::new(req("{{baseUrl}}/orgs/{orgId}/files/{name}.{ext}"));
    f.coll.vars = vec![var("baseUrl", "https://api.test")];
    f.req.path = vec![
        var("orgId", "{{orgId}}"),
        var("name", "report"),
        var("ext", "pdf"),
    ];
    f.req.vars = vec![var("orgId", "acme")];
    f.req.params = vec![kv("notify", "true", true), kv("off", "x", false)];
    out.push(f);

    for (url, v) in [
        ("http://h/{id}", "a/b"),
        ("http://h/{*path}", "a/b"),
        ("http://h/{id}", "a b"),
    ] {
        let mut f = Fx::new(req(url));
        f.req.path = vec![var("id", v), var("path", v)];
        out.push(f);
    }

    let mut f = Fx::new(req("http://h/{{x}}?a=1#frag"));
    f.req.params = vec![kv("p q", "{{y}}&", true)];
    f.req.vars = vec![var("x", "{{y}}-1"), var("y", "v")];
    out.push(f);

    // precedence, one level at a time
    let levels = ["request", "near", "outer", "secret", "env", "coll"];
    for skip in 0..levels.len() {
        let has = |i: usize| -> Vec<Var> {
            if i >= skip {
                vec![var("x", levels[i])]
            } else {
                vec![]
            }
        };
        let mut f = Fx::new(req("http://h/{{x}}"));
        f.req.vars = has(0);
        f.folders = vec![folder_vars(&has(2)), folder_vars(&has(1))];
        f.env = Some(Environment {
            name: "e".into(),
            secrets: has(3),
            vars: has(4),
        });
        f.coll.vars = has(5);
        out.push(f);
    }

    // header layering
    let mut f = Fx::new(req("http://h"));
    f.coll.headers = vec![kv("X-A", "1", true), kv("X-C", "c", true)];
    f.folders = vec![FolderDefaults {
        headers: vec![kv("X-B", "f", true)],
        ..Default::default()
    }];
    f.req.headers = vec![kv("x-a", "2", true), kv("X-C", "z", false)];
    out.push(f);

    // bodies and Content-Type
    let mut f = Fx::new(req("http://h"));
    f.req.vars = vec![var("n", "v")];
    f.req.body = Body::Json(TextBody {
        content: "{\"a\":\"{{n}}\"}".into(),
        sidecar: None,
    });
    out.push(f);
    let mut f = Fx::new(req("http://h"));
    f.req.headers = vec![kv("content-type", "application/vnd.x+json", true)];
    f.req.body = Body::Xml(TextBody {
        content: "<a/>".into(),
        sidecar: None,
    });
    out.push(f);
    let mut f = Fx::new(req("http://h"));
    f.req.vars = vec![var("n", "v")];
    f.req.body = Body::Form(vec![kv("a b", "{{n}}&", true)]);
    out.push(f);

    // auth that adds a non-credential header or query parameter
    for location in [ApiKeyLocation::Header, ApiKeyLocation::Query] {
        let mut f = Fx::new(req("http://h/p?z=1"));
        f.coll.vars = vec![var("t", "acme")];
        f.coll.auth = Auth::ApiKey {
            name: "x-tenant".into(),
            value: "{{t}}".into(),
            location,
        };
        f.req.headers = vec![kv("Accept", "*/*", true)];
        out.push(f);
    }
    // header layering through 2 and 3 folders; reversing the folder order changes the result
    for depth in [2usize, 3] {
        let mut f = Fx::new(req("http://h"));
        f.coll.headers = vec![kv("X-A", "coll", true), kv("X-Only-C", "c", true)];
        f.folders = (0..depth)
            .map(|i| FolderDefaults {
                headers: vec![
                    kv("x-a", &format!("f{i}"), true),
                    kv(&format!("X-F{i}"), "v", true),
                ],
                ..Default::default()
            })
            .collect();
        f.req.headers = vec![kv("X-R", "r", true)];
        out.push(f);
    }
    // auth inheritance through 2 and 3 folders; credential headers and API keys are
    // compared by name and position only
    let bearer = || Auth::Bearer {
        token: "tok".into(),
    };
    let basic = || Auth::Basic {
        username: "u".into(),
        password: "p".into(),
    };
    let apikey = || Auth::ApiKey {
        name: "X-Tenant".into(),
        value: "sk_live_abc".into(),
        location: ApiKeyLocation::Header,
    };
    for auths in [
        vec![bearer(), Auth::Inherit],
        vec![Auth::Inherit, basic()],
        vec![bearer(), Auth::Inherit, apikey()],
        vec![basic(), bearer(), Auth::Inherit],
        vec![Auth::Inherit, Auth::Inherit, apikey()],
    ] {
        let mut f = Fx::new(req("http://h"));
        f.coll.auth = basic();
        f.coll.headers = vec![kv("Accept", "*/*", true)];
        f.folders = auths
            .into_iter()
            .map(|a| FolderDefaults {
                auth: a,
                ..Default::default()
            })
            .collect();
        out.push(f);
    }
    // auth overriding a user Authorization header, and an API key under a plain name
    let mut f = Fx::new(req("http://h"));
    f.req.headers = vec![
        kv("Accept", "*/*", true),
        kv("authorization", "user", true),
        kv("X-Z", "z", true),
    ];
    f.req.auth = bearer();
    out.push(f);
    let mut f = Fx::new(req("http://h"));
    f.req.headers = vec![kv("x-tenant", "mine", true), kv("X-Z", "z", true)];
    f.req.auth = apikey();
    out.push(f);
    // auth that adds nothing
    let mut f = Fx::new(req("http://h"));
    f.coll.auth = Auth::Digest {
        username: "u".into(),
        password: "p".into(),
    };
    out.push(f);
    out
}

/// True when a fixture is meant for the parity comparison: nothing in it is secret,
/// dynamic, credential-named or over 200 characters, and `resolve()` succeeds.
fn eligible(f: &Fx) -> bool {
    let mut strings: Vec<&str> = vec![&f.req.url];
    let mut names: Vec<&str> = vec![];
    let mut vars: Vec<&Var> = f.req.vars.iter().chain(&f.req.path).collect();
    vars.extend(&f.coll.vars);
    for fo in &f.folders {
        vars.extend(&fo.vars);
    }
    if let Some(e) = &f.env {
        if !e.secrets.is_empty() {
            // The secret layer is a fixture for precedence, not for text parity.
            return false;
        }
        vars.extend(&e.vars);
    }
    for v in vars {
        names.push(&v.name);
        strings.push(&v.value);
    }
    let mut kvs: Vec<&crate::request::KeyValue> =
        f.req.headers.iter().chain(&f.req.params).collect();
    kvs.extend(&f.coll.headers);
    for fo in &f.folders {
        kvs.extend(&fo.headers);
    }
    for k in kvs {
        names.push(&k.name);
        strings.push(&k.value);
    }
    if let Body::Json(t) | Body::Xml(t) | Body::Text(t) = &f.req.body {
        strings.push(&t.content);
    }
    !names.iter().any(|n| crate::redact::is_credential_name(n))
        && !strings
            .iter()
            .any(|s| s.contains("{{$") || s.chars().count() > 200)
        && resolve(&f.req, &f.scope()).is_ok()
        // Basic, Bearer and API keys hide their values; `explain_matches_resolve_by_position`
        // covers them by name and position.
        && !all_auths(f).any(|a| matches!(a, Auth::Basic { .. } | Auth::Bearer { .. } | Auth::ApiKey { .. }))
}

fn all_auths(f: &Fx) -> impl Iterator<Item = &Auth> {
    std::iter::once(&f.req.auth)
        .chain(f.folders.iter().map(|fo| &fo.auth))
        .chain(std::iter::once(&f.coll.auth))
}

/// True when a fixture has no secret or dynamic value and `resolve()` succeeds.
fn comparable(f: &Fx) -> bool {
    f.env.as_ref().is_none_or(|e| e.secrets.is_empty())
        && resolve(&f.req, &f.scope()).is_ok()
        && !f.req.url.contains("{{$")
}

fn all_hidden(r: &Shown) -> bool {
    !r.0.is_empty() && r.0.iter().all(|p| matches!(p, ShownPart::Hidden(_)))
}

/// Header names and positions, and body size, equal `resolve()`'s even where values are
/// hidden; every value that is shown equals `resolve()`'s.
fn assert_positions(i: usize, f: &Fx) {
    let p = resolve(&f.req, &f.scope()).unwrap();
    let r = f.report();
    assert_eq!(
        r.headers.len(),
        p.headers.len(),
        "fixture {i}: header count"
    );
    for (n, (rh, ph)) in r.headers.iter().zip(&p.headers).enumerate() {
        assert_eq!(flat(&rh.name), ph.name, "fixture {i}: header {n} name");
        match rh.value.plain_text() {
            Some(v) => assert_eq!(v, ph.value, "fixture {i}: header {n} value"),
            None => assert!(all_hidden(&rh.value), "fixture {i}: header {n} hidden"),
        }
    }
    match &p.body {
        PreparedBody::Bytes(b) => assert_eq!(r.body.bytes, Some(b.len()), "fixture {i}: body"),
        PreparedBody::None => assert!(matches!(r.body.bytes, None | Some(0)), "fixture {i}"),
        _ => {}
    }
    if let Some(u) = r.url.plain_text() {
        assert_eq!(u, p.url, "fixture {i}: url");
    }
}

#[test]
fn explain_matches_resolve_for_plain_fixtures() {
    let all = fixtures();
    let found = all.iter().filter(|f| eligible(f)).count();
    let mut compared = 0;
    for (i, f) in all.iter().enumerate() {
        if !eligible(f) {
            continue;
        }
        let p = resolve(&f.req, &f.scope()).unwrap();
        let r = f.report();
        assert!(r.problems.is_empty(), "fixture {i}: {:?}", r.problems);
        assert_eq!(flat(&r.url), p.url, "fixture {i}: url");
        let got: Vec<(String, String)> = r
            .headers
            .iter()
            .map(|h| (flat(&h.name), flat(&h.value)))
            .collect();
        let want: Vec<(String, String)> = p
            .headers
            .iter()
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect();
        assert_eq!(got, want, "fixture {i}: headers");
        let enabled: Vec<_> = f.req.params.iter().filter(|p| p.enabled).collect();
        let query: Vec<_> = r
            .params
            .iter()
            .filter(|p| p.kind == ParamKind::Query)
            .collect();
        assert_eq!(query.len(), enabled.len(), "fixture {i}: params");
        if let PreparedBody::Bytes(b) = &p.body {
            assert_eq!(r.body.bytes, Some(b.len()), "fixture {i}: body");
        }
        compared += 1;
    }
    assert!(found > 0);
    assert!(compared >= found, "compared {compared} of {found}");
    assert!(found >= 10, "only {found} eligible fixtures");
}

#[test]
fn explain_matches_resolve_by_position() {
    let all = fixtures();
    let mut n = 0;
    for (i, f) in all.iter().enumerate() {
        if comparable(f) {
            assert_positions(i, f);
            n += 1;
        }
    }
    assert!(n >= 20, "only {n} comparable fixtures");
}

#[test]
fn auth_inheritance_names_the_right_folder() {
    let mut f = Fx::new(req("http://h"));
    f.folders = vec![
        FolderDefaults {
            auth: Auth::Basic {
                username: "u".into(),
                password: "p".into(),
            },
            ..Default::default()
        },
        FolderDefaults::default(),
        FolderDefaults {
            auth: Auth::Bearer { token: "t".into() },
            ..Default::default()
        },
    ];
    let r = f.report();
    assert_eq!(
        (r.auth.kind, r.auth.origin),
        (AuthKind::Bearer, Some(Origin::Folder(2)))
    );
    f.folders[2].auth = Auth::Inherit;
    let r = f.report();
    assert_eq!(
        (r.auth.kind, r.auth.origin),
        (AuthKind::Basic, Some(Origin::Folder(0)))
    );
}

#[test]
fn bearer_overrides_a_user_authorization_header_in_place() {
    let mut f = Fx::new(req("http://h"));
    f.req.headers = vec![kv("Authorization", "user", true), kv("X-Z", "z", true)];
    f.req.auth = Auth::Bearer { token: "t".into() };
    let r = f.report();
    assert_eq!(r.headers.len(), 2);
    assert_eq!(text(&r.headers[0].name), "Authorization");
    assert_eq!(r.headers[0].origin, Origin::Auth);
    assert_eq!(r.headers[0].overridden, vec![Origin::Request]);
    assert!(all_hidden(&r.headers[0].value));
}

// ---- leaks ----

const SENTINEL: &str = "sentinel-7f3a9";

fn leaks(f: &Fx) -> bool {
    let r = f.report();
    let a = explain_auth(&f.req, &f.scope());
    format!("{r:?}{a:?}").contains(SENTINEL)
}

#[test]
fn api_key_value_is_hidden_whatever_its_name() {
    for location in [ApiKeyLocation::Header, ApiKeyLocation::Query] {
        let mut f = Fx::new(req("http://h"));
        f.req.auth = Auth::ApiKey {
            name: "X-Tenant".into(),
            value: SENTINEL.into(),
            location,
        };
        assert!(!leaks(&f), "{location:?}");
        let r = f.report();
        assert!(all_hidden(&r.auth.fields[1].value));
    }
}

#[test]
fn a_name_from_a_credential_variable_or_secret_hides_the_value() {
    let mut f = Fx::new(req("http://h"));
    f.req.vars = vec![var("api_token", "X-Tenant")];
    f.env = Some(Environment {
        name: "e".into(),
        secrets: vec![var("sec", "X-Sec")],
        vars: vec![],
    });
    f.req.headers = vec![
        kv("{{api_token}}", SENTINEL, true),
        kv("{{sec}}", SENTINEL, true),
    ];
    f.req.params = vec![
        kv("{{api_token}}", SENTINEL, true),
        kv("{{sec}}", SENTINEL, true),
    ];
    f.req.auth = Auth::ApiKey {
        name: "{{api_token}}".into(),
        value: SENTINEL.into(),
        location: ApiKeyLocation::Header,
    };
    assert!(!leaks(&f));
}

#[test]
fn headers_named_by_different_hidden_variables_stay_apart() {
    let mut f = Fx::new(req("http://h"));
    f.req.vars = vec![var("a_token", "X-One"), var("b_token", "X-Two")];
    f.req.headers = vec![kv("{{a_token}}", "1", true), kv("{{b_token}}", "2", true)];
    assert_eq!(f.report().headers.len(), 2);
    // The same hidden variable twice is the same header.
    f.req.headers = vec![kv("{{a_token}}", "1", true), kv("{{a_token}}", "2", true)];
    assert_eq!(f.report().headers.len(), 1);
}

#[test]
fn credential_query_pairs_inline_in_urls_are_hidden() {
    let mut f = Fx::new(req(&format!(
        "http://h/p?a=1&access_token={SENTINEL}&b=2#f"
    )));
    assert!(!leaks(&f));
    let r = f.report();
    assert_eq!(
        flat(&r.url),
        "http://h/p?a=1&access_token=<Hidden(Credential)>&b=2#f"
    );
    f.req.url = "http://h".into();
    f.req.auth = Auth::OAuth2ClientCredentials {
        token_url: format!("http://t/x?client_secret={SENTINEL}&k=1"),
        client_id: "id".into(),
        client_secret: "s".into(),
        scope: "sc".into(),
        client_auth: ClientAuth::default(),
    };
    assert!(!leaks(&f));
    f.req.auth = Auth::OAuth2AuthCode {
        auth_url: format!("http://t/a?access_token={SENTINEL}"),
        token_url: format!("http://t/x?api_key={SENTINEL}"),
        client_id: "id".into(),
        client_secret: "s".into(),
        scope: "sc".into(),
        client_auth: ClientAuth::default(),
        redirect_port: 0,
    };
    assert!(!leaks(&f));
    let a = explain_auth(&f.req, &f.scope());
    assert_eq!(
        flat(&a.fields[0].value),
        "http://t/a?access_token=<Hidden(Credential)>"
    );
}

#[test]
fn basic_and_digest_usernames_are_hidden() {
    for auth in [
        Auth::Basic {
            username: SENTINEL.into(),
            password: "p".into(),
        },
        Auth::Digest {
            username: SENTINEL.into(),
            password: "p".into(),
        },
    ] {
        let mut f = Fx::new(req("http://h"));
        f.req.auth = auth;
        assert!(!leaks(&f));
        assert_eq!(
            explain_auth(&f.req, &f.scope()).fields[0].value,
            Shown(vec![ShownPart::Hidden(HideReason::Userinfo)])
        );
    }
}

// ---- precedence ----

#[test]
fn variable_precedence_lists_the_losers_as_shadowed() {
    let mut f = Fx::new(req("http://h/{{x}}"));
    f.req.vars = vec![var("x", "r")];
    f.folders = vec![
        folder_vars(&[var("x", "outer")]),
        folder_vars(&[var("x", "near")]),
    ];
    f.env = Some(Environment {
        name: "e".into(),
        secrets: vec![var("x", "s")],
        vars: vec![var("x", "ev")],
    });
    f.coll.vars = vec![var("x", "c")];
    let r = f.report();
    assert_eq!(r.variables.len(), 1);
    let v = &r.variables[0];
    assert_eq!(v.origin, Origin::Request);
    assert_eq!(text(&v.value), "r");
    assert_eq!(
        v.shadowed,
        vec![
            Origin::Folder(1),
            Origin::Folder(0),
            Origin::Secret,
            Origin::Environment,
            Origin::Collection
        ]
    );
    assert_eq!(v.locations, vec![Location::Url]);

    f.req.vars.clear();
    assert_eq!(f.report().variables[0].origin, Origin::Folder(1));
    f.folders.clear();
    let r = f.report();
    assert_eq!(r.variables[0].origin, Origin::Secret);
    assert_eq!(
        r.variables[0].value.0,
        vec![ShownPart::Secret {
            name: "x".into(),
            resolved: true
        }]
    );
    f.env.as_mut().unwrap().secrets.clear();
    assert_eq!(f.report().variables[0].origin, Origin::Environment);
    f.env = None;
    let r = f.report();
    assert_eq!(r.variables[0].origin, Origin::Collection);
    assert_eq!(text(&r.variables[0].value), "c");
}

#[test]
fn headers_layer_and_content_type_is_synthesized() {
    let mut f = Fx::new(req("http://h"));
    f.coll.headers = vec![kv("X-A", "1", true), kv("X-B", "c", true)];
    f.folders = vec![FolderDefaults {
        headers: vec![kv("x-a", "2", true)],
        ..Default::default()
    }];
    f.req.headers = vec![kv("X-A", "3", true)];
    f.req.body = Body::Json(TextBody {
        content: "{}".into(),
        sidecar: None,
    });
    let r = f.report();
    let a = &r.headers[0];
    assert_eq!(text(&a.value), "3");
    assert_eq!(a.origin, Origin::Request);
    assert_eq!(a.overridden, vec![Origin::Collection, Origin::Folder(0)]);
    assert_eq!(r.headers[1].origin, Origin::Collection);
    let ct = &r.headers[2];
    assert_eq!(text(&ct.name), "Content-Type");
    assert_eq!(text(&ct.value), "application/json");
    assert_eq!(ct.origin, Origin::BodyType);
    assert_eq!(ct.synthetic, Some(Synthetic::BodyType(BodyKind::Json)));
    assert_eq!(r.body.kind, BodyKind::Json);
    assert_eq!(r.body.bytes, Some(2));
}

#[test]
fn inherited_auth_names_the_folder_index() {
    let mut f = Fx::new(req("http://h"));
    f.folders = vec![
        FolderDefaults {
            auth: Auth::Bearer { token: "A".into() },
            ..Default::default()
        },
        FolderDefaults::default(),
    ];
    let r = f.report();
    assert_eq!(r.auth.kind, AuthKind::Bearer);
    assert_eq!(r.auth.origin, Some(Origin::Folder(0)));
    assert!(matches!(r.auth.application, AuthApplication::Header(_)));
    let h = &r.headers[0];
    assert_eq!(h.origin, Origin::Auth);
    assert_eq!(h.synthetic, Some(Synthetic::Auth(AuthKind::Bearer)));
    assert_eq!(h.value.0, vec![ShownPart::Hidden(HideReason::Credential)]);

    f.folders[1].auth = Auth::Digest {
        username: "u".into(),
        password: "p".into(),
    };
    let a = explain_auth(&f.req, &f.scope());
    assert_eq!(a.origin, Some(Origin::Folder(1)));
    assert_eq!(a.application, AuthApplication::AtSend);

    f.folders.clear();
    f.coll.auth = Auth::Inherit;
    assert_eq!(f.report().auth.origin, None);
}

#[test]
fn unresolved_cycle_and_too_deep_are_problems() {
    let mut f = Fx::new(req("http://h/{{missing}}/{{a}}/{{d0}}"));
    f.req.vars = vec![var("a", "{{b}}"), var("b", "{{a}}")];
    for i in 0..10 {
        f.req
            .vars
            .push(var(&format!("d{i}"), &format!("{{{{d{}}}}}", i + 1)));
    }
    let r = f.report();
    assert!(r.problems.iter().any(|p| matches!(
        p,
        Problem::UnresolvedVar { name, location: Location::Url } if name == "missing"
    )));
    assert!(
        r.problems
            .iter()
            .any(|p| matches!(p, Problem::VarCycle { .. }))
    );
    assert!(r.problems.iter().any(|p| matches!(
        p,
        Problem::Syntax { message, .. } if message.contains("nested deeper")
    )));
}

#[test]
fn settings_body_env_and_path_params() {
    let mut f = Fx::new(req("http://h/{id}"));
    f.req.path = vec![var("id", "{{n}}")];
    f.req.vars = vec![var("n", "7")];
    f.req.settings.verify_tls = false;
    f.req.settings.timeout = Some(std::time::Duration::from_secs(5));
    f.req.body = Body::Text(TextBody {
        content: "x{{n}}".into(),
        sidecar: None,
    });
    f.env = Some(Environment {
        name: "prod".into(),
        vars: vec![],
        secrets: vec![var("k", "v"), var("e", "")],
    });
    let r = f.report();
    assert_eq!(r.settings.verify_tls.source, SettingSource::Request);
    assert_eq!(r.settings.follow_redirects.source, SettingSource::Default);
    assert_eq!(r.settings.timeout.value, "5s");
    assert_eq!(r.body.bytes, Some(2));
    assert_eq!(r.body.vars, vec!["n".to_string()]);
    let env = r.environment.unwrap();
    assert_eq!(env.name, "prod");
    assert_eq!(
        env.secrets.iter().map(|s| s.present).collect::<Vec<_>>(),
        vec![true, false]
    );
    assert_eq!(r.params.len(), 1);
    assert_eq!(r.params[0].kind, ParamKind::Path);
    assert_eq!(text(&r.url), "http://h/7");
}

// ---- display rules ----

#[test]
fn credential_names_userinfo_and_long_values_are_hidden() {
    let mut f = Fx::new(req("https://user:pw@h/p"));
    f.req.headers = vec![kv("X-Api-Key", "plain-literal", true)];
    f.req.params = vec![kv("token", "abc", true), kv("long", &"a".repeat(201), true)];
    f.req.vars = vec![var("password", "hunter2"), var("ok", "fine")];
    f.req.url = "https://user:pw@h/{{password}}/{{ok}}".into();
    let r = f.report();
    assert_eq!(r.url.0[0], ShownPart::Hidden(HideReason::Userinfo));
    let s = format!("{r:?}");
    for leak in ["pw@", "plain-literal", "abc", "hunter2"] {
        assert!(!s.contains(leak), "{leak} leaked: {s}");
    }
    assert_eq!(
        r.headers[0].value.0,
        vec![ShownPart::Hidden(HideReason::Credential)]
    );
    assert_eq!(
        r.params[0].value.0,
        vec![ShownPart::Hidden(HideReason::Credential)]
    );
    assert_eq!(
        r.params[1].value.0,
        vec![ShownPart::Hidden(HideReason::TooLong)]
    );
    let pw = r.variables.iter().find(|v| v.name == "password").unwrap();
    assert_eq!(pw.value.0, vec![ShownPart::Hidden(HideReason::Credential)]);
    let ok = r.variables.iter().find(|v| v.name == "ok").unwrap();
    assert_eq!(text(&ok.value), "fine");
}

#[test]
fn dynamic_variables_are_marked_and_never_generated() {
    let mut f = Fx::new(req("http://h/{{$uuid}}?t={{$timestamp}}"));
    f.req.headers = vec![kv("X-Request", "{{$uuid}}", true)];
    let r = f.report();
    assert!(r.variables.iter().all(|v| v.origin == Origin::Dynamic));
    assert!(r.url.0.contains(&ShownPart::Dynamic {
        name: "$uuid".into()
    }));
    let s = format!("{r:?}");
    let b = s.as_bytes();
    let uuid_like = (0..b.len().saturating_sub(36)).any(|i| {
        let w = &b[i..i + 36];
        w.iter().enumerate().all(|(j, c)| match j {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
    });
    assert!(!uuid_like, "{s}");
}

#[test]
fn a_secret_value_never_reaches_the_report() {
    const MARK: &str = "CANARY-7f3a9c";
    let mut f = Fx::new(req("http://h/p?q={{nested}}&r={{sec}}"));
    f.env = Some(Environment {
        name: "e".into(),
        vars: vec![],
        secrets: vec![var("sec", MARK)],
    });
    f.req.vars = vec![var("nested", "pre-{{sec}}-post")];
    f.req.headers = vec![
        kv("X-Direct", "{{sec}}", true),
        kv("X-Nested", "{{nested}}", true),
    ];
    f.req.params = vec![kv("p", "{{sec}}", true)];
    f.req.auth = Auth::ApiKey {
        name: "x-tenant".into(),
        value: "{{sec}}".into(),
        location: ApiKeyLocation::Query,
    };
    f.req.body = Body::Json(TextBody {
        content: "{{sec}}".into(),
        sidecar: None,
    });
    let r = f.report();
    for s in [format!("{r:?}"), format!("{r:#?}")] {
        assert!(!s.contains(MARK), "{s}");
    }
    assert!(r.url.0.contains(&ShownPart::Secret {
        name: "sec".into(),
        resolved: true
    }));
    assert_eq!(r.body.bytes, None);
    // Each auth variant too.
    for auth in [
        Auth::Basic {
            username: "u".into(),
            password: "{{sec}}".into(),
        },
        Auth::Bearer {
            token: "{{sec}}".into(),
        },
        Auth::AwsSigV4 {
            access_key: "{{sec}}".into(),
            secret_key: "{{sec}}".into(),
            session_token: "{{sec}}".into(),
            region: "{{sec}}".into(),
            service: "{{sec}}".into(),
        },
    ] {
        f.req.auth = auth;
        let a = explain_auth(&f.req, &f.scope());
        assert!(!format!("{a:?}{:?}", f.report()).contains(MARK));
    }
}

#[test]
fn explain_auth_expands_only_auth_fields() {
    let mut f = Fx::new(req("http://h/{{only-in-url}}"));
    f.req.auth = Auth::Bearer {
        token: "{{t}}".into(),
    };
    let a = explain_auth(&f.req, &f.scope());
    assert_eq!(a.kind, AuthKind::Bearer);
    assert_eq!(a.origin, Some(Origin::Request));
    assert_eq!(
        a.application,
        AuthApplication::Header(Shown(vec![ShownPart::Text("Authorization".into())]))
    );
    assert_eq!(a.fields.len(), 1);
    assert_eq!(a.fields[0].name, "token");
    // `t` is unresolved, so the field is empty rather than hidden.
    assert!(a.fields[0].value.0.is_empty());
    // `explain_auth` returns no problems at all; the URL's unresolved variable is not
    // even looked at. The full report does see it.
    assert!(f.report().problems.iter().any(|p| matches!(
        p,
        Problem::UnresolvedVar { name, .. } if name == "only-in-url"
    )));
}

// ---- variable hiding by use (AC13) ----

const PLAIN: &str = "plain-sentinel-value";

fn plain_var_leaks(mut f: Fx) -> bool {
    f.coll.vars = vec![var("s", PLAIN)];
    format!("{:?}", f.report()).contains(PLAIN)
}

fn with_auth(auth: Auth) -> Fx {
    let mut f = Fx::new(req("http://h"));
    f.req.auth = auth;
    f
}

#[test]
fn a_bearer_token_variable_is_hidden() {
    assert!(!plain_var_leaks(with_auth(Auth::Bearer {
        token: "{{s}}".into()
    })));
}

#[test]
fn a_basic_password_variable_is_hidden() {
    assert!(!plain_var_leaks(with_auth(Auth::Basic {
        username: "u".into(),
        password: "{{s}}".into(),
    })));
}

#[test]
fn an_api_key_value_variable_is_hidden() {
    assert!(!plain_var_leaks(with_auth(Auth::ApiKey {
        name: "X-K".into(),
        value: "{{s}}".into(),
        location: ApiKeyLocation::Query,
    })));
}

#[test]
fn a_sigv4_secret_key_variable_is_hidden() {
    assert!(!plain_var_leaks(with_auth(Auth::AwsSigV4 {
        access_key: "a".into(),
        secret_key: "{{s}}".into(),
        session_token: String::new(),
        region: "r".into(),
        service: "svc".into(),
    })));
}

#[test]
fn an_authorization_header_variable_is_hidden() {
    let mut f = Fx::new(req("http://h"));
    f.req.headers = vec![kv("Authorization", "Bearer {{s}}", true)];
    assert!(!plain_var_leaks(f));
}

#[test]
fn a_credential_param_variable_is_hidden() {
    let mut f = Fx::new(req("http://h"));
    f.req.params = vec![kv("api_key", "{{s}}", true)];
    assert!(!plain_var_leaks(f));
}

#[test]
fn a_credential_query_pair_in_the_url_hides_its_variable() {
    assert!(!plain_var_leaks(Fx::new(req(
        "http://h/?access_token={{s}}"
    ))));
}

#[test]
fn a_variable_spliced_into_a_credential_query_value_is_hidden() {
    // `q` carries `&token=<s>` into `?a={{q}}`.
    let mut f = Fx::new(req("http://h/?a={{q}}"));
    f.req.vars = vec![var("q", "1&token={{s}}")];
    assert!(!plain_var_leaks(f));
}

#[test]
fn a_variable_used_only_in_a_shown_field_stays_shown() {
    let mut f = with_auth(Auth::AwsSigV4 {
        access_key: "a".into(),
        secret_key: "k".into(),
        session_token: String::new(),
        region: "{{s}}".into(),
        service: "svc".into(),
    });
    f.coll.vars = vec![var("s", PLAIN)];
    let r = f.report();
    let v = r.variables.iter().find(|v| v.name == "s").unwrap();
    assert_eq!(text(&v.value), PLAIN);
}
