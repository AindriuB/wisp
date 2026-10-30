//! Collection tree scanning and ordering.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table, Value};
use wisp_core::Method;
use wisp_core::request::{Auth, CollectionDefaults, Environment, FolderDefaults};
use wisp_core::resolve::Scope;

use crate::{STATE_DIR, StorageError, codec, fs};

const COLLECTION_FILE: &str = "collection.toml";
const FOLDER_FILE: &str = "folder.toml";
const ENV_DIR: &str = "environments";

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

fn parse_method(s: &str) -> Option<Method> {
    Some(match s.to_ascii_uppercase().as_str() {
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

fn read_meta(root: &Path, rel: &Path, stem: &str) -> Result<RequestMeta, StorageError> {
    let abs = root.join(rel);
    let text = fs::read_to_string(&abs)?;
    let doc = codec::parse(&abs, &text)?;
    let name = codec::read_opt_str(doc.as_table(), "name", &abs)?.unwrap_or_else(|| stem.into());
    let m = codec::read_str(doc.as_table(), "method", &abs)?;
    let method = parse_method(&m)
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
    visited: &mut HashSet<PathBuf>,
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
            // Skip any directory already visited (symlink cycles, repeat links).
            let canon = std::fs::canonicalize(&abs).map_err(|e| io(&abs, e))?;
            if !visited.insert(canon) {
                continue;
            }
            let defaults = load_folder_defaults(&abs)?;
            let children = scan_dir(root, &child_rel, &defaults.order, folders, visited)?;
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
        if let Some(i) = entries.iter().position(|(n, _)| n == name) {
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
    let mut visited = HashSet::from([root.clone()]);
    let nodes = scan_dir(
        &root,
        Path::new(""),
        &defaults.order,
        &mut folders,
        &mut visited,
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
}
