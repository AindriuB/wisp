//! TOML codec for request, folder, collection and environment files.
//!
//! Reads and writes go through `toml_edit` so comments and formatting survive.

use std::path::Path;

use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike, Value};
use wisp_core::request::{ApiKeyLocation, Auth, KeyValue, Var};

use crate::StorageError;

const AUTH_FIELDS: [&str; 6] = ["username", "password", "token", "name", "value", "in"];

fn invalid(path: &Path, key: impl Into<String>, message: impl Into<String>) -> StorageError {
    StorageError::Invalid {
        path: path.to_path_buf(),
        key: key.into(),
        message: message.into(),
    }
}

/// Parse TOML text, mapping syntax errors to `Parse` with the line number.
pub fn parse(path: &Path, text: &str) -> Result<DocumentMut, StorageError> {
    text.parse::<DocumentMut>().map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1);
        let message = match line {
            Some(l) => format!("line {l}: {}", e.message()),
            None => e.message().to_string(),
        };
        StorageError::Parse {
            path: path.to_path_buf(),
            message,
        }
    })
}

fn table_like<'a>(
    item: &'a Item,
    key: &str,
    path: &Path,
) -> Result<&'a dyn TableLike, StorageError> {
    item.as_table_like()
        .ok_or_else(|| invalid(path, key, "expected a table"))
}

/// Read a required string key.
pub fn read_str(table: &Table, key: &str, path: &Path) -> Result<String, StorageError> {
    read_opt_str(table, key, path)?.ok_or_else(|| invalid(path, key, "missing"))
}

/// Read an optional string key.
pub fn read_opt_str(table: &Table, key: &str, path: &Path) -> Result<Option<String>, StorageError> {
    opt_str_in(table, key, key, path)
}

fn opt_str_in(
    t: &dyn TableLike,
    key: &str,
    full: &str,
    path: &Path,
) -> Result<Option<String>, StorageError> {
    match t.get(key) {
        None => Ok(None),
        Some(item) => item
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| invalid(path, full, "expected a string")),
    }
}

/// Set a string key. An equal value is untouched; a changed one keeps its decor.
pub fn write_str(table: &mut Table, key: &str, value: &str) {
    set_str(table, key, value);
}

fn set_str(t: &mut dyn TableLike, key: &str, value: &str) {
    if let Some(item) = t.get_mut(key) {
        if item.as_str() == Some(value) {
            return;
        }
        if let Some(old) = item.as_value() {
            let decor = old.decor().clone();
            let mut new = Value::from(value);
            *new.decor_mut() = decor;
            *item = Item::Value(new);
            return;
        }
    }
    t.insert(key, Item::Value(Value::from(value)));
}

