//! Request file load and save.
//!
//! A request is a `.toml` file, plus an optional sidecar file holding a large text body.
//! Saves edit the existing document in place so comments and key order survive.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, TableLike, Value};
use wisp_core::Method;
use wisp_core::request::{Auth, Body, Part, PartValue, Request, RequestSettings, TextBody};

use crate::{StorageError, codec, fs};

const BODY_KEYS: [&str; 5] = ["content", "file", "fields", "parts", "path"];

fn invalid(path: &Path, key: impl Into<String>, message: impl Into<String>) -> StorageError {
    StorageError::Invalid {
        path: path.to_path_buf(),
        key: key.into(),
        message: message.into(),
    }
}

/// Prefix the key of an `Invalid` error, leaving other errors alone.
fn with_key(e: StorageError, key: impl Into<String>) -> StorageError {
    match e {
        StorageError::Invalid { path, message, .. } => StorageError::Invalid {
            path,
            key: key.into(),
            message,
        },
        other => other,
    }
}

fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

fn resolve(dir: &Path, s: &str) -> PathBuf {
    let p = Path::new(s);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

fn path_string(p: &Path, dir: &Path) -> String {
    match p.strip_prefix(dir) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        _ => p.to_string_lossy().into_owned(),
    }
}

fn method_str(m: Method) -> &'static str {
    match m {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
    }
}

fn parse_method(s: &str) -> Option<Method> {
    Some(match s {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => return None,
    })
}

/// Load a request file and any sidecar body.
pub fn load_request(path: &Path) -> Result<Request, StorageError> {
    let text = fs::read_to_string(path)?;
    let doc = codec::parse(path, &text)?;
    let dir = dir_of(path);

    let name = codec::read_str(&doc, "name", path)?;
    let method_text = codec::read_str(&doc, "method", path)?;
    let method = parse_method(&method_text)
        .ok_or_else(|| invalid(path, "method", format!("unknown method `{method_text}`")))?;
    let url = codec::read_str(&doc, "url", path)?;
    Ok(Request {
        name,
        method,
        url,
        path: codec::read_vars(&doc, "path", path)?,
        params: codec::read_key_values(&doc, "params", path)?,
        headers: codec::read_key_values(&doc, "headers", path)?,
        auth: codec::read_auth(&doc, Auth::Inherit, path)?,
        body: read_body(&doc, dir, path)?,
        vars: codec::read_vars(&doc, "vars", path)?,
        settings: read_settings(&doc, path)?,
    })
}

fn read_settings(doc: &Table, path: &Path) -> Result<RequestSettings, StorageError> {
    let mut s = RequestSettings::default();
    let Some(item) = doc.get("settings") else {
        return Ok(s);
    };
    let t = item
        .as_table_like()
        .ok_or_else(|| invalid(path, "settings", "expected a table"))?;
    if let Some(v) = t.get("timeout_ms") {
        let ms = v
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| {
                invalid(
                    path,
                    "settings.timeout_ms",
                    "expected a non-negative integer",
                )
            })?;
        s.timeout = Some(Duration::from_millis(ms));
    }
    let flag = |k: &str, default: bool| -> Result<bool, StorageError> {
        match t.get(k) {
            None => Ok(default),
            Some(v) => v
                .as_bool()
                .ok_or_else(|| invalid(path, format!("settings.{k}"), "expected a boolean")),
        }
    };
    s.follow_redirects = flag("follow_redirects", true)?;
    s.verify_tls = flag("verify_tls", true)?;
    Ok(s)
}

fn tables_of<'a>(
    item: &'a Item,
    key: &str,
    path: &Path,
) -> Result<Vec<&'a dyn TableLike>, StorageError> {
    match item {
        Item::ArrayOfTables(a) => Ok(a.iter().map(|t| t as &dyn TableLike).collect()),
        Item::Value(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v.as_inline_table()
                    .map(|t| t as &dyn TableLike)
                    .ok_or_else(|| invalid(path, format!("{key}[{i}]"), "expected a table"))
            })
            .collect(),
        _ => Err(invalid(path, key, "expected an array of tables")),
    }
}

