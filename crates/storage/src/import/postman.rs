//! Import a Postman collection v2.1 (JSON).

use serde_json::Value;
use wisp_core::Method;
use wisp_core::request::{
    ApiKeyLocation, Auth, Body, KeyValue, Part, PartValue, Request, TextBody, Var,
};

use super::{
    ImportError, ImportItem, ImportResult, RequestCounter, SecretSink, method_name, parse_json,
    parse_method, parse_query, request_name,
};

struct Ctx {
    result: ImportResult,
    counter: RequestCounter,
    sink: SecretSink,
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn array(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Parse a Postman v2.1 collection.
pub fn parse(input: &[u8]) -> Result<ImportResult, ImportError> {
    let doc = parse_json(input)?;
    let info = doc
        .get("info")
        .and_then(Value::as_object)
        .ok_or_else(|| ImportError::Parse("not a Postman collection: no `info`".into()))?;
    let schema = text(info.get("schema"));
    if !schema.contains("/v2.1") {
        return Err(ImportError::Unsupported(format!(
            "Postman schema `{schema}` is not v2.1"
        )));
    }
    let name = text(info.get("name"));
    let name = if name.is_empty() {
        "Postman import".to_string()
    } else {
        name
    };
    let mut cx = Ctx {
        result: ImportResult {
            name: name.clone(),
            ..ImportResult::default()
        },
        counter: RequestCounter::default(),
        sink: SecretSink::default(),
    };
    for var in array(doc.get("variable")) {
        let key = text(var.get("key"));
        if !key.is_empty() {
            let value = cx
                .sink
                .secret_ref("collection", &key, text(var.get("value")));
            cx.result.collection_vars.push(Var { name: key, value });
        }
    }
    if doc
        .get("auth")
        .is_some_and(|a| text(a.get("type")) != "noauth")
    {
        cx.result
            .warn(&name, "collection-level auth is not imported");
    }
    script_warning(&mut cx, &name, &doc);
    let items = items(&mut cx, array(doc.get("item")), &[])?;
    cx.result.items = items;
    if let Some(env) = cx.sink.into_env() {
        cx.result.environments.push(env);
    }
    Ok(cx.result)
}

fn script_warning(cx: &mut Ctx, at: &str, node: &Value) {
    let kinds: Vec<String> = array(node.get("event"))
        .iter()
        .filter(|e| {
            e.get("script")
                .and_then(|s| s.get("exec"))
                .is_some_and(|x| match x {
                    Value::Array(a) => !a.is_empty(),
                    Value::String(s) => !s.trim().is_empty(),
                    _ => false,
                })
        })
        .map(|e| text(e.get("listen")))
        .collect();
    if !kinds.is_empty() {
        cx.result.warn(
            at,
            format!("scripts ({}) are not imported", kinds.join(", ")),
        );
    }
}

fn items(cx: &mut Ctx, nodes: &[Value], path: &[String]) -> Result<Vec<ImportItem>, ImportError> {
    let mut out = Vec::new();
    for node in nodes {
        let mut name = text(node.get("name"));
        if node.get("item").is_some() {
            if name.is_empty() {
                name = "Folder".into();
            }
            let mut here = path.to_vec();
            here.push(name.clone());
            let at = here.join(" / ");
            script_warning(cx, &at, node);
            if node
                .get("auth")
                .is_some_and(|a| text(a.get("type")) != "noauth")
            {
                cx.result.warn(&at, "folder auth is not imported");
            }
            if !array(node.get("variable")).is_empty() {
                cx.result.warn(&at, "folder variables are not imported");
            }
            let children = items(cx, array(node.get("item")), &here)?;
            out.push(ImportItem::Folder {
                name,
                items: children,
            });
        } else {
            cx.counter.add()?;
            out.push(request_item(cx, node, name, path));
        }
    }
    Ok(out)
}

fn request_item(cx: &mut Ctx, node: &Value, name: String, path: &[String]) -> ImportItem {
    let mut req = Request::default();
    let raw = node.get("request");
    let obj = raw.filter(|r| r.is_object());
    let method_text = text(obj.and_then(|r| r.get("method")));
    let method = if method_text.is_empty() {
        Method::Get
    } else {
        parse_method(&method_text).unwrap_or(Method::Get)
    };
    let (url, params, path_vars) = match raw {
        Some(Value::String(s)) => split_url(s, &[], None),
        Some(r) => url_of(r.get("url")),
        None => (String::new(), Vec::new(), Vec::new()),
    };
    let (url, userinfo) = super::split_userinfo(&url);
    let name = if name.is_empty() {
        request_name(method_name(method), &url)
    } else {
        name
    };
    let mut at_path = path.to_vec();
    at_path.push(name.clone());
    let at = at_path.join(" / ");
    if !method_text.is_empty() && parse_method(&method_text).is_none() {
        cx.result.warn(
            &at,
            format!("method `{method_text}` is not supported; using GET"),
        );
    }
    req.name = name.clone();
    req.method = method;
    req.url = url;
    req.params = params;
    req.path = path_vars;
    if let Some(r) = obj {
        for h in array(r.get("header")) {
            let key = text(h.get("key"));
            if !key.is_empty() {
                let mut value = text(h.get("value"));
                if super::is_credential_header(&key) {
                    value = cx.sink.secret_ref(&name, &key.to_ascii_lowercase(), value);
                }
                req.headers.push(KeyValue {
                    name: key,
                    value,
                    enabled: !h.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                });
            }
        }
        req.body = body_of(cx, r.get("body"), &req.headers, &at);
        req.auth = auth_of(cx, r.get("auth"), &name, &at);
    }
    if let Some((user, pass)) = userinfo {
        if req.auth == Auth::Inherit {
            req.auth = Auth::Basic {
                username: cx.sink.secret_ref(&name, "username", user),
                password: cx.sink.secret_ref(&name, "password", pass),
            };
        } else {
            cx.result.warn(
                &at,
                "the URL has credentials; the request's own auth is used",
            );
        }
    }
    script_warning(cx, &at, node);
    ImportItem::Request { name, request: req }
}

/// URL string or object into (url without query, params, path vars).
fn url_of(url: Option<&Value>) -> (String, Vec<KeyValue>, Vec<Var>) {
    match url {
        Some(Value::String(s)) => split_url(s, &[], None),
        Some(o @ Value::Object(_)) => {
            let mut raw = text(o.get("raw"));
            if raw.is_empty() {
                let host = match o.get("host") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|x| text(Some(x)))
                        .collect::<Vec<_>>()
                        .join("."),
                    other => text(other),
                };
                let proto = text(o.get("protocol"));
                let port = text(o.get("port"));
                let path = match o.get("path") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|x| match x {
                            Value::Object(_) => text(x.get("value")),
                            _ => text(Some(x)),
                        })
                        .collect::<Vec<_>>()
                        .join("/"),
                    other => text(other),
                };
                if !proto.is_empty() {
                    raw.push_str(&proto);
                    raw.push_str("://");
                }
                raw.push_str(&host);
                if !port.is_empty() {
                    raw.push(':');
                    raw.push_str(&port);
                }
                if !path.is_empty() {
                    raw.push('/');
                    raw.push_str(&path);
                }
            }
            let query = o.get("query").and_then(Value::as_array);
            let explicit: Option<Vec<KeyValue>> = query.map(|q| {
                q.iter()
                    .filter(|p| !text(p.get("key")).is_empty())
                    .map(|p| KeyValue {
                        name: super::form_decode(&text(p.get("key"))),
                        value: super::form_decode(&text(p.get("value"))),
                        enabled: !p.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                    })
                    .collect()
            });
            split_url(&raw, array(o.get("variable")), explicit)
        }
        _ => (String::new(), Vec::new(), Vec::new()),
    }
}

