//! Collection tree scanning and ordering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table, Value};
use wisp_core::Method;
use wisp_core::request::{Auth, CollectionDefaults, Environment, FolderDefaults};
use wisp_core::resolve::Scope;

use crate::{STATE_DIR, StorageError, codec, fs};

const COLLECTION_FILE: &str = "collection.toml";
const FOLDER_FILE: &str = "folder.toml";
const ENV_DIR: &str = "environments";
/// Most folders one walk may visit; links can make one folder appear many times.
const MAX_FOLDERS: usize = 10_000;

/// Name and method of one request, read without loading its body.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestMeta {
    pub path: PathBuf,
    pub name: String,
    pub method: Method,
}

/// One entry of the collection tree. Paths are relative to the collection root.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Folder {
        path: PathBuf,
        name: String,
        children: Vec<Node>,
    },
    Request(RequestMeta),
    /// A request file that failed to parse.
    Invalid {
        path: PathBuf,
        error: String,
    },
}

/// An opened collection: defaults plus the ordered tree.
#[derive(Debug, Clone)]
pub struct CollectionTree {
    /// Canonical collection root.
    pub root: PathBuf,
    pub defaults: CollectionDefaults,
    /// Keyed by relative directory path; an entry exists for every folder.
    pub folders: HashMap<PathBuf, FolderDefaults>,
    pub nodes: Vec<Node>,
}

impl CollectionTree {
    /// Build the resolve scope for a request path (relative to the root).
    pub fn scope_for<'a>(&'a self, request: &Path, env: Option<&'a Environment>) -> Scope<'a> {
        let rel = request.strip_prefix(&self.root).unwrap_or(request);
        let mut folders: Vec<&FolderDefaults> = rel
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .filter(|p| !p.as_os_str().is_empty())
            .filter_map(|p| self.folders.get(p))
            .collect();
        folders.reverse();
        Scope {
            collection: &self.defaults,
            folders,
            environment: env,
        }
    }
}

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn invalid(path: &Path, key: &str, message: &str) -> StorageError {
    StorageError::Invalid {
        path: path.to_path_buf(),
        key: key.to_string(),
        message: message.to_string(),
    }
}

fn read_order(table: &Table, path: &Path) -> Result<Vec<String>, StorageError> {
    let Some(item) = table.get("order") else {
        return Ok(Vec::new());
    };
    let arr = item
        .as_array()
        .ok_or_else(|| invalid(path, "order", "expected an array of strings"))?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| invalid(path, "order", "expected an array of strings"))
        })
        .collect()
}

fn write_order(table: &mut Table, order: &[String]) {
    if order.is_empty() && table.get("order").is_none() {
        return;
    }
    let same = table
        .get("order")
        .and_then(Item::as_array)
        .is_some_and(|a| {
            a.len() == order.len() && a.iter().zip(order).all(|(v, s)| v.as_str() == Some(s))
        });
    if same {
        return;
    }
    let mut arr = Array::new();
    for s in order {
        arr.push(s.as_str());
    }
    match table.get_mut("order") {
        Some(item) if item.is_value() => {
            let decor = item.as_value().map(|v| v.decor().clone());
            let mut v = Value::Array(arr);
            if let Some(d) = decor {
                *v.decor_mut() = d;
            }
            *item = Item::Value(v);
        }
        _ => {
            table.insert("order", Item::Value(Value::Array(arr)));
        }
    }
}

