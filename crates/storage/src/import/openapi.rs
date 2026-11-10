//! Import an OpenAPI 3.0 or 3.1 document (JSON or YAML).

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use wisp_core::Method;
use wisp_core::request::{Body, KeyValue, Request, TextBody, Var};

use super::{
    ImportError, ImportItem, ImportResult, ImportedEnv, RequestCounter, method_name, parse_json,
    yaml,
};

/// Bytes of example text all operations together may copy out of the
/// document; shared `$ref`s are copied once per use.
const MAX_EXAMPLE_BYTES: usize = 64 * 1024 * 1024;

fn spend(budget: &mut usize, n: usize) -> Result<(), ImportError> {
    *budget = budget.checked_sub(n).ok_or_else(|| {
        ImportError::TooLarge("example data over 64 MiB after resolving $refs".into())
    })?;
    Ok(())
}

const METHODS: [(&str, Method); 7] = [
    ("get", Method::Get),
    ("put", Method::Put),
    ("post", Method::Post),
    ("delete", Method::Delete),
    ("options", Method::Options),
    ("head", Method::Head),
    ("patch", Method::Patch),
];

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Parse an OpenAPI 3 document.
pub fn parse(input: &[u8]) -> Result<ImportResult, ImportError> {
    super::check_size(input.len())?;
    let input = input.strip_prefix(b"\xef\xbb\xbf").unwrap_or(input);
    let first = input.iter().copied().find(|b| !b.is_ascii_whitespace());
    let doc = if first == Some(b'{') {
        parse_json(input)?
    } else {
        yaml::parse(input)?
    };
    let version = text(doc.get("openapi"));
    if !version.starts_with("3.") {
        if doc.get("swagger").is_some() {
            return Err(ImportError::Unsupported(
                "Swagger 2.0 is not supported; convert to OpenAPI 3".into(),
            ));
        }
        return Err(ImportError::Parse(
            "not an OpenAPI 3 document: no `openapi` version".into(),
        ));
    }
    let title = text(doc.get("info").and_then(|i| i.get("title")));
    let mut result = ImportResult {
        name: if title.is_empty() {
            "OpenAPI import".into()
        } else {
            title
        },
        ..ImportResult::default()
    };
    let root = result.name.clone();
    environments(&doc, &mut result)?;
    if doc
        .get("components")
        .and_then(|c| c.get("securitySchemes"))
        .and_then(Value::as_object)
        .is_some_and(|m| !m.is_empty())
    {
        result.warn(&root, "security schemes are not imported");
    }

    let mut counter = RequestCounter::default();
    let mut budget = MAX_EXAMPLE_BYTES;
    let mut untagged: Vec<ImportItem> = Vec::new();
    let mut folders: Vec<(String, Vec<ImportItem>)> = Vec::new();
    if let Some(paths) = doc.get("paths").and_then(Value::as_object) {
        for (path, item) in paths {
            let at = path.clone();
            let Some(item) = deref(&doc, item, &at, &mut result) else {
                continue;
            };
            for (key, method) in METHODS {
                let Some(op) = item.get(key).filter(|o| o.is_object()) else {
                    continue;
                };
                counter.add()?;
                let (tag, request) =
                    operation(&doc, &mut result, &mut budget, path, method, item, op)?;
                let name = request.name.clone();
                let node = ImportItem::Request { name, request };
                match tag {
                    Some(t) => match folders.iter_mut().find(|(n, _)| *n == t) {
                        Some((_, v)) => v.push(node),
                        None => folders.push((t, vec![node])),
                    },
                    None => untagged.push(node),
                }
            }
            if item.get("trace").is_some() {
                result.warn(&at, "TRACE operations are not supported");
            }
        }
    }
    result.items = folders
        .into_iter()
        .map(|(name, items)| ImportItem::Folder { name, items })
        .chain(untagged)
        .collect();
    Ok(result)
}

