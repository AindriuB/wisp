//! TOML codec for request, folder, collection and environment files.
//!
//! Reads and writes go through `toml_edit` so comments and formatting survive.

use std::path::Path;

use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike, Value};
use wisp_core::assert::{Assertion, Expected, Op, ParseError};
use wisp_core::request::{ApiKeyLocation, Auth, ClientAuth, KeyValue, Var};

use crate::StorageError;

const AUTH_FIELDS: [&str; 18] = [
    "username",
    "password",
    "token",
    "name",
    "value",
    "in",
    "access_key",
    "secret_key",
    "session_token",
    "region",
    "service",
    "auth_url",
    "token_url",
    "client_id",
    "client_secret",
    "scope",
    "client_auth",
    "redirect_port",
];

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
        "digest" => Auth::Digest {
            username: req("username")?,
            password: field("password")?.unwrap_or_default(),
        },
        "awssigv4" => Auth::AwsSigV4 {
            access_key: req("access_key")?,
            secret_key: req("secret_key")?,
            session_token: field("session_token")?.unwrap_or_default(),
            region: req("region")?,
            service: req("service")?,
        },
        "oauth2_client_credentials" => Auth::OAuth2ClientCredentials {
            token_url: req("token_url")?,
            client_id: req("client_id")?,
            client_secret: field("client_secret")?.unwrap_or_default(),
            scope: field("scope")?.unwrap_or_default(),
            client_auth: read_client_auth(&field("client_auth")?, path)?,
        },
        "oauth2_auth_code" => Auth::OAuth2AuthCode {
            auth_url: req("auth_url")?,
            token_url: req("token_url")?,
            client_id: req("client_id")?,
            client_secret: field("client_secret")?.unwrap_or_default(),
            scope: field("scope")?.unwrap_or_default(),
            client_auth: read_client_auth(&field("client_auth")?, path)?,
            redirect_port: match t.get("redirect_port") {
                None => 0,
                Some(item) => item
                    .as_integer()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| {
                        invalid(path, "auth.redirect_port", "expected a port number 0-65535")
                    })?,
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

fn read_client_auth(v: &Option<String>, path: &Path) -> Result<ClientAuth, StorageError> {
    match v.as_deref() {
        None | Some("basic") => Ok(ClientAuth::Basic),
        Some("body") => Ok(ClientAuth::Body),
        Some(other) => Err(invalid(
            path,
            "auth.client_auth",
            format!("unknown client authentication `{other}`"),
        )),
    }
}

fn client_auth_str(c: ClientAuth) -> &'static str {
    match c {
        ClientAuth::Basic => "basic",
        ClientAuth::Body => "body",
    }
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
        Auth::Digest { username, password } => (
            "digest",
            vec![("username", username), ("password", password)],
        ),
        Auth::AwsSigV4 {
            access_key,
            secret_key,
            session_token,
            region,
            service,
        } => (
            "awssigv4",
            vec![
                ("access_key", access_key),
                ("secret_key", secret_key),
                ("session_token", session_token),
                ("region", region),
                ("service", service),
            ],
        ),
        Auth::OAuth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
            client_auth,
        } => (
            "oauth2_client_credentials",
            vec![
                ("token_url", token_url),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("scope", scope),
                ("client_auth", client_auth_str(*client_auth)),
            ],
        ),
        Auth::OAuth2AuthCode {
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            client_auth,
            ..
        } => (
            "oauth2_auth_code",
            vec![
                ("auth_url", auth_url),
                ("token_url", token_url),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("scope", scope),
                ("client_auth", client_auth_str(*client_auth)),
            ],
        ),
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
    let keeps_port = matches!(auth, Auth::OAuth2AuthCode { .. });
    for f in AUTH_FIELDS {
        if keeps_port && f == "redirect_port" {
            continue;
        }
        if !fields.iter().any(|(k, _)| *k == f) {
            t.remove(f);
        }
    }
    set_str(t, "type", ty);
    for (k, v) in fields {
        // Skip an absent key whose value is the read default.
        let default = (k == "in" && v == "header")
            || (k == "password" && v.is_empty())
            || (k == "session_token" && v.is_empty())
            || (k == "client_secret" && v.is_empty())
            || (k == "scope" && v.is_empty())
            || (k == "client_auth" && v == "basic");
        if default && t.get(k).is_none() {
            continue;
        }
        set_str(t, k, v);
    }
    if let Auth::OAuth2AuthCode { redirect_port, .. } = auth {
        let port = i64::from(*redirect_port);
        if t.get("redirect_port").and_then(Item::as_integer) != Some(port)
            && (port != 0 || t.get("redirect_port").is_some())
        {
            match t.get_mut("redirect_port") {
                Some(item) => {
                    let mut new = Value::from(port);
                    if let Some(old) = item.as_value() {
                        *new.decor_mut() = old.decor().clone();
                    }
                    *item = Item::Value(new);
                }
                None => {
                    t.insert("redirect_port", Item::Value(Value::from(port)));
                }
            }
        }
    }
}