fn read_doc(path: &Path) -> Result<Option<DocumentMut>, StorageError> {
    match std::fs::read_to_string(path) {
        Ok(text) => codec::parse(path, &text).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

fn read_format(doc: &DocumentMut, path: &Path) -> Result<u32, StorageError> {
    let n = doc
        .get("format")
        .ok_or_else(|| invalid(path, "format", "missing"))?
        .as_integer()
        .ok_or_else(|| invalid(path, "format", "expected an integer"))?;
    if n != 1 {
        return Err(StorageError::UnsupportedFormat {
            path: path.to_path_buf(),
            found: n,
        });
    }
    Ok(1)
}

/// Load `collection.toml` from the collection root.
pub fn load_collection_defaults(root: &Path) -> Result<CollectionDefaults, StorageError> {
    let path = root.join(COLLECTION_FILE);
    let doc = read_doc(&path)?.ok_or_else(|| StorageError::NotACollection {
        path: root.to_path_buf(),
    })?;
    let t = doc.as_table();
    Ok(CollectionDefaults {
        format: read_format(&doc, &path)?,
        vars: codec::read_vars(t, "vars", &path)?,
        headers: codec::read_key_values(t, "headers", &path)?,
        auth: codec::read_auth(t, Auth::None, &path)?,
        order: read_order(t, &path)?,
    })
}

/// Save `collection.toml`, editing in place. A new file gets `format = 1`.
pub fn save_collection_defaults(
    root: &Path,
    defaults: &CollectionDefaults,
) -> Result<(), StorageError> {
    let path = root.join(COLLECTION_FILE);
    let mut doc = read_doc(&path)?.unwrap_or_default();
    let t = doc.as_table_mut();
    let had_auth = t.get("auth").is_some();
    if t.get("format").and_then(Item::as_integer) != Some(i64::from(defaults.format)) {
        t.insert("format", toml_edit::value(i64::from(defaults.format)));
    }
    codec::write_vars(t, "vars", &defaults.vars);
    codec::write_key_values(t, "headers", &defaults.headers);
    if had_auth || defaults.auth != Auth::None {
        codec::write_auth(t, &defaults.auth);
    }
    write_order(t, &defaults.order);
    fs::atomic_write(&path, doc.to_string().as_bytes())
}

/// Random per-collection ID that keys its per-user data. Always a safe path component.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CollectionId(String);

impl CollectionId {
    /// Accepts `^[A-Za-z0-9_-]{8,64}$` and normalises to lowercase, so ids that
    /// differ only in case are the same id (registry keys and keychain accounts
    /// are case-sensitive; case-insensitive stores would otherwise share them).
    pub fn parse(s: &str) -> Option<CollectionId> {
        let ok = (8..=64).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        ok.then(|| CollectionId(s.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CollectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn id_from_doc(doc: &DocumentMut, path: &Path) -> Result<Option<CollectionId>, StorageError> {
    let Some(item) = doc.get("id") else {
        return Ok(None);
    };
    let s = item
        .as_str()
        .ok_or_else(|| invalid(path, "id", "expected a string"))?;
    CollectionId::parse(s)
        .map(Some)
        .ok_or_else(|| invalid(path, "id", "expected 8 to 64 characters of A-Z a-z 0-9 _ -"))
}

/// Read the top-level `id` of `collection.toml`. A missing file or key is `Ok(None)`.
pub fn read_collection_id(root: &Path) -> Result<Option<CollectionId>, StorageError> {
    let path = root.join(COLLECTION_FILE);
    match read_doc(&path)? {
        Some(doc) => id_from_doc(&doc, &path),
        None => Ok(None),
    }
}

/// Return the collection's ID, writing a new one into `collection.toml` if it has none.
pub fn ensure_collection_id(root: &Path) -> Result<CollectionId, StorageError> {
    let path = root.join(COLLECTION_FILE);
    fs::confined_regular_file(root, &path, "collection.toml")?;
    let mut doc = read_doc(&path)?.unwrap_or_else(|| {
        let mut d = DocumentMut::new();
        d.insert("format", toml_edit::value(1));
        d
    });
    if let Some(id) = id_from_doc(&doc, &path)? {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    doc.insert("id", toml_edit::value(id.as_str()));
    fs::atomic_write(&path, doc.to_string().as_bytes())?;
    Ok(CollectionId(id))
}

/// Give the collection at `root` a fresh random id, replacing any existing one.
/// A copy that shares its original's id uses this to start with no keychain
/// secrets of its own.
pub fn assign_new_collection_id(root: &Path) -> Result<CollectionId, StorageError> {
    let path = root.join(COLLECTION_FILE);
    fs::confined_regular_file(root, &path, "collection.toml")?;
    let mut doc = read_doc(&path)?.unwrap_or_else(|| {
        let mut d = DocumentMut::new();
        d.insert("format", toml_edit::value(1));
        d
    });
    let id = uuid::Uuid::new_v4().simple().to_string();
    doc.insert("id", toml_edit::value(id.as_str()));
    fs::atomic_write(&path, doc.to_string().as_bytes())?;
    Ok(CollectionId(id))
}

/// Load `folder.toml` from a directory. A missing file reads as the default.
pub fn load_folder_defaults(dir: &Path) -> Result<FolderDefaults, StorageError> {
    let path = dir.join(FOLDER_FILE);
    let Some(doc) = read_doc(&path)? else {
        return Ok(FolderDefaults::default());
    };
    let t = doc.as_table();
    Ok(FolderDefaults {
        vars: codec::read_vars(t, "vars", &path)?,
        headers: codec::read_key_values(t, "headers", &path)?,
        auth: codec::read_auth(t, Auth::Inherit, &path)?,
        order: read_order(t, &path)?,
    })
}

/// Save `folder.toml`, editing in place.
pub fn save_folder_defaults(dir: &Path, defaults: &FolderDefaults) -> Result<(), StorageError> {
    let path = dir.join(FOLDER_FILE);
    let mut doc = read_doc(&path)?.unwrap_or_default();
    let t = doc.as_table_mut();
    let had_auth = t.get("auth").is_some();
    codec::write_vars(t, "vars", &defaults.vars);
    codec::write_key_values(t, "headers", &defaults.headers);
    if had_auth || defaults.auth != Auth::Inherit {
        codec::write_auth(t, &defaults.auth);
    }
    write_order(t, &defaults.order);
    fs::atomic_write(&path, doc.to_string().as_bytes())
}

fn read_meta(root: &Path, rel: &Path, stem: &str) -> Result<RequestMeta, StorageError> {
    let abs = root.join(rel);
    let text = fs::read_to_string(&abs)?;
    let doc = codec::parse(&abs, &text)?;
    let name = codec::read_opt_str(doc.as_table(), "name", &abs)?.unwrap_or_else(|| stem.into());
    let m = codec::read_str(doc.as_table(), "method", &abs)?;
    let method = crate::request::parse_method(&m)
        .ok_or_else(|| invalid(&abs, "method", &format!("unknown method `{m}`")))?;
    Ok(RequestMeta {
        path: rel.to_path_buf(),
        name,
        method,
    })
}

/// Scan one directory. `rel` is relative to `root`; `order` lists entry names.
fn scan_dir(
    root: &Path,
    rel: &Path,
    order: &[String],
    folders: &mut HashMap<PathBuf, FolderDefaults>,
    ancestors: &mut Vec<PathBuf>,
    visited: &mut usize,
) -> Result<Vec<Node>, StorageError> {
    let dir = root.join(rel);
    let mut entries: Vec<(String, Node)> = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| io(&dir, e))? {
        let entry = entry.map_err(|e| io(&dir, e))?;
        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };
        if file_name.starts_with('.') {
            continue;
        }
        let child_rel = rel.join(&file_name);
        let abs = entry.path();
        if abs.is_dir() {
            if rel.as_os_str().is_empty() && (file_name == ENV_DIR || file_name == STATE_DIR) {
                continue;
            }
            // Skip a directory that is one of its own ancestors (symlink cycle).
            let canon = std::fs::canonicalize(&abs).map_err(|e| io(&abs, e))?;
            if ancestors.contains(&canon) {
                continue;
            }
            *visited += 1;
            if *visited > MAX_FOLDERS {
                return Err(invalid(
                    root,
                    "collection",
                    &format!(
                        "more than {MAX_FOLDERS} folders (symbolic links may repeat folders); \
                         remove links that fan out and rejoin"
                    ),
                ));
            }
            let defaults = load_folder_defaults(&abs)?;
            ancestors.push(canon);
            let children = scan_dir(
                root,
                &child_rel,
                &defaults.order,
                folders,
                ancestors,
                visited,
            );
            ancestors.pop();
            let children = children?;
            folders.insert(child_rel.clone(), defaults);
            entries.push((
                file_name.clone(),
                Node::Folder {
                    path: child_rel,
                    name: file_name,
                    children,
                },
            ));
            continue;
        }
        let Some(stem) = file_name.strip_suffix(".toml") else {
            continue;
        };
        if file_name == COLLECTION_FILE || file_name == FOLDER_FILE || file_name.contains(".body.")
        {
            continue;
        }
        let node = match read_meta(root, &child_rel, stem) {
            Ok(meta) => Node::Request(meta),
            Err(e) => Node::Invalid {
                path: child_rel,
                error: e.to_string(),
            },
        };
        entries.push((stem.to_string(), node));
    }
    // Deterministic: case-insensitive, then exact name, then folders before requests.
    entries.sort_by_key(|(name, node)| {
        (
            name.to_lowercase(),
            name.clone(),
            !matches!(node, Node::Folder { .. }),
        )
    });
    let mut out = Vec::with_capacity(entries.len());
    for name in order {
        // A request may be listed as `stem` or `stem.toml`; a folder only by name.
        let pos = entries.iter().position(|(n, node)| {
            n == name
                || (!matches!(node, Node::Folder { .. })
                    && name.strip_suffix(".toml") == Some(n.as_str()))
        });
        if let Some(i) = pos {
            out.push(entries.remove(i).1);
        }
    }
    out.extend(entries.into_iter().map(|(_, n)| n));
    Ok(out)
}

/// Open a collection as an ordered tree without loading request bodies.
pub fn open_collection(root: &Path) -> Result<CollectionTree, StorageError> {
    if !root.join(COLLECTION_FILE).is_file() {
        return Err(StorageError::NotACollection {
            path: root.to_path_buf(),
        });
    }
    let root = std::fs::canonicalize(root).map_err(|e| io(root, e))?;
    let defaults = load_collection_defaults(&root)?;
    let mut folders = HashMap::new();
    let mut ancestors = vec![root.clone()];
    let nodes = scan_dir(
        &root,
        Path::new(""),
        &defaults.order,
        &mut folders,
        &mut ancestors,
        &mut 0,
    )?;
    Ok(CollectionTree {
        root,
        defaults,
        folders,
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        stdfs::create_dir_all(p.parent().unwrap()).unwrap();
        stdfs::write(p, text).unwrap();
    }

    fn req(name: &str, method: &str) -> String {
        format!("name = \"{name}\"\nmethod = \"{method}\"\nurl = \"http://x\"\n")
    }

    fn layout() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        w(r, "environments/dev.toml", "[vars]\n");
        w(r, ".wisp/history.jsonl", "");
        w(r, "users/folder.toml", "order = [\"create-user\"]\n");
        w(r, "users/get-user.toml", &req("Get user", "GET"));
        w(r, "users/create-user.toml", &req("Create user", "POST"));
        w(r, "users/create-user.body.json", "{}");
        d
    }

    fn names(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Folder { name, .. } => name.clone(),
                Node::Request(m) => m.name.clone(),
                Node::Invalid { path, .. } => path.display().to_string(),
            })
            .collect()
    }

    #[test]
    fn order_listed_first() {
        let d = layout();
        let t = open_collection(d.path()).unwrap();
        assert_eq!(t.nodes.len(), 1);
        let Node::Folder { children, .. } = &t.nodes[0] else {
            panic!()
        };
        assert_eq!(names(children), ["Create user", "Get user"]);
        assert!(t.folders.contains_key(Path::new("users")));
    }

    #[test]
    fn order_accepts_stem_and_toml_forms_in_one_list() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(
            r,
            "collection.toml",
            "format = 1\norder = [\"c.toml\", \"a\", \"b.toml\"]\n",
        );
        for n in ["a", "b", "c", "d"] {
            w(r, &format!("{n}.toml"), &req(n, "GET"));
        }
        let t = open_collection(r).unwrap();
        assert_eq!(names(&t.nodes), ["c", "a", "b", "d"]);
    }

    #[test]
    fn unlisted_sorted_case_insensitive_and_missing_ignored() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(
            r,
            "collection.toml",
            "format = 1\norder = [\"zed\", \"nope\"]\n",
        );
        w(r, "Beta.toml", &req("Beta", "GET"));
        w(r, "alpha.toml", &req("alpha", "GET"));
        w(r, "zed.toml", &req("zed", "PUT"));
        let t = open_collection(r).unwrap();
        assert_eq!(names(&t.nodes), ["zed", "alpha", "Beta"]);
    }

    #[cfg(unix)]
    #[test]
    fn open_does_not_read_sidecar() {
        use std::os::unix::fs::PermissionsExt;
        let d = layout();
        let side = d.path().join("users/create-user.body.json");
        stdfs::set_permissions(&side, stdfs::Permissions::from_mode(0o000)).unwrap();
        let res = open_collection(d.path());
        stdfs::set_permissions(&side, stdfs::Permissions::from_mode(0o644)).unwrap();
        let t = res.unwrap();
        let Node::Folder { children, .. } = &t.nodes[0] else {
            panic!()
        };
        assert!(children.iter().any(
            |n| matches!(n, Node::Request(m) if m.name == "Create user" && m.method == Method::Post)
        ));
        assert!(!children.iter().any(|n| matches!(n, Node::Invalid { .. })));
    }

    fn open_bounded(root: &Path) -> CollectionTree {
        let root = root.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(open_collection(&root));
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("open_collection did not return")
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_single_link() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        w(r, "a/r.toml", &req("r", "GET"));
        std::os::unix::fs::symlink("..", r.join("a/up")).unwrap();
        let t = open_bounded(r);
        assert_eq!(t.folders.len(), 1);
        assert!(t.folders.contains_key(Path::new("a")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_double_link() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        w(r, "a/r.toml", &req("r", "GET"));
        std::os::unix::fs::symlink("..", r.join("a/up")).unwrap();
        std::os::unix::fs::symlink("..", r.join("a/up2")).unwrap();
        let t = open_bounded(r);
        assert_eq!(t.folders.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_diamond_fails_with_folder_budget_error() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        for i in 0..15 {
            stdfs::create_dir_all(r.join(format!("l{i}"))).unwrap();
            for n in ["a", "b"] {
                std::os::unix::fs::symlink(format!("../l{}", i + 1), r.join(format!("l{i}/{n}")))
                    .unwrap();
            }
        }
        stdfs::create_dir_all(r.join("l15")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let p = r.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(open_collection(&p));
        });
        let err = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("open_collection did not return")
            .unwrap_err();
        assert!(err.to_string().contains(&MAX_FOLDERS.to_string()));
    }

    #[test]
    fn tiebreak_is_deterministic() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        w(r, "x/r.toml", &req("r", "GET"));
        w(r, "x.toml", &req("x", "GET"));
        let t = open_collection(r).unwrap();
        assert_eq!(names(&t.nodes), ["x", "x"]);
        assert!(matches!(t.nodes[0], Node::Folder { .. }));
    }

    #[test]
    fn broken_request_is_invalid_node() {
        let d = layout();
        w(d.path(), "users/bad.toml", "name = [\n");
        let t = open_collection(d.path()).unwrap();
        let Node::Folder { children, .. } = &t.nodes[0] else {
            panic!()
        };
        assert_eq!(children.len(), 3);
        assert!(children.iter().any(|n| matches!(n, Node::Invalid { .. })));
        assert_eq!(
            children
                .iter()
                .filter(|n| matches!(n, Node::Request(_)))
                .count(),
            2
        );
    }

    #[test]
    fn format_and_not_a_collection() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(
            open_collection(d.path()),
            Err(StorageError::NotACollection { .. })
        ));
        w(d.path(), "collection.toml", "format = 2\n");
        assert!(matches!(
            open_collection(d.path()),
            Err(StorageError::UnsupportedFormat { found: 2, .. })
        ));
    }

    #[test]
    fn skipped_entries_are_not_nodes() {
        let d = layout();
        let t = open_collection(d.path()).unwrap();
        assert_eq!(names(&t.nodes), ["users"]);
        let Node::Folder { children, .. } = &t.nodes[0] else {
            panic!()
        };
        assert!(
            children
                .iter()
                .all(|n| matches!(n, Node::Request(m) if m.path.starts_with("users")))
        );
    }

    #[test]
    fn scope_for_orders_folders_outermost_first() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        w(r, "a/folder.toml", "[vars]\nx = \"a\"\n");
        w(r, "a/b/folder.toml", "[vars]\nx = \"b\"\n");
        w(r, "a/b/req.toml", &req("r", "GET"));
        let t = open_collection(r).unwrap();
        let s = t.scope_for(Path::new("a/b/req.toml"), None);
        let xs: Vec<&str> = s.folders.iter().map(|f| f.vars[0].value.as_str()).collect();
        assert_eq!(xs, ["a", "b"]);
        assert!(t.scope_for(Path::new("req.toml"), None).folders.is_empty());
    }

    #[test]
    fn save_folder_changes_only_that_header() {
        let d = tempfile::tempdir().unwrap();
        let text = "# keep me\norder = [\"a\"]   # trailing\n\n[[headers]]\n# first\nname = \"A\"\nvalue = \"1\"\n\n[[headers]]\nname = \"B\"   # b\nvalue = \"2\"\n\n[vars]\n# v\nk = \"v\"\n";
        w(d.path(), "folder.toml", text);
        let mut f = load_folder_defaults(d.path()).unwrap();
        f.headers[0].value = "9".into();
        save_folder_defaults(d.path(), &f).unwrap();
        let out = stdfs::read_to_string(d.path().join("folder.toml")).unwrap();
        assert_eq!(out, text.replace("value = \"1\"", "value = \"9\""));
    }

    #[test]
    fn unchanged_save_adds_no_auth() {
        let d = tempfile::tempdir().unwrap();
        let c = "# c\nformat = 1\n\n[vars]\na = \"b\"  # x\n";
        let f = "# f\n[vars]\na = \"b\"\n";
        w(d.path(), "collection.toml", c);
        w(d.path(), "folder.toml", f);
        save_collection_defaults(d.path(), &load_collection_defaults(d.path()).unwrap()).unwrap();
        save_folder_defaults(d.path(), &load_folder_defaults(d.path()).unwrap()).unwrap();
        assert_eq!(
            stdfs::read_to_string(d.path().join("collection.toml")).unwrap(),
            c
        );
        assert_eq!(
            stdfs::read_to_string(d.path().join("folder.toml")).unwrap(),
            f
        );
    }

    #[test]
    fn new_collection_gets_format_and_existing_auth_kept() {
        let d = tempfile::tempdir().unwrap();
        save_collection_defaults(d.path(), &CollectionDefaults::default()).unwrap();
        let out = stdfs::read_to_string(d.path().join("collection.toml")).unwrap();
        assert_eq!(out, "format = 1\n");
        w(d.path(), "folder.toml", "[auth]\ntype = \"inherit\"\n");
        save_folder_defaults(d.path(), &FolderDefaults::default()).unwrap();
        let out = stdfs::read_to_string(d.path().join("folder.toml")).unwrap();
        assert!(out.contains("[auth]"));
    }

    #[test]
    fn lowercase_method_loads_through_tree_and_load_request() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "collection.toml", "format = 1\n");
        w(d.path(), "r.toml", &req("r", "post"));
        let t = open_collection(d.path()).unwrap();
        assert!(matches!(&t.nodes[0], Node::Request(m) if m.method == Method::Post));
        let r = crate::request::load_request(&d.path().join("r.toml")).unwrap();
        assert_eq!(r.method, Method::Post);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_sibling_folder_shows_under_both_names() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "collection.toml", "format = 1\n");
        w(r, "a/r.toml", &req("r", "GET"));
        std::os::unix::fs::symlink("a", r.join("b")).unwrap();
        let t = open_collection(r).unwrap();
        assert!(t.folders.contains_key(Path::new("a")));
        assert!(t.folders.contains_key(Path::new("b")));
        assert_eq!(t.nodes.len(), 2);
    }

    #[test]
    fn collection_id_added_once_keeping_comments() {
        let d = tempfile::tempdir().unwrap();
        let text = "# my collection\nformat = 1 # v\n\n[vars]\na = \"b\"\n";
        w(d.path(), "collection.toml", text);
        let id = ensure_collection_id(d.path()).unwrap();
        let after = stdfs::read_to_string(d.path().join("collection.toml")).unwrap();
        assert_eq!(after.matches("id = ").count(), 1);
        assert!(after.contains("# my collection\nformat = 1 # v"));
        assert!(after.contains("[vars]\na = \"b\""));
        assert_eq!(id.as_str().len(), 32);
        let m = stdfs::metadata(d.path().join("collection.toml"))
            .unwrap()
            .modified()
            .unwrap();
        let id2 = ensure_collection_id(d.path()).unwrap();
        assert_eq!(id, id2);
        assert_eq!(
            stdfs::read_to_string(d.path().join("collection.toml")).unwrap(),
            after
        );
        assert_eq!(
            stdfs::metadata(d.path().join("collection.toml"))
                .unwrap()
                .modified()
                .unwrap(),
            m
        );
    }

    #[test]
    fn collection_id_read_cases() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_collection_id(d.path()).unwrap(), None);
        w(d.path(), "collection.toml", "format = 1\n");
        assert_eq!(read_collection_id(d.path()).unwrap(), None);
        w(
            d.path(),
            "collection.toml",
            "format = 1\nid = \"abcd_efgh-12\"\n",
        );
        assert_eq!(
            read_collection_id(d.path()).unwrap().unwrap().as_str(),
            "abcd_efgh-12"
        );
        for bad in ["id = 5", "id = \"../x\"", "id = \"short\""] {
            w(d.path(), "collection.toml", &format!("format = 1\n{bad}\n"));
            assert!(
                matches!(
                    read_collection_id(d.path()),
                    Err(StorageError::Invalid { ref key, .. }) if key == "id"
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn collection_id_survives_save_and_load() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "collection.toml", "format = 1\n");
        let id = ensure_collection_id(d.path()).unwrap();
        let mut c = load_collection_defaults(d.path()).unwrap();
        c.vars.push(wisp_core::request::Var {
            name: "k".into(),
            value: "v".into(),
        });
        save_collection_defaults(d.path(), &c).unwrap();
        assert_eq!(read_collection_id(d.path()).unwrap(), Some(id));
        assert_eq!(load_collection_defaults(d.path()).unwrap().vars.len(), 1);
    }

    #[test]
    fn collection_id_creates_missing_file() {
        let d = tempfile::tempdir().unwrap();
        let id = ensure_collection_id(d.path()).unwrap();
        let text = stdfs::read_to_string(d.path().join("collection.toml")).unwrap();
        assert_eq!(text, format!("format = 1\nid = \"{id}\"\n"));
    }

    #[test]
    fn collection_id_parse_bounds() {
        assert!(CollectionId::parse("1234567").is_none());
        assert!(CollectionId::parse("12345678").is_some());
        assert!(CollectionId::parse(&"a".repeat(64)).is_some());
        assert!(CollectionId::parse(&"a".repeat(65)).is_none());
        assert!(CollectionId::parse("abcd efgh").is_none());
    }

    #[test]
    fn collection_id_parse_lowercases() {
        let a = CollectionId::parse("AbCdEf0123456789").unwrap();
        assert_eq!(a.as_str(), "abcdef0123456789");
        assert_eq!(a, CollectionId::parse("abcdef0123456789").unwrap());
    }

    #[test]
    fn assign_new_id_replaces_and_keeps_other_keys() {
        let d = tempfile::tempdir().unwrap();
        stdfs::write(
            d.path().join("collection.toml"),
            "format = 1\nid = \"abcdef0123456789\"\nname = \"x\"\n",
        )
        .unwrap();
        let new = assign_new_collection_id(d.path()).unwrap();
        assert_ne!(new.as_str(), "abcdef0123456789");
        assert_eq!(read_collection_id(d.path()).unwrap(), Some(new));
        let text = stdfs::read_to_string(d.path().join("collection.toml")).unwrap();
        assert!(text.contains("name = \"x\""));
    }

    #[test]
    fn collection_id_display_matches_as_str() {
        let id = CollectionId::parse("abcd_efgh").unwrap();
        assert_eq!(id.to_string(), id.as_str());
    }

    #[test]
    fn collection_id_generated_is_lowercase_hex() {
        let d = tempfile::tempdir().unwrap();
        let id = ensure_collection_id(d.path()).unwrap();
        assert!(
            id.as_str()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    #[test]
    fn collection_id_keeps_existing_value() {
        let d = tempfile::tempdir().unwrap();
        w(
            d.path(),
            "collection.toml",
            "format = 1\nid = \"keep_this_id\"\n",
        );
        assert_eq!(
            ensure_collection_id(d.path()).unwrap().as_str(),
            "keep_this_id"
        );
    }

    #[test]
    fn collection_id_invalid_existing_is_not_replaced() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "collection.toml", "format = 1\nid = 5\n");
        assert!(ensure_collection_id(d.path()).is_err());
        assert_eq!(
            stdfs::read_to_string(d.path().join("collection.toml")).unwrap(),
            "format = 1\nid = 5\n"
        );
    }
}