fn split_url(
    raw: &str,
    variables: &[Value],
    query: Option<Vec<KeyValue>>,
) -> (String, Vec<KeyValue>, Vec<Var>) {
    let (base, q) = raw.split_once('?').unwrap_or((raw, ""));
    let params = query.unwrap_or_else(|| parse_query(q));
    // Find where the path starts: after the scheme and host.
    let after_scheme = base.find("://").map_or(0, |i| i + 3);
    let host_end = if base[after_scheme..].starts_with("{{") {
        base[after_scheme..]
            .find("}}")
            .map_or(base.len(), |i| after_scheme + i + 2)
    } else {
        after_scheme
    };
    let path_start = base[host_end..]
        .find('/')
        .map_or(base.len(), |i| host_end + i);
    let (head, path) = base.split_at(path_start);
    let mut vars = Vec::new();
    let segs: Vec<String> = path
        .split('/')
        .map(|seg| match seg.strip_prefix(':') {
            Some(n)
                if !n.is_empty()
                    && n.chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-') =>
            {
                let value = variables
                    .iter()
                    .find(|v| text(v.get("key")) == n)
                    .map(|v| text(v.get("value")))
                    .unwrap_or_default();
                if !vars.iter().any(|v: &Var| v.name == n) {
                    vars.push(Var {
                        name: n.to_string(),
                        value,
                    });
                }
                format!("{{{n}}}")
            }
            _ => seg.to_string(),
        })
        .collect();
    (format!("{head}{}", segs.join("/")), params, vars)
}