pub(super) fn env_name(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Longest server URL accepted after variable substitution.
const MAX_URL_BYTES: usize = 64 * 1024;

/// Replace `{name}` placeholders in one pass over the original URL; text
/// substituted in is never rescanned.
fn substitute(url: &str, vars: &serde_json::Map<String, Value>) -> Result<String, ImportError> {
    let too_long = || ImportError::TooLarge(format!("server URL over {MAX_URL_BYTES} bytes"));
    if url.len() > MAX_URL_BYTES {
        return Err(too_long());
    }
    let mut out = String::new();
    let mut rest = url;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if vars.contains_key(&after[..close]) => {
                out.push_str(&text(vars[&after[..close]].get("default")));
                rest = &after[close + 1..];
            }
            None => {
                // No `}` anywhere ahead: nothing left can match.
                out.push('{');
                rest = after;
                break;
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
        if out.len() > MAX_URL_BYTES {
            return Err(too_long());
        }
    }
    out.push_str(rest);
    if out.len() > MAX_URL_BYTES {
        return Err(too_long());
    }
    Ok(out)
}

fn environments(doc: &Value, result: &mut ImportResult) -> Result<(), ImportError> {
    let root = result.name.clone();
    let servers = doc.get("servers").and_then(Value::as_array);
    let Some(servers) = servers.filter(|s| !s.is_empty()) else {
        result.warn(
            &root,
            "no `servers`; set the `baseUrl` variable before sending",
        );
        return Ok(());
    };
    let mut taken: HashSet<String> = HashSet::new();
    let mut next: HashMap<String, usize> = HashMap::new();
    for (i, s) in servers.iter().enumerate() {
        let mut url = text(s.get("url"));
        if let Some(vars) = s.get("variables").and_then(Value::as_object) {
            url = substitute(&url, vars)?;
        } else if url.len() > MAX_URL_BYTES {
            return Err(ImportError::TooLarge(format!(
                "server URL over {MAX_URL_BYTES} bytes"
            )));
        }
        let desc = text(s.get("description"));
        let mut name = env_name(if desc.is_empty() { &url } else { &desc });
        if name.is_empty() {
            name = format!("server-{}", i + 1);
        }
        let mut unique = name.clone();
        if taken.contains(&unique) {
            let n = next.entry(name.clone()).or_insert(2);
            loop {
                unique = format!("{name}-{n}");
                *n += 1;
                if !taken.contains(&unique) {
                    break;
                }
            }
        }
        taken.insert(unique.clone());
        result.environments.push(ImportedEnv {
            name: unique,
            vars: vec![Var {
                name: "baseUrl".into(),
                value: url,
            }],
            secrets: Vec::new(),
        });
    }
    Ok(())
}

/// Follow local `$ref`s; warn on external refs and cycles.
fn deref<'a>(
    root: &'a Value,
    v: &'a Value,
    at: &str,
    result: &mut ImportResult,
) -> Option<&'a Value> {
    let mut cur = v;
    let mut seen: HashSet<&str> = HashSet::new();
    while let Some(r) = cur.get("$ref").and_then(Value::as_str) {
        let Some(pointer) = r.strip_prefix('#') else {
            result.warn(at, format!("external $ref `{r}` is not imported"));
            return None;
        };
        if !seen.insert(r) {
            result.warn(at, format!("$ref cycle at `{r}`"));
            return None;
        }
        let mut node = root;
        if !pointer.is_empty() {
            let Some(rest) = pointer.strip_prefix('/') else {
                result.warn(at, format!("unsupported $ref `{r}`"));
                return None;
            };
            for seg in rest.split('/') {
                let seg = super::percent_decode(seg)
                    .replace("~1", "/")
                    .replace("~0", "~");
                let next = match node {
                    Value::Object(m) => m.get(&seg),
                    Value::Array(a) => seg.parse::<usize>().ok().and_then(|i| a.get(i)),
                    _ => None,
                };
                match next {
                    Some(n) => node = n,
                    None => {
                        result.warn(at, format!("$ref `{r}` does not resolve"));
                        return None;
                    }
                }
            }
        }
        cur = node;
    }
    Some(cur)
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(scalar_text).collect::<Vec<_>>().join(","),
        Value::Object(_) => v.to_string(),
    }
}