const ASSERT_KEY: &str = "assert";

/// The `[[assert]]` tables of a document; `None` when the key is absent or not an array of
/// tables (such a value is neither read nor touched on save).
fn assert_tables(doc: &Table) -> Option<&ArrayOfTables> {
    doc.get(ASSERT_KEY).and_then(Item::as_array_of_tables)
}

fn raw_text(t: &Table) -> String {
    let mut aot = ArrayOfTables::new();
    aot.push(t.clone());
    let mut d = DocumentMut::new();
    d.insert(ASSERT_KEY, Item::ArrayOfTables(aot));
    d.to_string()
}

fn expected_of(v: &Value) -> Option<Expected> {
    match v {
        Value::String(s) => Some(Expected::Str(s.value().clone())),
        Value::Integer(i) => Some(Expected::Int(*i.value())),
        Value::Float(f) => Some(Expected::Float(*f.value())),
        Value::Boolean(b) => Some(Expected::Bool(*b.value())),
        _ => None,
    }
}

fn read_one_assertion(t: &Table) -> Result<Assertion, ParseError> {
    let text = |key: &str| -> Result<Option<&str>, ParseError> {
        match t.get(key) {
            None => Ok(None),
            Some(i) => i.as_str().map(Some).ok_or(ParseError::Malformed),
        }
    };
    let kind = text("kind")?.ok_or(ParseError::Malformed)?;
    let op = text("op")?.ok_or(ParseError::Malformed)?;
    let target = match kind {
        "header" => text("name")?,
        "json" => text("path")?,
        _ => None,
    };
    // Keys of another kind make the entry malformed; any key outside the format makes it
    // one a newer version wrote, which is kept but never evaluated.
    let mut unknown = false;
    for (key, _) in t.iter() {
        match key {
            "kind" | "op" | "value" => {}
            "name" if kind == "header" => {}
            "path" if kind == "json" => {}
            "name" | "path" if matches!(kind, "status" | "header" | "json") => {
                return Err(ParseError::Malformed);
            }
            _ => unknown = true,
        }
    }
    let value = match t.get("value") {
        None => None,
        Some(i) => Some(
            i.as_value()
                .and_then(expected_of)
                .ok_or(ParseError::Malformed)?,
        ),
    };
    let a = Assertion::from_parts(kind, target, op, value)?;
    if unknown {
        return Err(ParseError::NotSupported);
    }
    Ok(a)
}

/// Equality that also holds for a NaN value, so an unchanged entry is recognised.
fn same(a: &Assertion, b: &Assertion) -> bool {
    a == b || format!("{a:?}") == format!("{b:?}")
}

fn table_positions(t: &Table, out: &mut Vec<usize>) {
    out.extend(t.position());
    for (_, item) in t.iter() {
        match item {
            Item::Table(c) => table_positions(c, out),
            Item::ArrayOfTables(a) => a.iter().for_each(|c| table_positions(c, out)),
            _ => {}
        }
    }
}

fn set_positions(t: &mut Table, next: &mut impl FnMut() -> usize) {
    t.set_position(next());
    for (_, item) in t.iter_mut() {
        match item {
            Item::Table(c) => set_positions(c, next),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(|c| set_positions(c, next)),
            _ => {}
        }
    }
}

fn read_assertion(t: &Table) -> Assertion {
    match read_one_assertion(t) {
        Ok(a) => a,
        Err(e) => Assertion::Unrecognised {
            raw: raw_text(t),
            reason: e.reason().to_string(),
        },
    }
}

/// Read the `[[assert]]` array; an unknown or malformed entry becomes `Unrecognised`.
pub fn read_assertions(doc: &Table) -> Vec<Assertion> {
    assert_tables(doc)
        .map(|a| a.iter().map(read_assertion).collect())
        .unwrap_or_default()
}