fn read_parts(item: &Item, dir: &Path, path: &Path) -> Result<Vec<Part>, StorageError> {
    let mut out = Vec::new();
    for (i, t) in tables_of(item, "body.parts", path)?.into_iter().enumerate() {
        let full = |f: &str| format!("body.parts[{i}].{f}");
        let get = |f: &str| -> Result<Option<String>, StorageError> {
            match t.get(f) {
                None => Ok(None),
                Some(v) => v
                    .as_str()
                    .map(|s| Some(s.to_string()))
                    .ok_or_else(|| invalid(path, full(f), "expected a string")),
            }
        };
        let name = get("name")?.ok_or_else(|| invalid(path, full("name"), "missing"))?;
        let value = match (get("value")?, get("file")?) {
            (Some(v), None) => PartValue::Text(v),
            (None, Some(f)) => PartValue::File(resolve(dir, &f)),
            (None, None) => return Err(invalid(path, full("value"), "needs `value` or `file`")),
            (Some(_), Some(_)) => {
                return Err(invalid(path, full("file"), "has both `value` and `file`"));
            }
        };
        let enabled = match t.get("enabled") {
            None => true,
            Some(e) => e
                .as_bool()
                .ok_or_else(|| invalid(path, full("enabled"), "expected a boolean"))?,
        };
        out.push(Part {
            name,
            value,
            enabled,
        });
    }
    Ok(out)
}

/// Check a sidecar name and return its full path. The name must be a plain
/// file name in the request file's folder, not the request file itself, and a
/// symlink must resolve to somewhere inside that folder.
fn sidecar_path(dir: &Path, path: &Path, name: &str) -> Result<PathBuf, StorageError> {
    let bad = |why: &str| invalid(path, "body.file", format!("sidecar `{name}` {why}"));
    let drive_prefixed = name.as_bytes().get(1) == Some(&b':');
    if name.is_empty() || name == "." || name == ".." {
        return Err(bad("is not a file name"));
    }
    if name.contains(['/', '\\']) || drive_prefixed {
        return Err(bad("must be a plain file name in the request's folder"));
    }
    if Path::new(name).is_absolute() || Path::new(name).components().count() != 1 {
        return Err(bad("must be a plain file name in the request's folder"));
    }
    let own = path.file_name().map(|n| n.to_string_lossy().to_lowercase());
    if own.as_deref() == Some(name.to_lowercase().as_str()) {
        return Err(bad("is the request file itself"));
    }
    let target = dir.join(name);
    // Same file as the request (symlink, Unicode-normalisation or Windows alias).
    let same = match (std::fs::canonicalize(&target), std::fs::canonicalize(path)) {
        (Ok(t), Ok(r)) => t == r,
        _ => false,
    };
    if same {
        return Err(bad("is the request file itself"));
    }
    if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
        let inside = match (std::fs::canonicalize(&target), std::fs::canonicalize(dir)) {
            (Ok(t), Ok(d)) => t.starts_with(d),
            _ => false,
        };
        if !inside {
            return Err(bad("is a symlink that leaves the request's folder"));
        }
    }
    Ok(target)
}

fn read_text_body(t: &Table, dir: &Path, path: &Path) -> Result<TextBody, StorageError> {
    let field =
        |k: &str| codec::read_opt_str(t, k, path).map_err(|e| with_key(e, format!("body.{k}")));
    match (field("file")?, field("content")?) {
        (Some(_), Some(_)) => Err(invalid(path, "body.file", "has both `file` and `content`")),
        (Some(name), None) => {
            let target = sidecar_path(dir, path, &name)?;
            Ok(TextBody {
                content: fs::read_to_string(&target)?,
                sidecar: Some(name),
            })
        }
        (None, content) => Ok(TextBody {
            content: content.unwrap_or_default(),
            sidecar: None,
        }),
    }
}

fn read_body(doc: &Table, dir: &Path, path: &Path) -> Result<Body, StorageError> {
    let Some(item) = doc.get("body") else {
        return Ok(Body::None);
    };
    let t = item
        .as_table()
        .ok_or_else(|| invalid(path, "body", "expected a table"))?;
    let ty = codec::read_str(t, "type", path).map_err(|e| with_key(e, "body.type"))?;
    Ok(match ty.as_str() {
        "none" => Body::None,
        "json" => Body::Json(read_text_body(t, dir, path)?),
        "text" => Body::Text(read_text_body(t, dir, path)?),
        "xml" => Body::Xml(read_text_body(t, dir, path)?),
        "form" => Body::Form(match codec::read_key_values(t, "fields", path) {
            Ok(v) => v,
            Err(StorageError::Invalid { path, key, message }) => {
                return Err(StorageError::Invalid {
                    path,
                    key: format!("body.{key}"),
                    message,
                });
            }
            Err(e) => return Err(e),
        }),
        "multipart" => Body::Multipart(match t.get("parts") {
            None => Vec::new(),
            Some(p) => read_parts(p, dir, path)?,
        }),
        "file" => {
            let p = codec::read_opt_str(t, "path", path)
                .map_err(|e| with_key(e, "body.path"))?
                .ok_or_else(|| invalid(path, "body.path", "missing"))?;
            Body::File(resolve(dir, &p))
        }
        other => {
            return Err(invalid(
                path,
                "body.type",
                format!("unknown body type `{other}`"),
            ));
        }
    })
}