/// Read a `[vars]`-style table of string values.
pub fn read_vars(table: &Table, key: &str, path: &Path) -> Result<Vec<Var>, StorageError> {
    let Some(item) = table.get(key) else {
        return Ok(Vec::new());
    };
    let t = table_like(item, key, path)?;
    let mut out = Vec::new();
    for (name, v) in t.iter() {
        let value = v
            .as_str()
            .ok_or_else(|| invalid(path, format!("{key}.{name}"), "expected a string"))?;
        out.push(Var {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
    Ok(out)
}

/// Write a `[vars]`-style table, editing in place.
pub fn write_vars(table: &mut Table, key: &str, vars: &[Var]) {
    if vars.is_empty() && table.get(key).is_none() {
        return;
    }
    if table.get(key).and_then(Item::as_table_like).is_none() {
        table.insert(key, Item::Table(Table::new()));
    }
    let Some(t) = table.get_mut(key).and_then(Item::as_table_like_mut) else {
        return;
    };
    let stale: Vec<String> = t
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| !vars.iter().any(|v| &v.name == k))
        .collect();
    for k in stale {
        t.remove(&k);
    }
    for v in vars {
        set_str(t, &v.name, &v.value);
    }
}

/// Read an array of `{ name, value, enabled }` tables.
pub fn read_key_values(
    table: &Table,
    key: &str,
    path: &Path,
) -> Result<Vec<KeyValue>, StorageError> {
    let Some(item) = table.get(key) else {
        return Ok(Vec::new());
    };
    let mut entries: Vec<&dyn TableLike> = Vec::new();
    match item {
        Item::ArrayOfTables(a) => entries.extend(a.iter().map(|t| t as &dyn TableLike)),
        Item::Value(Value::Array(a)) => {
            for (i, v) in a.iter().enumerate() {
                match v.as_inline_table() {
                    Some(t) => entries.push(t),
                    None => return Err(invalid(path, format!("{key}[{i}]"), "expected a table")),
                }
            }
        }
        _ => return Err(invalid(path, key, "expected an array of tables")),
    }
    let mut out = Vec::new();
    for (i, t) in entries.into_iter().enumerate() {
        let full = |f: &str| format!("{key}[{i}].{f}");
        let name = opt_str_in(t, "name", &full("name"), path)?
            .ok_or_else(|| invalid(path, full("name"), "missing"))?;
        let value = opt_str_in(t, "value", &full("value"), path)?
            .ok_or_else(|| invalid(path, full("value"), "missing"))?;
        let enabled = match t.get("enabled") {
            None => true,
            Some(e) => e
                .as_bool()
                .ok_or_else(|| invalid(path, full("enabled"), "expected a boolean"))?,
        };
        out.push(KeyValue {
            name,
            value,
            enabled,
        });
    }
    Ok(out)
}

fn set_entry(t: &mut dyn TableLike, kv: &KeyValue) {
    set_str(t, "name", &kv.name);
    set_str(t, "value", &kv.value);
    let existing = t.get("enabled").and_then(Item::as_bool);
    if kv.enabled {
        if existing == Some(false) {
            t.remove("enabled");
        }
    } else if existing != Some(false) {
        t.insert("enabled", Item::Value(Value::from(false)));
    }
}

/// One output position of a list write.
enum Slot {
    /// Existing entry, left untouched.
    Keep(usize),
    /// Existing entry updated in place to the new entry.
    Edit(usize, usize),
    /// Brand new entry.
    Insert(usize),
}

fn lenient(t: &dyn TableLike) -> KeyValue {
    let s = |k: &str| {
        t.get(k)
            .and_then(Item::as_str)
            .unwrap_or_default()
            .to_string()
    };
    KeyValue {
        name: s("name"),
        value: s("value"),
        enabled: t.get("enabled").and_then(Item::as_bool).unwrap_or(true),
    }
}

/// Plan how `new` replaces `old`: anchor on a longest common subsequence, then pair
/// the leftovers between anchors as edits, deleting or inserting any surplus.
fn plan(old: &[KeyValue], new: &[KeyValue]) -> Vec<Slot> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut anchors = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    anchors.push((n, m));
    let mut slots = Vec::new();
    let (mut oi, mut nj) = (0, 0);
    for (ai, aj) in anchors {
        let (removed, added) = (ai - oi, aj - nj);
        for k in 0..removed.min(added) {
            slots.push(Slot::Edit(oi + k, nj + k));
        }
        for k in removed.min(added)..added {
            slots.push(Slot::Insert(nj + k));
        }
        if ai < n {
            slots.push(Slot::Keep(ai));
        }
        oi = ai + 1;
        nj = aj + 1;
    }
    slots
}

/// Write a list of `{ name, value, enabled }` tables, editing the existing file in place.
///
/// New entries are matched to existing tables by a longest-common-subsequence diff on
/// the full `(name, value, enabled)` triple. Matched tables are not touched. Between
/// matches, leftover old and new entries are paired in order as in-place edits; surplus
/// old tables are removed and surplus new ones inserted at their position.
pub fn write_key_values(table: &mut Table, key: &str, items: &[KeyValue]) {
    if items.is_empty() && table.get(key).is_none() {
        return;
    }
    match table.get_mut(key) {
        Some(Item::ArrayOfTables(arr)) => {
            let old: Vec<KeyValue> = arr.iter().map(|t| lenient(t)).collect();
            let slots = plan(&old, items);
            if is_identity(&slots, old.len()) {
                return;
            }
            let src: Vec<Table> = arr.iter().cloned().collect();
            arr.clear();
            for slot in slots {
                match slot {
                    Slot::Keep(i) => arr.push(src[i].clone()),
                    Slot::Edit(i, j) => {
                        let mut t = src[i].clone();
                        set_entry(&mut t, &items[j]);
                        arr.push(t);
                    }
                    Slot::Insert(j) => {
                        let mut t = Table::new();
                        set_entry(&mut t, &items[j]);
                        arr.push(t);
                    }
                }
            }
            if arr.is_empty() {
                table.remove(key);
            }
        }
        Some(Item::Value(Value::Array(arr))) => {
            let src: Vec<Option<InlineTable>> =
                arr.iter().map(|v| v.as_inline_table().cloned()).collect();
            let old: Vec<KeyValue> = src
                .iter()
                .map(|t| {
                    t.as_ref()
                        .map(|t| lenient(t))
                        .unwrap_or_else(|| lenient(&InlineTable::new()))
                })
                .collect();
            let slots = plan(&old, items);
            if is_identity(&slots, old.len()) {
                return;
            }
            arr.clear();
            for slot in slots {
                match slot {
                    Slot::Keep(i) => {
                        arr.push_formatted(Value::InlineTable(src[i].clone().unwrap_or_default()))
                    }
                    Slot::Edit(i, j) => {
                        let mut t = src[i].clone().unwrap_or_default();
                        set_entry(&mut t, &items[j]);
                        arr.push_formatted(Value::InlineTable(t));
                    }
                    Slot::Insert(j) => {
                        let mut t = InlineTable::new();
                        set_entry(&mut t, &items[j]);
                        arr.push(Value::InlineTable(t));
                    }
                }
            }
        }
        _ => {
            let mut arr = ArrayOfTables::new();
            for kv in items {
                let mut t = Table::new();
                set_entry(&mut t, kv);
                arr.push(t);
            }
            table.insert(key, Item::ArrayOfTables(arr));
        }
    }
}

fn is_identity(slots: &[Slot], old_len: usize) -> bool {
    slots.len() == old_len
        && slots
            .iter()
            .enumerate()
            .all(|(i, s)| matches!(s, Slot::Keep(k) if *k == i))
}

/// Read `[auth]`. An absent table reads as `default`.
pub fn read_auth(table: &Table, default: Auth, path: &Path) -> Result<Auth, StorageError> {
    let Some(item) = table.get("auth") else {
        return Ok(default);
    };
    let t = table_like(item, "auth", path)?;
    let field = |f: &str| -> Result<Option<String>, StorageError> {
        opt_str_in(t, f, &format!("auth.{f}"), path)
    };
    let req = |f: &str| -> Result<String, StorageError> {
        field(f)?.ok_or_else(|| invalid(path, format!("auth.{f}"), "missing"))
    };
    let ty = req("type")?;
    Ok(match ty.as_str() {
        "none" => Auth::None,
        "inherit" => Auth::Inherit,
        "basic" => Auth::Basic {
            username: req("username")?,
            password: field("password")?.unwrap_or_default(),
        },
        "bearer" => Auth::Bearer {
            token: req("token")?,
        },
        "apikey" => Auth::ApiKey {
            name: req("name")?,
            value: req("value")?,
            location: match field("in")?.as_deref() {
                None | Some("header") => ApiKeyLocation::Header,
                Some("query") => ApiKeyLocation::Query,
                Some(other) => {
                    return Err(invalid(
                        path,
                        "auth.in",
                        format!("unknown location `{other}`"),
                    ));
                }
            },
        },
        other => {
            return Err(invalid(
                path,
                "auth.type",
                format!("unknown auth type `{other}`"),
            ));
        }
    })
}

/// Write `[auth]`, editing an existing table in place.
///
/// This always creates an `[auth]` table. Callers skip the call when the value equals
/// that file's default and the file had no `[auth]` table.
pub fn write_auth(table: &mut Table, auth: &Auth) {
    if table.get("auth").and_then(Item::as_table_like).is_none() {
        table.insert("auth", Item::Table(Table::new()));
    }
    let Some(t) = table.get_mut("auth").and_then(Item::as_table_like_mut) else {
        return;
    };
    let (ty, fields): (&str, Vec<(&str, &str)>) = match auth {
        Auth::None => ("none", vec![]),
        Auth::Inherit => ("inherit", vec![]),
        Auth::Basic { username, password } => (
            "basic",
            vec![("username", username), ("password", password)],
        ),
        Auth::Bearer { token } => ("bearer", vec![("token", token)]),
        Auth::ApiKey {
            name,
            value,
            location,
        } => (
            "apikey",
            vec![
                ("name", name),
                ("value", value),
                (
                    "in",
                    match location {
                        ApiKeyLocation::Header => "header",
                        ApiKeyLocation::Query => "query",
                    },
                ),
            ],
        ),
    };
    for f in AUTH_FIELDS {
        if !fields.iter().any(|(k, _)| *k == f) {
            t.remove(f);
        }
    }
    set_str(t, "type", ty);
    for (k, v) in fields {
        // Skip an absent key whose value is the read default.
        let default = (k == "in" && v == "header") || (k == "password" && v.is_empty());
        if default && t.get(k).is_none() {
            continue;
        }
        set_str(t, k, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"# top comment

name = "x"   # trailing

[vars]
# base
base   =   "http://localhost"  # local
port = "8080"


[[headers]]
name = "Accept"
value  = "*/*"

[[headers]]
# second
name = "X-Debug"
value = "1"
enabled = false

[auth]
type = "bearer"   # tok
token = "abc"
"#;

    fn p() -> &'static Path {
        Path::new("t.toml")
    }

    fn load() -> (DocumentMut, Vec<Var>, Vec<KeyValue>, Auth) {
        let doc = parse(p(), FIXTURE).unwrap();
        let vars = read_vars(&doc, "vars", p()).unwrap();
        let hs = read_key_values(&doc, "headers", p()).unwrap();
        let auth = read_auth(&doc, Auth::None, p()).unwrap();
        (doc, vars, hs, auth)
    }

    fn lines_differing(a: &str, b: &str) -> usize {
        assert_eq!(a.lines().count(), b.lines().count());
        a.lines().zip(b.lines()).filter(|(x, y)| x != y).count()
    }

    #[test]
    fn unchanged_roundtrip_is_byte_identical() {
        let (mut doc, vars, hs, auth) = load();
        write_vars(&mut doc, "vars", &vars);
        write_key_values(&mut doc, "headers", &hs);
        write_auth(&mut doc, &auth);
        let name = read_str(&doc, "name", p()).unwrap();
        write_str(&mut doc, "name", &name);
        assert_eq!(doc.to_string(), FIXTURE);
    }

    #[test]
    fn changing_a_var_changes_one_line() {
        let (mut doc, mut vars, ..) = load();
        vars[1].value = "9090".into();
        write_vars(&mut doc, "vars", &vars);
        let out = doc.to_string();
        assert_eq!(lines_differing(FIXTURE, &out), 1);
        assert!(out.contains("port = \"9090\""));
        assert!(out.contains("# local"));
    }

    #[test]
    fn changed_scalar_keeps_decor() {
        let (mut doc, mut vars, ..) = load();
        vars[0].value = "http://x".into();
        write_vars(&mut doc, "vars", &vars);
        assert!(doc.to_string().contains("base   =   \"http://x\"  # local"));
    }

    #[test]
    fn toggling_enabled_changes_only_that_header() {
        let (mut doc, _, mut hs, _) = load();
        hs[0].enabled = false;
        write_key_values(&mut doc, "headers", &hs);
        let expected = FIXTURE.replacen(
            "value  = \"*/*\"\n",
            "value  = \"*/*\"\nenabled = false\n",
            1,
        );
        assert_eq!(doc.to_string(), expected);
    }

    #[test]
    fn toggling_back_removes_enabled() {
        let (mut doc, _, mut hs, _) = load();
        hs[1].enabled = true;
        write_key_values(&mut doc, "headers", &hs);
        let out = doc.to_string();
        assert!(!out.contains("enabled"));
    }

    #[test]
    fn remove_and_append() {
        let (mut doc, mut vars, mut hs, _) = load();
        vars.remove(0);
        vars.push(Var {
            name: "z".into(),
            value: "1".into(),
        });
        hs.remove(0);
        hs.push(KeyValue {
            name: "N".into(),
            value: "V".into(),
            enabled: true,
        });
        write_vars(&mut doc, "vars", &vars);
        write_key_values(&mut doc, "headers", &hs);
        let doc2 = parse(p(), &doc.to_string()).unwrap();
        assert_eq!(read_vars(&doc2, "vars", p()).unwrap(), vars);
        assert_eq!(read_key_values(&doc2, "headers", p()).unwrap(), hs);
    }

    #[test]
    fn empty_absent_stays_absent() {
        let mut doc = parse(p(), "a = \"b\"\n").unwrap();
        write_vars(&mut doc, "vars", &[]);
        write_key_values(&mut doc, "headers", &[]);
        assert_eq!(doc.to_string(), "a = \"b\"\n");
    }

    #[test]
    fn auth_variants_roundtrip() {
        let all = [
            Auth::None,
            Auth::Inherit,
            Auth::Basic {
                username: "u".into(),
                password: "p".into(),
            },
            Auth::Bearer { token: "t".into() },
            Auth::ApiKey {
                name: "k".into(),
                value: "v".into(),
                location: ApiKeyLocation::Header,
            },
            Auth::ApiKey {
                name: "k".into(),
                value: "v".into(),
                location: ApiKeyLocation::Query,
            },
        ];
        for a in &all {
            // Write over every other variant to check stale fields are removed.
            for prev in &all {
                let mut doc = DocumentMut::new();
                write_auth(&mut doc, prev);
                write_auth(&mut doc, a);
                let doc = parse(p(), &doc.to_string()).unwrap();
                assert_eq!(&read_auth(&doc, Auth::None, p()).unwrap(), a);
            }
        }
    }

    #[test]
    fn absent_auth_reads_default() {
        let doc = parse(p(), "").unwrap();
        assert_eq!(read_auth(&doc, Auth::Inherit, p()).unwrap(), Auth::Inherit);
    }

    #[test]
    fn digest_is_invalid() {
        let doc = parse(p(), "[auth]\ntype = \"digest\"\n").unwrap();
        match read_auth(&doc, Auth::None, p()) {
            Err(StorageError::Invalid { key, .. }) => assert_eq!(key, "auth.type"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn non_string_var_is_invalid() {
        let doc = parse(p(), "[vars]\nport = 8080\n").unwrap();
        match read_vars(&doc, "vars", p()) {
            Err(StorageError::Invalid { key, .. }) => assert_eq!(key, "vars.port"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn broken_toml_reports_line() {
        match parse(p(), "a = 1\nb = = 2\n") {
            Err(StorageError::Parse { message, .. }) => {
                assert!(message.contains("line 2"), "{message}")
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    fn kv(n: &str, v: &str) -> KeyValue {
        KeyValue {
            name: n.into(),
            value: v.into(),
            enabled: true,
        }
    }

    const TWO: &str = "[[headers]]\nname = \"A\"\nvalue = \"1\"\n\n[[headers]]\n# second\nname   = \"B\"\nvalue = \"2\"   # why\n";

    fn rewrite(src: &str, items: &[KeyValue]) -> String {
        let mut doc = parse(p(), src).unwrap();
        write_key_values(&mut doc, "headers", items);
        doc.to_string()
    }

    #[test]
    fn list_remove_first_keeps_second_bytes() {
        let out = rewrite(TWO, &[kv("B", "2")]);
        assert_eq!(
            out,
            "\n[[headers]]\n# second\nname   = \"B\"\nvalue = \"2\"   # why\n"
        );
    }

    #[test]
    fn list_edit_value_keeps_comment() {
        let out = rewrite(TWO, &[kv("A", "1"), kv("B", "3")]);
        assert_eq!(lines_differing(TWO, &out), 1);
        assert_eq!(out, TWO.replace("\"2\"", "\"3\""));
    }

    #[test]
    fn list_insert_between_leaves_neighbours() {
        let out = rewrite(TWO, &[kv("A", "1"), kv("N", "9"), kv("B", "2")]);
        let (first, second) = TWO.split_once("\n[[headers]]\n# second").unwrap();
        let second = format!("[[headers]]\n# second{second}");
        assert!(out.starts_with(first));
        assert!(out.ends_with(&second));
        let mid = &out[first.len()..out.len() - second.len()];
        assert!(
            mid.contains("name = \"N\"") && mid.contains("value = \"9\""),
            "{out}"
        );
        let doc = parse(p(), &out).unwrap();
        assert_eq!(
            read_key_values(&doc, "headers", p()).unwrap(),
            vec![kv("A", "1"), kv("N", "9"), kv("B", "2")]
        );
    }

    #[test]
    fn list_duplicate_names_remove_first() {
        let src = "[[headers]]\nname = \"A\"\nvalue = \"1\"\n[[headers]]\n# c\nname = \"A\"\nvalue = \"2\"\n";
        let out = rewrite(src, &[kv("A", "2")]);
        assert_eq!(out, "[[headers]]\n# c\nname = \"A\"\nvalue = \"2\"\n");
    }

    #[test]
    fn explicit_enabled_true_and_empty_present_vars_survive() {
        let src = "a = \"b\"\n\n[vars]\n# add vars here\n\n[[headers]]\nname = \"A\"\nvalue = \"1\"\nenabled = true\n";
        let mut doc = parse(p(), src).unwrap();
        let hs = read_key_values(&doc, "headers", p()).unwrap();
        write_vars(&mut doc, "vars", &[]);
        write_key_values(&mut doc, "headers", &hs);
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn inline_array_edited_in_place() {
        let src = "headers = [{ name = \"A\", value = \"1\" }, { name = \"B\", value = \"2\" }]\n";
        let mut doc = parse(p(), src).unwrap();
        let hs = read_key_values(&doc, "headers", p()).unwrap();
        write_key_values(&mut doc, "headers", &hs);
        assert_eq!(doc.to_string(), src);
        write_key_values(&mut doc, "headers", &[kv("A", "1"), kv("B", "3")]);
        assert!(doc.to_string().starts_with("headers = ["));
        assert!(!doc.to_string().contains("[[headers]]"));
        let doc2 = parse(p(), &doc.to_string()).unwrap();
        assert_eq!(
            read_key_values(&doc2, "headers", p()).unwrap(),
            vec![kv("A", "1"), kv("B", "3")]
        );
    }

    #[test]
    fn auth_defaults_not_added_on_unchanged_save() {
        for src in [
            "[auth]\ntype = \"apikey\"\nname = \"k\"\nvalue = \"v\"\n",
            "[auth]\ntype = \"basic\"\nusername = \"u\"\n",
        ] {
            let mut doc = parse(p(), src).unwrap();
            let a = read_auth(&doc, Auth::None, p()).unwrap();
            write_auth(&mut doc, &a);
            assert_eq!(doc.to_string(), src);
        }
    }
}