fn assertion_table(a: &Assertion, path: &Path) -> Result<Table, StorageError> {
    let mut t = Table::new();
    match a {
        Assertion::Unrecognised { raw, .. } => {
            let doc = parse(path, raw)?;
            return assert_tables(&doc)
                .and_then(|a| a.get(0))
                .cloned()
                .ok_or_else(|| invalid(path, ASSERT_KEY, "unrecognised entry has no table"));
        }
        Assertion::Status { op, expected } => {
            t.insert("kind", Item::Value("status".into()));
            put_op(&mut t, *op, expected);
        }
        Assertion::Header { name, op, expected } => {
            t.insert("kind", Item::Value("header".into()));
            t.insert("name", Item::Value(name.as_str().into()));
            put_op(&mut t, *op, expected);
        }
        Assertion::Json { path, op, expected } => {
            t.insert("kind", Item::Value("json".into()));
            t.insert("path", Item::Value(path.as_str().into()));
            put_op(&mut t, *op, expected);
        }
    }
    Ok(t)
}

fn put_op(t: &mut Table, op: Op, expected: &Expected) {
    t.insert("op", Item::Value(op.as_str().into()));
    if op == Op::Exists && *expected == Expected::Bool(true) {
        return;
    }
    let v = match expected {
        Expected::Str(s) => Value::from(s.as_str()),
        Expected::Int(n) => Value::from(*n),
        Expected::Float(f) => Value::from(*f),
        Expected::Bool(b) => Value::from(*b),
    };
    t.insert("value", Item::Value(v));
}