/// Save a request, editing the existing file in place when there is one.
pub fn save_request(path: &Path, request: &Request) -> Result<(), StorageError> {
    let dir = dir_of(path);
    let mut doc = match std::fs::read_to_string(path) {
        Ok(text) => codec::parse(path, &text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(e) => {
            return Err(StorageError::Io {
                path: path.to_path_buf(),
                source: e,
            });
        }
    };
    let had_auth = doc.get("auth").is_some();
    if let Body::Json(tb) | Body::Text(tb) | Body::Xml(tb) = &request.body
        && let Some(name) = &tb.sidecar
    {
        sidecar_path(dir, path, name)?;
    }

    codec::write_str(&mut doc, "name", &request.name);
    codec::write_str(&mut doc, "method", method_str(request.method));
    codec::write_str(&mut doc, "url", &request.url);
    codec::write_vars(&mut doc, "path", &request.path);
    codec::write_vars(&mut doc, "vars", &request.vars);
    codec::write_key_values(&mut doc, "params", &request.params);
    codec::write_key_values(&mut doc, "headers", &request.headers);
    if had_auth || request.auth != Auth::Inherit {
        codec::write_auth(&mut doc, &request.auth);
    }
    write_body(&mut doc, &request.body, dir, path)?;
    write_settings(&mut doc, &request.settings);

    fs::atomic_write(path, doc.to_string().as_bytes())
}

/// Replace a value, keeping the old one's decor (comments, spacing).
fn replace_value(t: &mut dyn TableLike, key: &str, new: Value) {
    if let Some(item) = t.get_mut(key)
        && let Some(old) = item.as_value()
    {
        let mut new = new;
        *new.decor_mut() = old.decor().clone();
        *item = Item::Value(new);
        return;
    }
    t.insert(key, Item::Value(new));
}

fn write_settings(doc: &mut Table, s: &RequestSettings) {
    if s.timeout.is_none() && s.follow_redirects && s.verify_tls && doc.get("settings").is_none() {
        return;
    }
    if doc.get("settings").and_then(Item::as_table_like).is_none() {
        doc.insert("settings", Item::Table(Table::new()));
    }
    let Some(t) = doc.get_mut("settings").and_then(Item::as_table_like_mut) else {
        return;
    };
    match s.timeout {
        Some(d) => {
            let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
            if t.get("timeout_ms").and_then(Item::as_integer) != Some(ms) {
                replace_value(t, "timeout_ms", Value::from(ms));
            }
        }
        None => {
            t.remove("timeout_ms");
        }
    }
    for (key, on) in [
        ("follow_redirects", s.follow_redirects),
        ("verify_tls", s.verify_tls),
    ] {
        let existing = t.get(key).and_then(Item::as_bool);
        if !on {
            if existing != Some(false) {
                replace_value(t, key, Value::from(false));
            }
        } else if existing != Some(true) {
            // The default: an explicit `true` stays as written, anything else is omitted.
            t.remove(key);
        }
    }
    if t.is_empty() {
        doc.remove("settings");
    }
}

/// Build a TOML string value: a multi-line literal when that round-trips, else basic.
fn string_value(s: &str) -> Value {
    let literal_ok = s.contains('\n')
        && !s.contains("'''")
        && !s.ends_with('\'')
        && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t');
    if literal_ok
        && let Ok(v) = Value::from_str(&format!("'''\n{s}'''"))
        && v.as_str() == Some(s)
    {
        return v;
    }
    Value::from(s)
}

fn set_content(t: &mut Table, content: &str) {
    if t.get("content").and_then(Item::as_str) == Some(content) {
        return;
    }
    replace_value(t, "content", string_value(content));
}

fn write_body(doc: &mut Table, body: &Body, dir: &Path, path: &Path) -> Result<(), StorageError> {
    if matches!(body, Body::None) {
        let explicit_none = doc
            .get("body")
            .and_then(Item::as_table_like)
            .is_some_and(|t| t.get("type").and_then(Item::as_str) == Some("none"));
        if doc.get("body").is_some() && !explicit_none {
            doc.remove("body");
        }
        return Ok(());
    }
    if doc.get("body").and_then(Item::as_table).is_none() {
        doc.insert("body", Item::Table(Table::new()));
    }
    let Some(t) = doc.get_mut("body").and_then(Item::as_table_mut) else {
        return Ok(());
    };
    let (ty, keep): (&str, &[&str]) = match body {
        Body::None => return Ok(()),
        Body::Json(tb) => ("json", text_keys(tb)),
        Body::Text(tb) => ("text", text_keys(tb)),
        Body::Xml(tb) => ("xml", text_keys(tb)),
        Body::Form(_) => ("form", &["fields"]),
        Body::Multipart(_) => ("multipart", &["parts"]),
        Body::File(_) => ("file", &["path"]),
    };
    codec::write_str(t, "type", ty);
    for k in BODY_KEYS {
        if !keep.contains(&k) {
            t.remove(k);
        }
    }
    match body {
        Body::Json(tb) | Body::Text(tb) | Body::Xml(tb) => match &tb.sidecar {
            Some(name) => {
                let target = sidecar_path(dir, path, name)?;
                let unchanged = std::fs::read_to_string(&target).is_ok_and(|c| c == tb.content);
                if !unchanged {
                    fs::atomic_write(&target, tb.content.as_bytes())?;
                }
                codec::write_str(t, "file", name);
            }
            None => set_content(t, &tb.content),
        },
        Body::Form(fields) => codec::write_key_values(t, "fields", fields),
        Body::Multipart(parts) => {
            let same = t
                .get("parts")
                .and_then(|p| read_parts(p, dir, path).ok())
                .is_some_and(|old| &old == parts);
            if !same {
                t.insert("parts", Item::ArrayOfTables(parts_table(parts, dir)));
            }
        }
        Body::File(p) => {
            let same = t
                .get("path")
                .and_then(Item::as_str)
                .is_some_and(|s| resolve(dir, s) == *p);
            if !same {
                codec::write_str(t, "path", &path_string(p, dir));
            }
        }
        Body::None => {}
    }
    Ok(())
}

fn text_keys(tb: &TextBody) -> &'static [&'static str] {
    if tb.sidecar.is_some() {
        &["file"]
    } else {
        &["content"]
    }
}