fn body_of(cx: &mut Ctx, body: Option<&Value>, headers: &[KeyValue], at: &str) -> Body {
    let Some(b) = body.filter(|b| b.is_object()) else {
        return Body::None;
    };
    let kv = |arr: &[Value]| -> Vec<KeyValue> {
        arr.iter()
            .filter(|p| !text(p.get("key")).is_empty())
            .map(|p| KeyValue {
                name: text(p.get("key")),
                value: text(p.get("value")),
                enabled: !p.get("disabled").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect()
    };
    match text(b.get("mode")).as_str() {
        "raw" => {
            let content = text(b.get("raw"));
            let lang = text(
                b.get("options")
                    .and_then(|o| o.get("raw"))
                    .and_then(|r| r.get("language")),
            )
            .to_ascii_lowercase();
            let ct_json = headers.iter().any(|h| {
                h.name.eq_ignore_ascii_case("content-type")
                    && h.value.to_ascii_lowercase().contains("json")
            });
            let tb = TextBody {
                content,
                sidecar: None,
            };
            match lang.as_str() {
                "json" => Body::Json(tb),
                "xml" => Body::Xml(tb),
                "" if ct_json => Body::Json(tb),
                _ => Body::Text(tb),
            }
        }
        "urlencoded" => Body::Form(kv(array(b.get("urlencoded")))),
        "formdata" => {
            let mut parts = Vec::new();
            for p in array(b.get("formdata")) {
                let name = text(p.get("key"));
                if name.is_empty() {
                    continue;
                }
                let enabled = !p.get("disabled").and_then(Value::as_bool).unwrap_or(false);
                if text(p.get("type")) == "file" {
                    cx.result.warn(
                        at,
                        format!("file part `{name}` is imported disabled and empty; pick the file after import"),
                    );
                    parts.push(Part {
                        name,
                        value: PartValue::Text(String::new()),
                        enabled: false,
                    });
                    continue;
                }
                let value = PartValue::Text(text(p.get("value")));
                parts.push(Part {
                    name,
                    value,
                    enabled,
                });
            }
            Body::Multipart(parts)
        }
        "graphql" => {
            cx.result.warn(at, "GraphQL body is not imported");
            Body::None
        }
        "file" => {
            cx.result.warn(at, "file body is not imported");
            Body::None
        }
        "" => Body::None,
        other => {
            cx.result
                .warn(at, format!("body mode `{other}` is not imported"));
            Body::None
        }
    }
}

fn auth_of(cx: &mut Ctx, auth: Option<&Value>, request: &str, at: &str) -> Auth {
    let Some(a) = auth.filter(|a| a.is_object()) else {
        return Auth::Inherit;
    };
    let kind = text(a.get("type"));
    let field = |name: &str| -> String {
        array(a.get(&kind))
            .iter()
            .find(|e| text(e.get("key")) == name)
            .map(|e| text(e.get("value")))
            .unwrap_or_default()
    };
    match kind.as_str() {
        "noauth" => Auth::None,
        "basic" => Auth::Basic {
            username: cx.sink.secret_ref(request, "username", field("username")),
            password: cx.sink.secret_ref(request, "password", field("password")),
        },
        "bearer" => Auth::Bearer {
            token: cx.sink.secret_ref(request, "token", field("token")),
        },
        "apikey" => Auth::ApiKey {
            name: field("key"),
            value: cx.sink.secret_ref(request, "value", field("value")),
            location: if field("in") == "query" {
                ApiKeyLocation::Query
            } else {
                ApiKeyLocation::Header
            },
        },
        other => {
            cx.result.warn(
                at,
                format!("auth type `{other}` is not imported; using inherit"),
            );
            Auth::Inherit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/import/postman.json");

    fn find<'a>(items: &'a [ImportItem], name: &str) -> &'a Request {
        for i in items {
            match i {
                ImportItem::Request { name: n, request } if n == name => return request,
                ImportItem::Folder { items, .. } => {
                    if let Some(r) = try_find(items, name) {
                        return r;
                    }
                }
                _ => {}
            }
        }
        panic!("no request {name}");
    }

    fn try_find<'a>(items: &'a [ImportItem], name: &str) -> Option<&'a Request> {
        for i in items {
            match i {
                ImportItem::Request { name: n, request } if n == name => return Some(request),
                ImportItem::Folder { items, .. } => {
                    if let Some(r) = try_find(items, name) {
                        return Some(r);
                    }
                }
                _ => {}
            }
        }
        None
    }

    #[test]
    fn fixture_tree_and_paths() {
        let r = parse(FIXTURE).unwrap();
        assert_eq!(r.name, "Pet Store");
        let ImportItem::Folder { name, items } = &r.items[0] else {
            panic!()
        };
        assert_eq!(name, "Pets");
        assert!(matches!(&items[0], ImportItem::Folder { name, .. } if name == "Admin"));
        let get = find(&r.items, "Get pet");
        assert_eq!(get.url, "{{baseUrl}}/pets/{id}");
        assert_eq!(
            get.path,
            [Var {
                name: "id".into(),
                value: "7".into()
            }]
        );
        assert_eq!(get.params[0].name, "verbose");
        assert!(!get.params[0].enabled);
    }

    #[test]
    fn fixture_bodies() {
        let r = parse(FIXTURE).unwrap();
        assert!(matches!(find(&r.items, "Create pet").body, Body::Json(_)));
        assert!(matches!(find(&r.items, "Login").body, Body::Form(_)));
        let Body::Multipart(parts) = &find(&r.items, "Upload").body else {
            panic!()
        };
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].value, PartValue::Text(String::new()));
        assert!(!parts[1].enabled);
    }

    #[test]
    fn fixture_secrets_only_in_secrets_env() {
        let r = parse(FIXTURE).unwrap();
        let basic = &find(&r.items, "Login").auth;
        assert_eq!(
            basic,
            &Auth::Basic {
                username: "{{login_username}}".into(),
                password: "{{login_password}}".into()
            }
        );
        let Auth::Bearer { token } = &find(&r.items, "Create pet").auth else {
            panic!()
        };
        assert_eq!(token, "{{create_pet_token}}");
        let env = &r.environments[0];
        assert_eq!(env.name, super::super::SECRETS_ENV_PLACEHOLDER);
        assert!(env.vars.is_empty());
        assert!(env.secrets.iter().any(|v| v.value == "s3cret-pass"));
        let debug = format!("{:?}", r.items);
        assert!(!debug.contains("s3cret-pass") && !debug.contains("tok-123"));
    }

    #[test]
    fn fixture_vars_and_script_warning() {
        let r = parse(FIXTURE).unwrap();
        assert_eq!(r.collection_vars[0].name, "baseUrl");
        let scripts: Vec<_> = r
            .warnings
            .iter()
            .filter(|w| w.message.contains("scripts"))
            .collect();
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].at, "Pets / Create pet");
    }

    #[test]
    fn unsupported_auth_warns() {
        let doc = br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "item":[{"name":"r","request":{"method":"GET","url":"http://h/x","auth":{"type":"digest","digest":[]}}}]}"#;
        let r = parse(doc).unwrap();
        assert_eq!(find(&r.items, "r").auth, Auth::Inherit);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn graphql_warns() {
        let doc = br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "item":[{"name":"r","request":{"method":"POST","url":"http://h/x","body":{"mode":"graphql","graphql":{"query":"{a}"}}}}]}"#;
        let r = parse(doc).unwrap();
        assert_eq!(find(&r.items, "r").body, Body::None);
        assert!(r.warnings[0].message.contains("GraphQL"));
    }

    #[test]
    fn wrong_schema_is_error() {
        let doc = br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.0.0/collection.json"},"item":[]}"#;
        assert!(matches!(parse(doc), Err(ImportError::Unsupported(_))));
        assert!(parse(b"{}").is_err());
        assert!(parse(b"[1").is_err());
        assert!(parse(b"{\"info\":{\"name\":\"x\"}}").is_err());
    }

    #[test]
    fn url_object_without_raw() {
        let doc = br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "item":[{"name":"r","request":{"method":"GET","url":{"protocol":"https","host":["api","x","com"],"path":["a",":b"]}}}]}"#;
        let r = parse(doc).unwrap();
        assert_eq!(find(&r.items, "r").url, "https://api.x.com/a/{b}");
    }

    #[test]
    fn string_request_shorthand() {
        let doc = br#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "item":[{"name":"r","request":"http://h/x?a=1"}]}"#;
        let r = parse(doc).unwrap();
        let req = find(&r.items, "r");
        assert_eq!(req.url, "http://h/x");
        assert_eq!(req.params.len(), 1);
    }

    #[test]
    fn hostile_depth_and_size() {
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        assert!(matches!(parse(deep.as_bytes()), Err(ImportError::TooDeep)));
        let big = vec![b' '; super::super::MAX_INPUT_BYTES + 1];
        assert!(matches!(parse(&big), Err(ImportError::TooLarge(_))));
    }

    #[test]
    fn too_many_requests() {
        let item = r#"{"name":"r","request":{"method":"GET","url":"http://h/x"}}"#;
        let items = vec![item; 5001].join(",");
        let doc = format!(
            r#"{{"info":{{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},"item":[{items}]}}"#
        );
        assert!(matches!(
            parse(doc.as_bytes()),
            Err(ImportError::TooLarge(_))
        ));
    }

    #[test]
    fn many_same_named_secrets_are_linear() {
        let item = r#"{"name":"r","request":{"method":"GET","url":"http://h/x","auth":{"type":"basic","basic":[{"key":"username","value":"u"},{"key":"password","value":"p"}]}}}"#;
        let items = vec![item; 4000].join(",");
        let doc = format!(
            r#"{{"info":{{"name":"x","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},"item":[{items}]}}"#
        );
        let t = std::time::Instant::now();
        let r = parse(doc.as_bytes()).unwrap();
        let secrets = &r.environments[0].secrets;
        let names: std::collections::HashSet<_> = secrets.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names.len(), secrets.len());
        assert!(secrets.len() >= 8000);
        assert!(t.elapsed().as_secs() < 2);
    }

    fn one_request(body: serde_json::Value, url: serde_json::Value) -> (Request, ImportResult) {
        let doc = serde_json::json!({
            "info": {"name": "c", "_postman_id": "x",
                "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item": [{"name": "r", "request": {"method": "POST", "url": url, "body": body}}]
        });
        let r = parse(doc.to_string().as_bytes()).unwrap();
        let req = find(&r.items, "r").clone();
        (req, r)
    }

    #[test]
    fn file_paths_are_never_imported() {
        for bad in [
            "/etc/passwd",
            "~/.ssh/id_rsa",
            "../x",
            "C:\\x",
            "\\\\host\\share\\x",
        ] {
            let body = serde_json::json!({"mode": "formdata",
                "formdata": [{"key": "f", "type": "file", "src": bad}]});
            let (req, r) = one_request(body, "http://h/up".into());
            let Body::Multipart(parts) = req.body else {
                panic!()
            };
            assert_eq!(parts[0].value, PartValue::Text(String::new()), "{bad}");
            assert!(!parts[0].enabled);
            let w = r
                .warnings
                .iter()
                .find(|w| w.message.contains("`f`"))
                .unwrap();
            assert!(!w.message.contains(bad) && !w.at.contains(bad), "{bad}");
        }
        let body = serde_json::json!({"mode": "formdata",
            "formdata": [{"key": "f", "type": "file", "src": "assets/a.png"}]});
        let (req, _) = one_request(body, "http://h/up".into());
        let Body::Multipart(parts) = req.body else {
            panic!()
        };
        assert_eq!(parts[0].value, PartValue::Text(String::new()));
        assert!(!parts[0].enabled);
    }

    #[test]
    fn query_array_is_percent_decoded() {
        let url = serde_json::json!({"raw": "http://h/x?a%20b=c%20d",
            "query": [{"key": "a%20b", "value": "c%20d"}]});
        let (req, _) = one_request(serde_json::json!({}), url);
        assert_eq!(req.params[0].name, "a b");
        assert_eq!(req.params[0].value, "c d");

        let url = serde_json::json!({"raw": "http://h/x?a+b=c+d",
            "query": [{"key": "a+b", "value": "c+d"}]});
        let (req, _) = one_request(serde_json::json!({}), url);
        assert_eq!(req.params[0].name, "a b");
        assert_eq!(req.params[0].value, "c d");
    }

    #[test]
    fn raw_url_query_plus_is_space() {
        let url = serde_json::json!("http://h/x?k=a+b&j=a%2Bb");
        let (req, _) = one_request(serde_json::json!({}), url);
        assert_eq!(req.params[0].value, "a b");
        assert_eq!(req.params[1].value, "a+b");
    }

    #[test]
    fn url_userinfo_becomes_basic_auth_secrets() {
        let (req, r) = one_request(
            serde_json::json!({"mode": "raw", "raw": ""}),
            "http://bob:pw@h/up".into(),
        );
        assert_eq!(req.url, "http://h/up");
        assert!(matches!(req.auth, Auth::Basic { .. }));
        let env = &r.environments[0];
        assert!(env.secrets.iter().any(|v| v.value == "pw"));
        assert!(!format!("{req:?}").contains("pw"));
    }

    #[test]
    fn explicit_auth_beats_url_userinfo() {
        let doc = serde_json::json!({
            "info": {"name": "c", "_postman_id": "x",
                "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item": [{"name": "r", "request": {"method": "GET",
                "url": {"raw": "http://bob:pw@h/up"},
                "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "tok"}]}}}]
        });
        let r = parse(doc.to_string().as_bytes()).unwrap();
        let req = find(&r.items, "r");
        assert_eq!(req.url, "http://h/up");
        assert!(matches!(req.auth, Auth::Bearer { .. }));
        assert!(r.environments[0].secrets.iter().all(|v| v.value != "pw"));
        assert!(r.warnings.iter().any(|w| w.message.contains("credentials")));
    }
}