/// An example value for a parameter or media object, if one is given.
fn example<'a>(
    doc: &'a Value,
    node: &'a Value,
    at: &str,
    result: &mut ImportResult,
) -> Option<&'a Value> {
    if let Some(e) = node.get("example") {
        return Some(e);
    }
    if let Some(m) = node.get("examples").and_then(Value::as_object)
        && let Some((_, first)) = m.iter().next()
        && let Some(ex) = deref(doc, first, at, result)
        && let Some(v) = ex.get("value")
    {
        return Some(v);
    }
    let schema = deref(doc, node.get("schema")?, at, result)?;
    schema.get("example").or_else(|| schema.get("default"))
}

fn operation(
    doc: &Value,
    result: &mut ImportResult,
    budget: &mut usize,
    path: &str,
    method: Method,
    item: &Value,
    op: &Value,
) -> Result<(Option<String>, Request), ImportError> {
    let fallback = format!("{} {path}", method_name(method));
    let mut name = text(op.get("summary"));
    if name.trim().is_empty() {
        name = text(op.get("operationId"));
    }
    if name.trim().is_empty() {
        name = fallback.clone();
    }
    let at = format!("{path} {}", method_name(method));
    let mut req = Request {
        name: name.clone(),
        method,
        url: format!("{{{{baseUrl}}}}{path}"),
        ..Request::default()
    };

    // Path-level parameters first; an operation parameter replaces one with
    // the same name and location.
    let mut params: Vec<&Value> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for list in [item.get("parameters"), op.get("parameters")] {
        for p in list.and_then(Value::as_array).into_iter().flatten() {
            let Some(p) = deref(doc, p, &at, result) else {
                continue;
            };
            let key = (text(p.get("name")), text(p.get("in")));
            if let Some(&i) = index.get(&key) {
                params[i] = p;
            } else {
                index.insert(key, params.len());
                params.push(p);
            }
        }
    }
    for p in params {
        let pname = text(p.get("name"));
        if pname.is_empty() {
            continue;
        }
        let value = example(doc, p, &at, result).map(scalar_text);
        spend(budget, pname.len() + value.as_ref().map_or(0, String::len))?;
        let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        let kv = |value: Option<String>| KeyValue {
            enabled: required || value.is_some(),
            name: pname.clone(),
            value: value.unwrap_or_default(),
        };
        match text(p.get("in")).as_str() {
            "path" => req.path.push(Var {
                name: pname,
                value: value.unwrap_or_default(),
            }),
            "query" => req.params.push(kv(value)),
            "header" => {
                if !["accept", "content-type", "authorization"]
                    .contains(&pname.to_ascii_lowercase().as_str())
                {
                    req.headers.push(kv(value));
                }
            }
            other => result.warn(
                &at,
                format!("`{other}` parameter `{pname}` is not imported"),
            ),
        }
    }

    if let Some(body) = op.get("requestBody")
        && let Some(body) = deref(doc, body, &at, result)
        && let Some(content) = body.get("content").and_then(Value::as_object)
    {
        let json = content
            .iter()
            .find(|(k, _)| k.as_str() == "application/json")
            .or_else(|| content.iter().find(|(k, _)| k.contains("json")));
        match json {
            Some((_, media)) => {
                let content = example(doc, media, &at, result)
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => serde_json::to_string_pretty(other).unwrap_or_default(),
                    })
                    .unwrap_or_default();
                spend(budget, content.len())?;
                req.body = Body::Json(TextBody {
                    content,
                    sidecar: None,
                });
            }
            None => {
                let kinds: Vec<&str> = content.keys().map(String::as_str).collect();
                result.warn(
                    &at,
                    format!("request body type {} is not imported", kinds.join(", ")),
                );
            }
        }
    }

    let tag = op
        .get("tags")
        .and_then(Value::as_array)
        .and_then(|t| t.first())
        .map(|t| text(Some(t)))
        .filter(|t| !t.trim().is_empty());
    Ok((tag, req))
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &[u8] = include_bytes!("../../tests/fixtures/import/openapi.json");
    const YAML: &[u8] = include_bytes!("../../tests/fixtures/import/openapi.yaml");

    fn requests(items: &[ImportItem]) -> Vec<&Request> {
        let mut out = Vec::new();
        for i in items {
            match i {
                ImportItem::Request { request, .. } => out.push(request),
                ImportItem::Folder { items, .. } => out.extend(requests(items)),
            }
        }
        out
    }

    #[test]
    fn json_and_yaml_are_equal() {
        assert_eq!(parse(JSON).unwrap(), parse(YAML).unwrap());
    }

    #[test]
    fn operations_and_names() {
        let r = parse(JSON).unwrap();
        assert_eq!(r.name, "Pets API");
        let reqs = requests(&r.items);
        let names: Vec<_> = reqs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"List pets"));
        assert!(names.contains(&"getPet"));
        assert!(names.contains(&"DELETE /pets/{id}"));
        let list = reqs.iter().find(|r| r.name == "List pets").unwrap();
        assert_eq!(list.url, "{{baseUrl}}/pets");
        assert_eq!(list.params[0].name, "limit");
        assert_eq!(list.params[0].value, "10");
        let ImportItem::Folder { name, .. } = &r.items[0] else {
            panic!()
        };
        assert_eq!(name, "pets");
    }

    #[test]
    fn two_servers_two_environments() {
        let r = parse(JSON).unwrap();
        assert_eq!(r.environments.len(), 2);
        assert_eq!(r.environments[0].name, "Production");
        assert_eq!(
            r.environments[0].vars[0].value,
            "https://api.example.com/v2"
        );
        assert_eq!(r.environments[1].vars[0].name, "baseUrl");
    }

    #[test]
    fn body_example_and_path_params() {
        let r = parse(JSON).unwrap();
        let reqs = requests(&r.items);
        let create = reqs.iter().find(|r| r.name == "Create pet").unwrap();
        let Body::Json(t) = &create.body else {
            panic!()
        };
        assert!(t.content.contains("rex"));
        let get = reqs.iter().find(|r| r.name == "getPet").unwrap();
        assert_eq!(get.path[0].name, "id");
        assert_eq!(get.path[0].value, "42");
    }

    #[test]
    fn ref_cycle_terminates_and_external_warns() {
        let r = parse(JSON).unwrap();
        assert!(r.warnings.iter().any(|w| w.message.contains("cycle")));
        assert!(
            r.warnings
                .iter()
                .any(|w| w.message.contains("external $ref"))
        );
    }

    #[test]
    fn swagger_2_is_error() {
        let e = parse(br#"{"swagger":"2.0","paths":{}}"#).unwrap_err();
        assert!(matches!(e, ImportError::Unsupported(_)));
        assert!(parse(b"{}").is_err());
        assert!(parse(b"just: text").is_err());
    }

    #[test]
    fn server_variables_use_defaults() {
        let r = parse(
            br#"{"openapi":"3.1.0","servers":[{"url":"https://{env}.x.com","variables":{"env":{"default":"dev"}}}],"paths":{}}"#,
        )
        .unwrap();
        assert_eq!(r.environments[0].vars[0].value, "https://dev.x.com");
    }

    #[test]
    fn hostile_inputs() {
        let t = std::time::Instant::now();
        let big = vec![b' '; super::super::MAX_INPUT_BYTES + 1];
        assert!(matches!(parse(&big), Err(ImportError::TooLarge(_))));
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        assert!(matches!(parse(deep.as_bytes()), Err(ImportError::TooDeep)));
        let mut paths = String::new();
        for i in 0..5001 {
            if i > 0 {
                paths.push(',');
            }
            paths.push_str(&format!(r#""/p{i}":{{"get":{{}}}}"#));
        }
        let doc = format!(r#"{{"openapi":"3.0.0","paths":{{{paths}}}}}"#);
        assert!(matches!(
            parse(doc.as_bytes()),
            Err(ImportError::TooLarge(_))
        ));
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn shared_ref_examples_hit_the_byte_budget() {
        let big = "x".repeat(1024 * 1024);
        let mut paths = String::new();
        for i in 0..70 {
            if i > 0 {
                paths.push(',');
            }
            paths.push_str(&format!(
                r##""/p{i}":{{"post":{{"requestBody":{{"$ref":"#/components/requestBodies/B"}}}}}}"##
            ));
        }
        let doc = format!(
            r##"{{"openapi":"3.0.0","paths":{{{paths}}},"components":{{"requestBodies":{{"B":{{"content":{{"application/json":{{"example":"{big}"}}}}}}}}}}}}"##
        );
        let r = parse(doc.as_bytes());
        assert!(
            matches!(r, Err(ImportError::TooLarge(_))),
            "{:?}",
            r.map(|r| r.warnings)
        );
    }

    #[test]
    fn many_parameters_merge_in_linear_time() {
        let mut params = String::new();
        for i in 0..60_000 {
            if i > 0 {
                params.push(',');
            }
            params.push_str(&format!(r#"{{"name":"p{i}","in":"query"}}"#));
        }
        let doc = format!(
            r#"{{"openapi":"3.0.0","paths":{{"/a":{{"parameters":[{params}],"get":{{"parameters":[{params}]}}}}}}}}"#
        );
        let t = std::time::Instant::now();
        let r = parse(doc.as_bytes()).unwrap();
        assert_eq!(requests(&r.items)[0].params.len(), 60_000);
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn server_variable_bomb_is_too_large() {
        let doc = serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "t", "version": "1"},
            "paths": {},
            "servers": [{
                "url": "{a}".repeat(1000),
                "variables": {
                    "a": {"default": "{b}".repeat(1000)},
                    "b": {"default": "x".repeat(200)}
                }
            }]
        });
        let start = std::time::Instant::now();
        let r = parse(doc.to_string().as_bytes());
        assert!(matches!(r, Err(ImportError::TooLarge(_))), "{r:?}");
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn server_variables_substitute_once() {
        let doc = serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "t", "version": "1"},
            "paths": {},
            "servers": [{
                "url": "https://{h}/{v}",
                "variables": {"h": {"default": "{v}"}, "v": {"default": "api"}}
            }]
        });
        let r = parse(doc.to_string().as_bytes()).unwrap();
        assert_eq!(r.environments[0].vars[0].value, "https://{v}/api");
    }

    #[test]
    fn brace_heavy_url_is_linear() {
        let vars = serde_json::json!({"z": {}});
        let vars = vars.as_object().unwrap();
        let start = std::time::Instant::now();
        let big = format!("{}{}", "{".repeat(30_000), "a".repeat(30_000));
        assert_eq!(substitute(&big, vars).unwrap(), big);
        let closed = format!("{}}}", "{".repeat(60_000));
        assert_eq!(substitute(&closed, vars).unwrap(), closed);
        let over = format!("{}{}", "{".repeat(65_000), "a".repeat(1024 * 1024));
        assert!(matches!(
            substitute(&over, vars),
            Err(ImportError::TooLarge(_))
        ));
        assert!(start.elapsed().as_millis() < 500);
    }

    #[test]
    fn many_identical_servers_name_quickly() {
        let servers: Vec<_> = (0..3000).map(|_| serde_json::json!({"url": "a"})).collect();
        let doc = serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "t", "version": "1"},
            "paths": {},
            "servers": servers
        });
        let start = std::time::Instant::now();
        let r = parse(doc.to_string().as_bytes()).unwrap();
        assert!(start.elapsed().as_secs() < 2);
        assert_eq!(r.environments.len(), 3000);
        assert_eq!(r.environments[2999].name, "a-3000");
    }
}