/// Write the `[[assert]]` array. Writes only when `assertions` is non-empty or the file
/// already has the array; an unchanged entry keeps its table (comments and spacing) and an
/// `Unrecognised` entry is written back from its raw text. A saved empty list removes the
/// array.
pub fn write_assertions(
    doc: &mut Table,
    assertions: &[Assertion],
    path: &Path,
) -> Result<(), StorageError> {
    if doc.get(ASSERT_KEY).is_some() && assert_tables(doc).is_none() {
        // Not an array of tables: it reads as empty and is never replaced.
        if assertions.is_empty() {
            return Ok(());
        }
        return Err(invalid(
            path,
            ASSERT_KEY,
            "`assert` is not an array of tables; remove it to add assertions",
        ));
    }
    let old: Vec<(Assertion, Table)> = assert_tables(doc)
        .map(|a| a.iter().map(|t| (read_assertion(t), t.clone())).collect())
        .unwrap_or_default();
    if old.len() == assertions.len() && old.iter().zip(assertions).all(|((o, _), a)| same(o, a)) {
        return Ok(());
    }
    if assertions.is_empty() {
        doc.remove(ASSERT_KEY);
        return Ok(());
    }
    // Tables print in `position` order, so the positions the array already holds (child
    // tables included) are handed out afresh, in order, to every table written. Any
    // surplus continues from the last one, which keeps the array ahead of later tables.
    let mut slots = Vec::new();
    for (_, t) in &old {
        table_positions(t, &mut slots);
    }
    if slots.is_empty() {
        let mut all = Vec::new();
        table_positions(doc, &mut all);
        slots.push(all.into_iter().max().unwrap_or(0));
    }
    slots.sort_unstable();
    let mut taken = 0;
    let mut next = || {
        let p = slots[taken.min(slots.len() - 1)];
        taken += 1;
        p
    };
    let mut used = vec![false; old.len()];
    let mut out = ArrayOfTables::new();
    for a in assertions {
        let reuse = old
            .iter()
            .enumerate()
            .find(|(i, (o, _))| !used[*i] && same(o, a))
            .map(|(i, _)| i);
        let mut table = match reuse {
            Some(i) => {
                used[i] = true;
                old[i].1.clone()
            }
            None => assertion_table(a, path)?,
        };
        set_positions(&mut table, &mut next);
        out.push(table);
    }
    doc.insert(ASSERT_KEY, Item::ArrayOfTables(out));
    Ok(())
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

    fn auth_code_roundtrip(port_lines: &str, head: &str) {
        let text = format!(
            "[auth]\ntype = \"oauth2_auth_code\"\n{head}auth_url = \"https://i/a\"\ntoken_url = \"https://i/t\"\nclient_id = \"c\"\n{port_lines}"
        );
        let mut doc = parse(p(), &text).unwrap();
        let auth = read_auth(&doc, Auth::None, p()).unwrap();
        write_auth(&mut doc, &auth);
        assert_eq!(doc.to_string(), text);
    }

    #[test]
    fn auth_code_redirect_port_comment_kept() {
        auth_code_roundtrip("redirect_port = 8123 # fixed\n", "");
    }

    #[test]
    fn auth_code_redirect_port_position_kept() {
        auth_code_roundtrip("", "redirect_port = 8123\n");
    }

    #[test]
    fn auth_code_redirect_port_zero_kept() {
        auth_code_roundtrip("redirect_port = 0\n", "");
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
            Auth::Digest {
                username: "u".into(),
                password: "p".into(),
            },
            Auth::AwsSigV4 {
                access_key: "ak".into(),
                secret_key: "sk".into(),
                session_token: "st".into(),
                region: "us-east-1".into(),
                service: "s3".into(),
            },
            Auth::AwsSigV4 {
                access_key: "ak".into(),
                secret_key: "sk".into(),
                session_token: String::new(),
                region: "eu-west-1".into(),
                service: "execute-api".into(),
            },
            Auth::OAuth2ClientCredentials {
                token_url: "https://idp.test/token".into(),
                client_id: "cid".into(),
                client_secret: "cs".into(),
                scope: "read write".into(),
                client_auth: ClientAuth::Body,
            },
            Auth::OAuth2ClientCredentials {
                token_url: "https://idp.test/token".into(),
                client_id: "cid".into(),
                client_secret: String::new(),
                scope: String::new(),
                client_auth: ClientAuth::Basic,
            },
            Auth::OAuth2AuthCode {
                auth_url: "https://idp.test/auth".into(),
                token_url: "https://idp.test/token".into(),
                client_id: "cid".into(),
                client_secret: "cs".into(),
                scope: "read".into(),
                client_auth: ClientAuth::Body,
                redirect_port: 8123,
            },
            Auth::OAuth2AuthCode {
                auth_url: "{{a}}".into(),
                token_url: "{{t}}".into(),
                client_id: "{{c}}".into(),
                client_secret: String::new(),
                scope: String::new(),
                client_auth: ClientAuth::Basic,
                redirect_port: 0,
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
    fn oauth2_auth_roundtrips_byte_for_byte_and_keeps_comments() {
        let text = "# top\n[auth]\ntype = \"oauth2_auth_code\" # grant\nauth_url = \"https://i/a\"\ntoken_url = \"https://i/t\"\nclient_id = \"c\"\nclient_secret = \"{{s}}\" # secret\nscope = \"r\"\nclient_auth = \"body\"\nredirect_port = 8123\n";
        let mut doc = parse(p(), text).unwrap();
        let a = read_auth(&doc, Auth::None, p()).unwrap();
        write_auth(&mut doc, &a);
        assert_eq!(doc.to_string(), text);
        let text = "[auth]\ntype = \"oauth2_client_credentials\"\ntoken_url = \"t\"\nclient_id = \"c\" # id\nclient_auth = \"body\"\n";
        let mut doc = parse(p(), text).unwrap();
        let a = read_auth(&doc, Auth::None, p()).unwrap();
        write_auth(&mut doc, &a);
        assert_eq!(doc.to_string(), text);
    }

    #[test]
    fn unknown_oauth2_client_auth_is_invalid_naming_the_key() {
        for ty in ["oauth2_client_credentials", "oauth2_auth_code"] {
            let text = format!(
                "[auth]\ntype = \"{ty}\"\nauth_url = \"a\"\ntoken_url = \"t\"\nclient_id = \"c\"\nclient_auth = \"mtls\"\n"
            );
            let doc = parse(p(), &text).unwrap();
            match read_auth(&doc, Auth::None, p()) {
                Err(StorageError::Invalid { key, .. }) => assert_eq!(key, "auth.client_auth"),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn absent_auth_reads_default() {
        let doc = parse(p(), "").unwrap();
        assert_eq!(read_auth(&doc, Auth::Inherit, p()).unwrap(), Auth::Inherit);
    }

    #[test]
    fn unknown_auth_type_is_invalid() {
        let doc = parse(p(), "[auth]\ntype = \"ntlm\"\n").unwrap();
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