fn parts_table(parts: &[Part], dir: &Path) -> ArrayOfTables {
    let mut arr = ArrayOfTables::new();
    for p in parts {
        let mut t = Table::new();
        codec::write_str(&mut t, "name", &p.name);
        match &p.value {
            PartValue::Text(v) => codec::write_str(&mut t, "value", v),
            PartValue::File(f) => codec::write_str(&mut t, "file", &path_string(f, dir)),
        }
        if !p.enabled {
            t.insert("enabled", Item::Value(Value::from(false)));
        }
        arr.push(t);
    }
    arr
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::{KeyValue, Var};

    const EXAMPLE: &str = r#"name = "Create user"
method = "POST"
url = "{{baseUrl}}/orgs/{orgId}/files/{name}.{ext}"   # query lives in [[params]]

[path]
orgId = "{{orgId}}"
name = "report"
ext = "pdf"

[[params]]
name = "notify"
value = "true"
enabled = true

[[headers]]
name = "X-Trace"
value = "{{$uuid}}"
enabled = false

[auth]
type = "inherit"                     # none | inherit | basic | bearer | apikey

[body]
type = "json"                        # none | json | text | xml | form | multipart | file
file = "create-user.body.json"       # or: content = '''...''' for small bodies
"#;

    fn setup(text: &str) -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("req.toml");
        std::fs::write(&p, text).unwrap();
        (d, p)
    }

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

    fn example() -> (tempfile::TempDir, PathBuf) {
        let (d, p) = setup(EXAMPLE);
        std::fs::write(d.path().join("create-user.body.json"), "{\"a\": 1}\n").unwrap();
        (d, p)
    }

    #[test]
    fn spec_example_loads() {
        let (_d, p) = example();
        let r = load_request(&p).unwrap();
        assert_eq!(r.name, "Create user");
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, "{{baseUrl}}/orgs/{orgId}/files/{name}.{ext}");
        assert_eq!(
            r.path,
            vec![
                var("orgId", "{{orgId}}"),
                var("name", "report"),
                var("ext", "pdf")
            ]
        );
        assert_eq!(r.params, vec![kv("notify", "true", true)]);
        assert_eq!(r.headers, vec![kv("X-Trace", "{{$uuid}}", false)]);
        assert_eq!(r.auth, Auth::Inherit);
        assert_eq!(
            r.body,
            Body::Json(TextBody {
                content: "{\"a\": 1}\n".into(),
                sidecar: Some("create-user.body.json".into())
            })
        );
        assert_eq!(r.settings, RequestSettings::default());
    }

    #[test]
    fn unchanged_save_is_byte_identical() {
        let (_d, p) = example();
        let r = load_request(&p).unwrap();
        save_request(&p, &r).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), EXAMPLE);
    }

    #[test]
    fn changing_url_changes_one_line() {
        let (_d, p) = example();
        let mut r = load_request(&p).unwrap();
        r.url = "https://x.test".into();
        save_request(&p, &r).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after.lines().count(), EXAMPLE.lines().count());
        let diff = after
            .lines()
            .zip(EXAMPLE.lines())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diff, 1);
        assert_eq!(load_request(&p).unwrap().url, "https://x.test");
    }

    #[test]
    fn sidecar_is_written_and_reloaded() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let r = Request {
            name: "n".into(),
            body: Body::Json(TextBody {
                content: "{\"k\":\n 2}".into(),
                sidecar: Some("create-user.body.json".into()),
            }),
            ..Request::default()
        };
        save_request(&p, &r).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("create-user.body.json")).unwrap(),
            "{\"k\":\n 2}"
        );
        let toml = std::fs::read_to_string(&p).unwrap();
        assert!(toml.contains("file = \"create-user.body.json\""));
        assert!(!toml.contains("content"));
        assert_eq!(load_request(&p).unwrap(), r);
    }

    #[test]
    fn every_body_variant_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let inside = d.path().join("files").join("a.bin");
        let outside = std::env::temp_dir().join("elsewhere.bin");
        let bodies = vec![
            Body::None,
            Body::Json(TextBody {
                content: "{}".into(),
                sidecar: None,
            }),
            Body::Json(TextBody {
                content: "a\nb\n".into(),
                sidecar: None,
            }),
            Body::Text(TextBody {
                content: "it's\ttabbed \"q\" \\ \r\n".into(),
                sidecar: None,
            }),
            Body::Xml(TextBody {
                content: "\n<a/>\n'".into(),
                sidecar: Some("b.xml".into()),
            }),
            Body::Form(vec![kv("a", "1", true), kv("b", "2", false)]),
            Body::Multipart(vec![
                Part {
                    name: "t".into(),
                    value: PartValue::Text("v".into()),
                    enabled: true,
                },
                Part {
                    name: "f".into(),
                    value: PartValue::File(inside.clone()),
                    enabled: false,
                },
                Part {
                    name: "o".into(),
                    value: PartValue::File(outside.clone()),
                    enabled: true,
                },
            ]),
            Body::File(outside),
            Body::File(inside),
        ];
        // Saving each in turn over the same file also checks switching types in place.
        for body in bodies {
            let r = Request {
                name: "n".into(),
                body,
                ..Request::default()
            };
            save_request(&p, &r).unwrap();
            assert_eq!(load_request(&p).unwrap(), r);
        }
    }

    #[test]
    fn other_fields_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let r = Request {
            name: "n".into(),
            method: Method::Delete,
            url: "u".into(),
            path: vec![var("a", "b")],
            params: vec![kv("p", "1", true)],
            headers: vec![kv("h", "2", false)],
            auth: Auth::Bearer { token: "t".into() },
            vars: vec![var("v", "w")],
            settings: RequestSettings {
                timeout: Some(Duration::from_millis(1500)),
                follow_redirects: false,
                verify_tls: false,
            },
            ..Request::default()
        };
        save_request(&p, &r).unwrap();
        assert_eq!(load_request(&p).unwrap(), r);
        let toml = std::fs::read_to_string(&p).unwrap();
        assert!(toml.find("[path]").unwrap() < toml.find("[[params]]").unwrap());
        // Back to defaults: the settings table disappears.
        let d2 = Request {
            settings: RequestSettings::default(),
            ..r
        };
        save_request(&p, &d2).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("settings"));
        assert_eq!(load_request(&p).unwrap(), d2);
    }

    #[test]
    fn body_with_triple_quote_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let r = Request {
            name: "n".into(),
            body: Body::Text(TextBody {
                content: "a '''b''' \nc".into(),
                sidecar: None,
            }),
            ..Request::default()
        };
        save_request(&p, &r).unwrap();
        assert_eq!(load_request(&p).unwrap(), r);
    }

    #[test]
    fn multiline_content_is_a_literal_string() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let r = Request {
            name: "n".into(),
            body: Body::Json(TextBody {
                content: "{\n  \"a\": 1\n}".into(),
                sidecar: None,
            }),
            ..Request::default()
        };
        save_request(&p, &r).unwrap();
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("content = '''\n{\n")
        );
    }

    #[test]
    fn unknown_method_is_invalid() {
        let (_d, p) = setup("name = \"x\"\nmethod = \"BREW\"\nurl = \"u\"\n");
        match load_request(&p) {
            Err(StorageError::Invalid { key, .. }) => assert_eq!(key, "method"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_sidecar_is_io_with_its_path() {
        let (d, p) = setup(EXAMPLE);
        match load_request(&p) {
            Err(StorageError::Io { path, .. }) => {
                assert_eq!(path, d.path().join("create-user.body.json"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn file_body_under_the_directory_is_relative() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let r = Request {
            name: "n".into(),
            body: Body::File(d.path().join("data").join("x.bin")),
            ..Request::default()
        };
        save_request(&p, &r).unwrap();
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("path = \"data/x.bin\"")
        );
    }

    #[test]
    fn absent_auth_stays_absent() {
        let text = "name = \"x\"\nmethod = \"GET\"\nurl = \"u\"\n";
        let (_d, p) = setup(text);
        let r = load_request(&p).unwrap();
        assert_eq!(r.auth, Auth::Inherit);
        save_request(&p, &r).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
    }

    #[test]
    fn explicit_inherit_auth_is_kept() {
        let text = "name = \"x\"\nmethod = \"GET\"\nurl = \"u\"\n\n[auth]\ntype = \"inherit\"\n";
        let (_d, p) = setup(text);
        let r = load_request(&p).unwrap();
        save_request(&p, &r).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
    }

    #[test]
    fn new_file_uses_spec_key_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let r = Request {
            name: "n".into(),
            url: "u".into(),
            auth: Auth::None,
            body: Body::Text(TextBody {
                content: "x".into(),
                sidecar: None,
            }),
            ..Request::default()
        };
        save_request(&p, &r).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.starts_with("name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n"));
        assert!(t.find("[auth]").unwrap() < t.find("[body]").unwrap());
    }

    fn sidecar_req(name: &str) -> String {
        format!(
            "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[body]\ntype = \"text\"\nfile = '{name}'\n"
        )
    }

    fn bad_key(r: Result<impl std::fmt::Debug, StorageError>) {
        match r {
            Err(StorageError::Invalid { key, .. }) => assert_eq!(key, "body.file"),
            other => panic!("expected Invalid body.file, got {other:?}"),
        }
    }

    fn rejected_on_load_and_save(name: &str, own: &str) {
        let d = tempfile::tempdir().unwrap();
        let folder = d.path().join("coll/sub");
        std::fs::create_dir_all(&folder).unwrap();
        let victim = d.path().join("victim.txt");
        std::fs::write(&victim, "SECRET").unwrap();
        let p = folder.join(own);
        let text = sidecar_req(name);
        std::fs::write(&p, &text).unwrap();
        let v_mtime = std::fs::metadata(&victim).unwrap().modified().unwrap();
        let r_mtime = std::fs::metadata(&p).unwrap().modified().unwrap();

        bad_key(load_request(&p));

        let mut req = Request {
            body: Body::Text(TextBody {
                content: "EDITED".into(),
                sidecar: Some(name.into()),
            }),
            ..Request::default()
        };
        req.name = "n".into();
        bad_key(save_request(&p, &req));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "SECRET");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().modified().unwrap(),
            v_mtime
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), r_mtime);
    }

    #[test]
    fn sidecar_parent_traversal_rejected() {
        rejected_on_load_and_save("../../victim.txt", "r.toml");
        rejected_on_load_and_save("../x", "r.toml");
    }

    #[test]
    fn sidecar_absolute_and_nested_and_drive_rejected() {
        let d = tempfile::tempdir().unwrap();
        let abs = d.path().join("victim.txt");
        rejected_on_load_and_save(abs.to_str().unwrap(), "r.toml");
        rejected_on_load_and_save("/abs", "r.toml");
        rejected_on_load_and_save("sub/x", "r.toml");
        rejected_on_load_and_save("sub\\x", "r.toml");
        rejected_on_load_and_save("C:x", "r.toml");
        rejected_on_load_and_save("C:\\x", "r.toml");
        rejected_on_load_and_save("..", "r.toml");
        rejected_on_load_and_save(".", "r.toml");
        rejected_on_load_and_save("", "r.toml");
    }

    #[test]
    fn sidecar_named_like_request_file_rejected() {
        rejected_on_load_and_save("r.toml", "r.toml");
        rejected_on_load_and_save("R.TOML", "r.toml");
    }

    #[cfg(unix)]
    #[test]
    fn sidecar_symlink_outside_folder_rejected_inside_allowed() {
        let d = tempfile::tempdir().unwrap();
        let folder = d.path().join("coll");
        std::fs::create_dir_all(&folder).unwrap();
        let victim = d.path().join("victim.txt");
        std::fs::write(&victim, "SECRET").unwrap();
        std::os::unix::fs::symlink(&victim, folder.join("link.txt")).unwrap();
        let p = folder.join("r.toml");
        std::fs::write(&p, sidecar_req("link.txt")).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let v_mtime = std::fs::metadata(&victim).unwrap().modified().unwrap();
        let r_mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        bad_key(load_request(&p));
        let req = Request {
            name: "n".into(),
            body: Body::Text(TextBody {
                content: "EDITED".into(),
                sidecar: Some("link.txt".into()),
            }),
            ..Request::default()
        };
        bad_key(save_request(&p, &req));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "SECRET");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().modified().unwrap(),
            v_mtime
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), r_mtime);

        std::fs::write(folder.join("real.txt"), "hi").unwrap();
        std::os::unix::fs::symlink(folder.join("real.txt"), folder.join("ok.txt")).unwrap();
        std::fs::write(&p, sidecar_req("ok.txt")).unwrap();
        let mut r = load_request(&p).unwrap();
        assert!(matches!(&r.body, Body::Text(t) if t.content == "hi"));
        if let Body::Text(t) = &mut r.body {
            t.content = "bye".into();
        }
        save_request(&p, &r).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("real.txt")).unwrap(),
            "bye"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sidecar_symlink_to_request_file_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.toml");
        let text = sidecar_req("self.txt");
        std::fs::write(&p, &text).unwrap();
        std::os::unix::fs::symlink(&p, d.path().join("self.txt")).unwrap();
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        bad_key(load_request(&p));
        let req = Request {
            name: "n".into(),
            body: Body::Text(TextBody {
                content: "EDITED".into(),
                sidecar: Some("self.txt".into()),
            }),
            ..Request::default()
        };
        bad_key(save_request(&p, &req));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), mtime);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sidecar_nfd_alias_of_request_file_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("caf\u{e9}.toml");
        let nfd = "cafe\u{301}.toml";
        let text = sidecar_req(nfd);
        std::fs::write(&p, &text).unwrap();
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        bad_key(load_request(&p));
        let req = Request {
            name: "n".into(),
            body: Body::Text(TextBody {
                content: "EDITED".into(),
                sidecar: Some(nfd.into()),
            }),
            ..Request::default()
        };
        bad_key(save_request(&p, &req));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), mtime);
    }

    #[cfg(unix)]
    #[test]
    fn bare_relative_request_path_allows_inside_symlink() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("real.txt"), "hi").unwrap();
        std::os::unix::fs::symlink(d.path().join("real.txt"), d.path().join("ok.txt")).unwrap();
        std::fs::write(d.path().join("r.toml"), sidecar_req("ok.txt")).unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(d.path()).unwrap();
        let r = load_request(Path::new("r.toml"));
        std::env::set_current_dir(cwd).unwrap();
        let r = r.unwrap();
        assert!(matches!(&r.body, Body::Text(t) if t.content == "hi"));
    }
}
