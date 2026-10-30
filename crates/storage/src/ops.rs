//! File operations behind the sidebar: create, rename, move and delete.
//!
//! All paths are relative to the collection root. Nothing here overwrites an
//! existing file or directory, and nothing leaves the root.

use std::path::{Component, Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Value};

use crate::{STATE_DIR, StorageError, codec, fs};

type Result<T> = std::result::Result<T, StorageError>;

const COLLECTION_FILE: &str = "collection.toml";
const FOLDER_FILE: &str = "folder.toml";
const ENV_DIR: &str = "environments";

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::Invalid {
        path: path.to_path_buf(),
        key: "path".to_string(),
        message: message.into(),
    }
}

fn exists_err(path: &Path) -> StorageError {
    StorageError::Exists {
        path: path.to_path_buf(),
    }
}

/// Turn a display name into a file-system friendly name: lowercase
/// alphanumerics joined by single `-`. A name with none becomes `untitled`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for c in name.chars() {
        if c.is_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('-');
            }
            pending = false;
            out.extend(c.to_lowercase());
        } else {
            pending = true;
        }
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

/// Canonical root.
fn canon_root(root: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(root).map_err(|e| io(root, e))
}

/// Validate a relative path and return it without `.` components. An empty
/// result means the root itself.
fn clean(rel: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => {
                return Err(invalid(
                    rel,
                    "must be a relative path without `..` inside the collection",
                ));
            }
        }
    }
    if let Some(Component::Normal(first)) = out.components().next() {
        let first = first.to_string_lossy();
        if first.starts_with('.') || first.to_lowercase() == ENV_DIR {
            return Err(invalid(rel, format!("`{first}` is not editable here")));
        }
    }
    Ok(out)
}

/// Names may not start with `.` (they would be hidden or reserved entries).
fn check_name(name: &str) -> Result<()> {
    if name.trim_start().starts_with('.') {
        return Err(invalid(Path::new(name), "a name may not start with `.`"));
    }
    Ok(())
}

/// `a` and `b` are the same file-system entry, whatever names reach them
/// (case or Unicode folding, symlinks, short names).
fn same_entry(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(x), Ok(y)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
    }
    matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Check `abs` (which must exist) resolves inside the canonical root and not
/// into a dot-entry, the environments folder or a `collection.toml` /
/// `folder.toml`. The resolved path is judged, not the text that was given:
/// a symlink or a file-system case alias gets no further than the real name.
fn confine(rootc: &Path, rel: &Path, abs: &Path) -> Result<()> {
    let c = std::fs::canonicalize(abs).map_err(|e| io(abs, e))?;
    let Ok(inside) = c.strip_prefix(rootc) else {
        return Err(invalid(rel, "resolves outside the collection"));
    };
    if let Some(Component::Normal(first)) = inside.components().next() {
        let lower = first.to_string_lossy().to_lowercase();
        if lower.starts_with('.')
            || lower == ENV_DIR
            || same_entry(&rootc.join(first), &rootc.join(ENV_DIR))
        {
            return Err(invalid(rel, "resolves into a reserved entry"));
        }
    }
    if let Some(name) = inside.file_name() {
        let lower = name.to_string_lossy().to_lowercase();
        if lower == COLLECTION_FILE || lower == FOLDER_FILE {
            return Err(invalid(rel, "resolves to a collection or folder file"));
        }
    }
    Ok(())
}

/// A directory (existing, inside the root) to create into or move into.
fn clean_dir(rootc: &Path, dir: &Path) -> Result<PathBuf> {
    let rel = clean(dir)?;
    let abs = rootc.join(&rel);
    confine(rootc, dir, &abs)?;
    if !abs.is_dir() {
        return Err(invalid(dir, "is not a folder"));
    }
    Ok(rel)
}

#[derive(PartialEq)]
enum Kind {
    Folder,
    Request,
}

/// A node to rename, move or delete: validated relative path and its kind.
///
/// `follow` is set for rename and move, which write through the node: its own
/// canonical path must then be inside the root (delete only unlinks it).
fn clean_node(rootc: &Path, rel: &Path, follow: bool) -> Result<(PathBuf, Kind)> {
    let rel_clean = clean(rel)?;
    let Some(name) = rel_clean
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        return Err(invalid(rel, "names the collection root"));
    };
    let abs = rootc.join(&rel_clean);
    if let Some(parent) = abs.parent() {
        confine(rootc, rel, parent)?;
    }
    let meta = std::fs::symlink_metadata(&abs).map_err(|e| io(&abs, e))?;
    // A symlinked node is only unlinked by delete; anything else is judged by
    // where it really is.
    if follow || !meta.file_type().is_symlink() {
        confine(rootc, rel, &abs)?;
    }
    if abs.is_dir() {
        return Ok((rel_clean, Kind::Folder));
    }
    let lower = name.to_lowercase();
    if !lower.ends_with(".toml")
        || lower == COLLECTION_FILE
        || lower == FOLDER_FILE
        || name.contains(".body.")
        || name.starts_with('.')
    {
        return Err(invalid(rel, "is not a request or folder"));
    }
    Ok((rel_clean, Kind::Request))
}

/// `a` and `b` are the same entry under a case-insensitive file system: same
/// folder, names equal ignoring case, same canonical path, neither a symlink.
/// Two paths that merely canonicalise to the same place (a symlink and its
/// target) are different entries.
fn same_file(a: &Path, b: &Path) -> bool {
    let plain = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| !m.file_type().is_symlink());
    let (Some(an), Some(bn)) = (a.file_name(), b.file_name()) else {
        return false;
    };
    a.parent() == b.parent()
        && an.to_string_lossy().to_lowercase() == bn.to_string_lossy().to_lowercase()
        && plain(a)
        && plain(b)
        && matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Neither `<stem>` (folder) nor `<stem>.toml` (request) may exist in `dir`,
/// except `source` itself being the same entry under another case.
fn ensure_stem_free(dir: &Path, stem: &str, source: Option<&Path>, is_request: bool) -> Result<()> {
    let (own, other) = (dir.join(stem), dir.join(format!("{stem}.toml")));
    if is_request {
        ensure_free(&own, None)?;
        ensure_free(&other, source)
    } else {
        ensure_free(&own, source)?;
        ensure_free(&other, None)
    }
}

/// Fail with `Exists` when `target` is occupied by something other than `source`.
fn ensure_free(target: &Path, source: Option<&Path>) -> Result<()> {
    if std::fs::symlink_metadata(target).is_err() {
        return Ok(());
    }
    if let Some(s) = source
        && same_file(target, s)
    {
        return Ok(());
    }
    Err(exists_err(target))
}

/// Approximate full case folding (U+017F folds to `s`, as on APFS).
fn fold_case(name: &str) -> String {
    name.to_uppercase().to_lowercase()
}

fn reserved_at_root(parent_rel: &Path, name: &str) -> bool {
    let folded = fold_case(name);
    parent_rel.as_os_str().is_empty() && (folded == ENV_DIR || folded == STATE_DIR)
}

/// Pick `<slug>`, `<slug>-2`, ... for which neither `<n>.toml` nor folder `<n>` exists.
fn unique_stem(abs_dir: &Path, parent_rel: &Path, base: &str, request: bool) -> String {
    let mut n = 1u32;
    loop {
        let cand = if n == 1 {
            base.to_string()
        } else {
            format!("{base}-{n}")
        };
        n += 1;
        let taken = std::fs::symlink_metadata(abs_dir.join(&cand)).is_ok()
            || std::fs::symlink_metadata(abs_dir.join(format!("{cand}.toml"))).is_ok();
        let reserved = reserved_at_root(parent_rel, &cand)
            || (request && (cand == "collection" || cand == "folder"));
        if !taken && !reserved {
            return cand;
        }
    }
}

fn parent_of(rel: &Path) -> PathBuf {
    rel.parent().map(Path::to_path_buf).unwrap_or_default()
}

fn stem_of(name: &str, kind: &Kind) -> String {
    match kind {
        Kind::Request => name.strip_suffix(".toml").unwrap_or(name).to_string(),
        Kind::Folder => name.to_string(),
    }
}

/// How to change a parent's `order` list.
enum OrderEdit<'a> {
    Remove(&'a str),
    Replace(&'a str, &'a str),
}

/// Compute the new text of the parent's `order` file without writing it.
/// `None` when there is no such file, no `order` array, or nothing changes.
fn plan_order(
    rootc: &Path,
    parent_rel: &Path,
    edit: &OrderEdit,
) -> Result<Option<(PathBuf, String)>> {
    let path = if parent_rel.as_os_str().is_empty() {
        rootc.join(COLLECTION_FILE)
    } else {
        rootc.join(parent_rel).join(FOLDER_FILE)
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(&path, e)),
    };
    let mut doc: DocumentMut = codec::parse(&path, &text)?;
    let Some(arr) = doc.get_mut("order").and_then(Item::as_array_mut) else {
        return Ok(None);
    };
    let name = match edit {
        OrderEdit::Remove(n) | OrderEdit::Replace(n, _) => *n,
    };
    let Some(i) = arr.iter().position(|v| v.as_str() == Some(name)) else {
        return Ok(None);
    };
    apply_edit(arr, i, edit);
    Ok(Some((path, doc.to_string())))
}

fn apply_edit(arr: &mut Array, i: usize, edit: &OrderEdit) {
    match edit {
        OrderEdit::Remove(_) => {
            let first_prefix = arr.get(0).and_then(|v| v.decor().prefix().cloned());
            arr.remove(i);
            // Removing the head must not leave the next item's separator space behind.
            if i == 0
                && let (Some(prefix), Some(next)) = (first_prefix, arr.get_mut(0))
            {
                next.decor_mut().set_prefix(prefix);
            }
        }
        OrderEdit::Replace(_, new) => {
            let mut v = Value::from(*new);
            if let Some(old) = arr.get(i) {
                *v.decor_mut() = old.decor().clone();
            }
            arr.replace(i, v);
        }
    }
}

fn write_planned(plan: Option<(PathBuf, String)>) -> Result<()> {
    if let Some((path, text)) = plan {
        fs::atomic_write(&path, text.as_bytes())?;
    }
    Ok(())
}

/// Sidecar body file name of a request document, if it has one.
fn sidecar_of(doc: &DocumentMut, req_rel: &Path) -> Result<Option<String>> {
    let Some(name) = doc
        .get("body")
        .and_then(Item::as_table_like)
        .and_then(|t| t.get("file"))
        .and_then(Item::as_str)
    else {
        return Ok(None);
    };
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && name.as_bytes().get(1) != Some(&b':');
    if plain {
        Ok(Some(name.to_string()))
    } else {
        Err(invalid(
            req_rel,
            format!("sidecar `{name}` is not a plain file name"),
        ))
    }
}

/// The sidecar of the request `req_abs` if it is safe to move or delete with
/// it: a normal file, not a request/folder/collection file or dot-file, not a
/// symlink leaving the request's folder, and not used by another request there.
fn safe_sidecar(req_abs: &Path, sidecar: &str) -> Option<PathBuf> {
    let lower = sidecar.to_lowercase();
    if lower.ends_with(".toml") || sidecar.starts_with('.') {
        return None;
    }
    let dir = req_abs.parent()?;
    let path = dir.join(sidecar);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() {
        let target = std::fs::canonicalize(&path).ok()?;
        if !target.starts_with(std::fs::canonicalize(dir).ok()?) {
            return None;
        }
        // A link to a request, folder file or dot-file is not a body.
        let tname = target.file_name()?.to_string_lossy().to_lowercase();
        if tname.ends_with(".toml") || tname.starts_with('.') {
            return None;
        }
    }
    if !path.is_file() {
        return None;
    }
    let me = req_abs.file_name()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let lname = name.to_string_lossy().to_lowercase();
        if name == me
            || !lname.ends_with(".toml")
            || lname == COLLECTION_FILE
            || lname == FOLDER_FILE
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let used = match text.parse::<DocumentMut>() {
            Ok(doc) => sidecar_of(&doc, Path::new(""))
                .ok()
                .flatten()
                .is_some_and(|other| other == sidecar || same_entry(&dir.join(other), &path)),
            Err(_) => text.contains(sidecar),
        };
        if used {
            return None;
        }
    }
    Some(path)
}

fn read_request_doc(abs: &Path) -> Result<DocumentMut> {
    let text = fs::read_to_string(abs)?;
    codec::parse(abs, &text)
}

/// Create a request named `name` in `dir`. Returns its path relative to `root`.
pub fn create_request(root: &Path, dir: &Path, name: &str) -> Result<PathBuf> {
    check_name(name)?;
    let rootc = canon_root(root)?;
    let dir_rel = clean_dir(&rootc, dir)?;
    let abs_dir = rootc.join(&dir_rel);
    let stem = unique_stem(&abs_dir, &dir_rel, &slug(name), true);
    let rel = dir_rel.join(format!("{stem}.toml"));
    let abs = rootc.join(&rel);
    ensure_free(&abs, None)?;
    let mut doc = DocumentMut::new();
    codec::write_str(&mut doc, "name", name);
    codec::write_str(&mut doc, "method", "GET");
    codec::write_str(&mut doc, "url", "");
    fs::atomic_write(&abs, doc.to_string().as_bytes())?;
    Ok(rel)
}

/// Create a folder named after `name` in `parent`.
pub fn create_folder(root: &Path, parent: &Path, name: &str) -> Result<PathBuf> {
    check_name(name)?;
    let rootc = canon_root(root)?;
    let parent_rel = clean_dir(&rootc, parent)?;
    let abs_parent = rootc.join(&parent_rel);
    let stem = unique_stem(&abs_parent, &parent_rel, &slug(name), false);
    let rel = parent_rel.join(&stem);
    let abs = rootc.join(&rel);
    std::fs::create_dir(&abs).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            exists_err(&abs)
        } else {
            io(&abs, e)
        }
    })?;
    Ok(rel)
}

/// Rename a request: set its `name`, move the file to the new slug, and move
/// a sidecar body along with it.
pub fn rename_request(root: &Path, rel: &Path, new_name: &str) -> Result<PathBuf> {
    check_name(new_name)?;
    let rootc = canon_root(root)?;
    let (rel, kind) = clean_node(&rootc, rel, true)?;
    if kind != Kind::Request {
        return Err(invalid(&rel, "is not a request"));
    }
    let parent_rel = parent_of(&rel);
    let abs = rootc.join(&rel);
    let old_file = rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let old_stem = stem_of(&old_file, &Kind::Request);
    let new_stem = slug(new_name);
    let mut doc = read_request_doc(&abs)?;
    let sidecar = sidecar_of(&doc, &rel)?;
    let dir_abs = rootc.join(&parent_rel);

    let renaming = new_stem != old_stem;
    let mut new_sidecar = None;
    if renaming {
        if new_stem == "collection" || new_stem == "folder" {
            return Err(exists_err(&dir_abs.join(format!("{new_stem}.toml"))));
        }
        ensure_stem_free(&dir_abs, &new_stem, Some(&abs), true)?;
        if let Some(sc) = &sidecar
            && safe_sidecar(&abs, sc).is_some()
        {
            let new = match sc.strip_prefix(&format!("{old_stem}.")) {
                Some(rest) => format!("{new_stem}.{rest}"),
                None => sc.clone(),
            };
            if new != *sc {
                ensure_free(&dir_abs.join(&new), Some(&dir_abs.join(sc)))?;
                new_sidecar = Some(new);
            }
        }
    }
    let new_rel = parent_rel.join(format!("{new_stem}.toml"));
    let order = if renaming {
        plan_order(
            &rootc,
            &parent_rel,
            &OrderEdit::Replace(&old_stem, &new_stem),
        )?
    } else {
        None
    };

    let original = fs::read_to_string(&abs)?;
    codec::write_str(&mut doc, "name", new_name);
    if let Some(new) = &new_sidecar
        && let Some(t) = doc.get_mut("body").and_then(Item::as_table_mut)
    {
        codec::write_str(t, "file", new);
    }
    // Sidecar first, so a failure never leaves the request pointing at nothing.
    let sidecar_paths = match (&sidecar, &new_sidecar) {
        (Some(old), Some(new)) => Some((dir_abs.join(old), dir_abs.join(new))),
        _ => None,
    };
    if let Some((from, to)) = &sidecar_paths {
        std::fs::rename(from, to).map_err(|e| io(from, e))?;
    }
    let undo_sidecar = || {
        if let Some((from, to)) = &sidecar_paths {
            let _ = std::fs::rename(to, from);
        }
    };
    if let Err(e) = fs::atomic_write(&abs, doc.to_string().as_bytes()) {
        undo_sidecar();
        return Err(e);
    }
    if renaming && let Err(e) = std::fs::rename(&abs, rootc.join(&new_rel)) {
        undo_sidecar();
        let _ = fs::atomic_write(&abs, original.as_bytes());
        return Err(io(&abs, e));
    }
    write_planned(order)?;
    Ok(new_rel)
}

/// Rename a folder to the slug of `new_name`.
pub fn rename_folder(root: &Path, rel: &Path, new_name: &str) -> Result<PathBuf> {
    check_name(new_name)?;
    let rootc = canon_root(root)?;
    let (rel, kind) = clean_node(&rootc, rel, true)?;
    if kind != Kind::Folder {
        return Err(invalid(&rel, "is not a folder"));
    }
    let parent_rel = parent_of(&rel);
    let old = rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let new = slug(new_name);
    let new_rel = parent_rel.join(&new);
    if new == old {
        return Ok(rel);
    }
    if reserved_at_root(&parent_rel, &new) {
        return Err(exists_err(&rootc.join(&new_rel)));
    }
    let abs = rootc.join(&rel);
    let target = rootc.join(&new_rel);
    ensure_stem_free(&rootc.join(&parent_rel), &new, Some(&abs), false)?;
    let order = plan_order(&rootc, &parent_rel, &OrderEdit::Replace(&old, &new))?;
    std::fs::rename(&abs, &target).map_err(|e| io(&abs, e))?;
    write_planned(order)?;
    Ok(new_rel)
}

/// Move a request (with its sidecar) or a folder into `new_parent`.
pub fn move_node(root: &Path, rel: &Path, new_parent: &Path) -> Result<PathBuf> {
    let rootc = canon_root(root)?;
    let (rel, kind) = clean_node(&rootc, rel, true)?;
    let dest_rel = clean_dir(&rootc, new_parent)?;
    let name = rel
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let name_str = name.to_string_lossy().into_owned();
    let parent_rel = parent_of(&rel);
    if kind == Kind::Folder && dest_rel.starts_with(&rel) {
        return Err(invalid(new_parent, "cannot move a folder into itself"));
    }
    if dest_rel == parent_rel {
        return Ok(rel);
    }
    if reserved_at_root(&dest_rel, &name_str) {
        return Err(exists_err(&rootc.join(&dest_rel).join(&name)));
    }
    let abs = rootc.join(&rel);
    let new_rel = dest_rel.join(&name);
    let target = rootc.join(&new_rel);
    // A folder and a request of the same stem would collide in `order`.
    ensure_stem_free(
        &rootc.join(&dest_rel),
        &stem_of(&name_str, &kind),
        None,
        kind == Kind::Request,
    )?;
    let mut sidecar_move = None;
    if kind == Kind::Request
        && let Some(sc) = sidecar_of(&read_request_doc(&abs)?, &rel).ok().flatten()
        && let Some(from) = safe_sidecar(&abs, &sc)
    {
        let to = rootc.join(&dest_rel).join(&sc);
        ensure_free(&to, None)?;
        if to == target {
            return Err(exists_err(&to));
        }
        sidecar_move = Some((from, to));
    }
    let order = plan_order(
        &rootc,
        &parent_rel,
        &OrderEdit::Remove(&stem_of(&name_str, &kind)),
    )?;
    if let Some((from, to)) = &sidecar_move {
        std::fs::rename(from, to).map_err(|e| io(from, e))?;
    }
    if let Err(e) = std::fs::rename(&abs, &target) {
        if let Some((from, to)) = &sidecar_move {
            let _ = std::fs::rename(to, from);
        }
        return Err(io(&abs, e));
    }
    write_planned(order)?;
    Ok(new_rel)
}

/// Delete a request (with its sidecar) or a folder recursively. Permanent.
pub fn delete_node(root: &Path, rel: &Path) -> Result<()> {
    let rootc = canon_root(root)?;
    let (rel, kind) = clean_node(&rootc, rel, false)?;
    let parent_rel = parent_of(&rel);
    let abs = rootc.join(&rel);
    let name = rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let order = plan_order(
        &rootc,
        &parent_rel,
        &OrderEdit::Remove(&stem_of(&name, &kind)),
    )?;
    match kind {
        Kind::Folder => {
            // A symlinked folder is unlinked, never followed.
            if std::fs::symlink_metadata(&abs).is_ok_and(|m| m.file_type().is_symlink()) {
                std::fs::remove_file(&abs).map_err(|e| io(&abs, e))?;
            } else {
                std::fs::remove_dir_all(&abs).map_err(|e| io(&abs, e))?;
            }
        }
        Kind::Request => {
            // A symlinked request is only unlinked: its content is not ours to trust.
            let linked = std::fs::symlink_metadata(&abs).is_ok_and(|m| m.file_type().is_symlink());
            let sidecar = if linked {
                None
            } else {
                read_request_doc(&abs)
                    .ok()
                    .and_then(|d| sidecar_of(&d, &rel).ok().flatten())
                    .and_then(|sc| safe_sidecar(&abs, &sc))
            };
            std::fs::remove_file(&abs).map_err(|e| io(&abs, e))?;
            if let Some(p) = sidecar {
                std::fs::remove_file(&p).map_err(|e| io(&p, e))?;
            }
        }
    }
    write_planned(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::{Node, open_collection};
    use std::fs as stdfs;

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        stdfs::create_dir_all(p.parent().unwrap()).unwrap();
        stdfs::write(p, text).unwrap();
    }

    fn req(name: &str) -> String {
        format!("name = \"{name}\"\nmethod = \"GET\"\nurl = \"http://x\"\n")
    }

    fn sidecar_req(name: &str, sc: &str) -> String {
        format!("{}[body]\ntype = \"json\"\nfile = \"{sc}\"\n", req(name))
    }

    fn col() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "collection.toml", "format = 1\n");
        d
    }

    /// Flatten the opened tree to `path:name` strings.
    fn tree(root: &Path) -> Vec<String> {
        fn walk(nodes: &[Node], out: &mut Vec<String>) {
            for n in nodes {
                match n {
                    Node::Folder {
                        path,
                        name,
                        children,
                    } => {
                        out.push(format!("{}/ {name}", path.display()));
                        walk(children, out);
                    }
                    Node::Request(m) => out.push(format!("{} {}", m.path.display(), m.name)),
                    Node::Invalid { path, .. } => out.push(format!("{} INVALID", path.display())),
                }
            }
        }
        let mut out = Vec::new();
        walk(&open_collection(root).unwrap().nodes, &mut out);
        out
    }

    fn snapshot(root: &Path) -> Vec<(PathBuf, String)> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<(PathBuf, String)>) {
            for e in stdfs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let rel = p.strip_prefix(root).unwrap().to_path_buf();
                if p.is_dir() {
                    out.push((rel, "<dir>".into()));
                    walk(&p, root, out);
                } else {
                    out.push((rel, stdfs::read_to_string(&p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn slug_rules() {
        assert_eq!(slug("Get User!"), "get-user");
        assert_eq!(slug("  a  / b "), "a-b");
        assert_eq!(slug("..."), "untitled");
        assert_eq!(slug("Ünï 2"), "ünï-2");
    }

    #[test]
    fn create_request_and_suffixes() {
        let d = col();
        let a = create_request(d.path(), Path::new(""), "Get User").unwrap();
        assert_eq!(a, Path::new("get-user.toml"));
        let b = create_request(d.path(), Path::new(""), "Get User").unwrap();
        assert_eq!(b, Path::new("get-user-2.toml"));
        let c = create_request(d.path(), Path::new(""), "get user").unwrap();
        assert_eq!(c, Path::new("get-user-3.toml"));
        assert_eq!(
            tree(d.path()),
            [
                "get-user.toml Get User",
                "get-user-2.toml Get User",
                "get-user-3.toml get user"
            ]
        );
        let r = crate::request::load_request(&d.path().join(&a)).unwrap();
        assert_eq!(r.url, "");
    }

    #[test]
    fn create_never_makes_reserved_files() {
        let d = col();
        let r = create_request(d.path(), Path::new(""), "Collection").unwrap();
        assert_eq!(r, Path::new("collection-2.toml"));
        let f = create_request(d.path(), Path::new(""), "folder").unwrap();
        assert_eq!(f, Path::new("folder-2.toml"));
        let e = create_folder(d.path(), Path::new(""), "Environments").unwrap();
        assert_eq!(e, Path::new("environments-2"));
    }

    #[test]
    fn create_folder_nested() {
        let d = col();
        let f = create_folder(d.path(), Path::new(""), "Users").unwrap();
        let g = create_folder(d.path(), &f, "Admin Tools").unwrap();
        assert_eq!(g, Path::new("users/admin-tools"));
        let h = create_folder(d.path(), Path::new(""), "users").unwrap();
        assert_eq!(h, Path::new("users-2"));
        assert_eq!(
            tree(d.path()),
            [
                "users/ users",
                "users/admin-tools/ admin-tools",
                "users-2/ users-2"
            ]
        );
    }

    #[test]
    fn rename_request_with_sidecar_and_order() {
        let d = col();
        let r = d.path();
        w(
            r,
            "collection.toml",
            "format = 1\n# keep me\norder = [\"a\", \"b\"] # trailing\n# last\n",
        );
        w(r, "a.toml", &sidecar_req("A", "a.body.json"));
        w(r, "a.body.json", "{}");
        w(r, "b.toml", &req("B"));
        let new = rename_request(r, Path::new("a.toml"), "Fresh Name").unwrap();
        assert_eq!(new, Path::new("fresh-name.toml"));
        assert!(!r.join("a.toml").exists() && !r.join("a.body.json").exists());
        assert_eq!(
            stdfs::read_to_string(r.join("fresh-name.body.json")).unwrap(),
            "{}"
        );
        let loaded = crate::request::load_request(&r.join(&new)).unwrap();
        assert_eq!(loaded.name, "Fresh Name");
        let c = stdfs::read_to_string(r.join("collection.toml")).unwrap();
        assert_eq!(
            c,
            "format = 1\n# keep me\norder = [\"fresh-name\", \"b\"] # trailing\n# last\n"
        );
        assert_eq!(tree(r), ["fresh-name.toml Fresh Name", "b.toml B"]);
    }

    #[test]
    fn rename_keeps_comments_in_request() {
        let d = col();
        let r = d.path();
        w(
            r,
            "a.toml",
            "# top\nname = \"A\" # n\nmethod = \"GET\"\nurl = \"u\"\n",
        );
        rename_request(r, Path::new("a.toml"), "Z").unwrap();
        assert_eq!(
            stdfs::read_to_string(r.join("z.toml")).unwrap(),
            "# top\nname = \"Z\" # n\nmethod = \"GET\"\nurl = \"u\"\n"
        );
    }

    #[test]
    fn rename_only_name_change_keeps_file() {
        let d = col();
        w(d.path(), "a.toml", &req("A"));
        let n = rename_request(d.path(), Path::new("a.toml"), "A!").unwrap();
        assert_eq!(n, Path::new("a.toml"));
        assert_eq!(tree(d.path()), ["a.toml A!"]);
    }

    #[test]
    fn rename_folder_updates_order_and_tree() {
        let d = col();
        let r = d.path();
        w(r, "f/folder.toml", "order = [\"x\", \"sub\"]\n# c\n");
        w(r, "f/x.toml", &req("X"));
        w(r, "f/sub/y.toml", &req("Y"));
        w(r, "collection.toml", "format = 1\norder = [\"f\"]\n");
        let n = rename_folder(r, Path::new("f"), "Group").unwrap();
        assert_eq!(n, Path::new("group"));
        assert_eq!(
            stdfs::read_to_string(r.join("collection.toml")).unwrap(),
            "format = 1\norder = [\"group\"]\n"
        );
        let n2 = rename_folder(r, Path::new("group/sub"), "Inner").unwrap();
        assert_eq!(n2, Path::new("group/inner"));
        assert_eq!(
            stdfs::read_to_string(r.join("group/folder.toml")).unwrap(),
            "order = [\"x\", \"inner\"]\n# c\n"
        );
        assert_eq!(
            tree(r),
            [
                "group/ group",
                "group/x.toml X",
                "group/inner/ inner",
                "group/inner/y.toml Y"
            ]
        );
    }

    #[test]
    fn move_request_with_sidecar_removes_order() {
        let d = col();
        let r = d.path();
        w(r, "src/folder.toml", "# hi\norder = [\"a\", \"b\"]\n");
        w(r, "src/a.toml", &sidecar_req("A", "a.body.json"));
        w(r, "src/a.body.json", "{}");
        w(r, "src/b.toml", &req("B"));
        w(r, "dst/x.toml", &req("X"));
        let n = move_node(r, Path::new("src/a.toml"), Path::new("dst")).unwrap();
        assert_eq!(n, Path::new("dst/a.toml"));
        assert!(r.join("dst/a.body.json").exists());
        assert!(!r.join("src/a.body.json").exists());
        assert_eq!(
            stdfs::read_to_string(r.join("src/folder.toml")).unwrap(),
            "# hi\norder = [\"b\"]\n"
        );
        assert_eq!(
            tree(r),
            [
                "dst/ dst",
                "dst/a.toml A",
                "dst/x.toml X",
                "src/ src",
                "src/b.toml B"
            ]
        );
        let loaded = crate::request::load_request(&r.join(&n)).unwrap();
        assert!(matches!(loaded.body, wisp_core::request::Body::Json(_)));
    }

    #[test]
    fn move_folder_and_to_root() {
        let d = col();
        let r = d.path();
        w(r, "a/b/x.toml", &req("X"));
        w(r, "c/y.toml", &req("Y"));
        let n = move_node(r, Path::new("a/b"), Path::new("c")).unwrap();
        assert_eq!(n, Path::new("c/b"));
        let n = move_node(r, Path::new("c/y.toml"), Path::new("")).unwrap();
        assert_eq!(n, Path::new("y.toml"));
        assert_eq!(
            tree(r),
            ["a/ a", "c/ c", "c/b/ b", "c/b/x.toml X", "y.toml Y"]
        );
    }

    #[test]
    fn move_folder_into_itself_rejected() {
        let d = col();
        w(d.path(), "a/b/x.toml", &req("X"));
        let before = snapshot(d.path());
        for to in ["a", "a/b"] {
            let e = move_node(d.path(), Path::new("a"), Path::new(to)).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{to}: {e:?}");
        }
        assert_eq!(snapshot(d.path()), before);
    }

    #[test]
    fn delete_request_folder_and_order() {
        let d = col();
        let r = d.path();
        w(
            r,
            "collection.toml",
            "format = 1\norder = [\"a\", \"f\", \"b\"]\n",
        );
        w(r, "a.toml", &sidecar_req("A", "a.body.json"));
        w(r, "a.body.json", "{}");
        w(r, "b.toml", &req("B"));
        w(r, "f/x.toml", &req("X"));
        delete_node(r, Path::new("a.toml")).unwrap();
        assert!(!r.join("a.body.json").exists());
        delete_node(r, Path::new("f")).unwrap();
        assert!(!r.join("f").exists());
        assert_eq!(
            stdfs::read_to_string(r.join("collection.toml")).unwrap(),
            "format = 1\norder = [\"b\"]\n"
        );
        assert_eq!(tree(r), ["b.toml B"]);
    }

    #[test]
    fn collisions_leave_disk_unchanged() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", "a.body.json"));
        w(r, "a.body.json", "{}");
        w(r, "z.toml", &req("Z"));
        w(r, "f/a.toml", &req("Other"));
        w(r, "g/h/q.toml", &req("Q"));
        w(r, "h/q.toml", &req("Q2"));
        let before = snapshot(r);
        // request rename onto existing request
        let e = rename_request(r, Path::new("a.toml"), "Z").unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        // request move onto existing request
        let e = move_node(r, Path::new("a.toml"), Path::new("f")).unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        // folder rename and move onto existing folder
        let e = rename_folder(r, Path::new("g"), "h").unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        let e = move_node(r, Path::new("g/h"), Path::new("")).unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        assert_eq!(snapshot(r), before);
    }

    #[test]
    fn sidecar_collision_on_move() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", "a.body.json"));
        w(r, "a.body.json", "{}");
        w(r, "f/a.body.json", "other");
        let before = snapshot(r);
        let e = move_node(r, Path::new("a.toml"), Path::new("f")).unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        assert_eq!(snapshot(r), before);
    }

    #[test]
    fn rename_sidecar_onto_sibling_request_file_fails() {
        // The new sidecar name equals an existing file.
        let d = col();
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", "a.new.json"));
        w(r, "a.new.json", "{}");
        w(r, "new.toml", &req("Sibling"));
        w(r, "x.toml", &req("X"));
        // sidecar `a.new.json` -> `b.new.json`, which already exists.
        w(r, "b.new.json", "{}");
        let before = snapshot(r);
        let e = rename_request(r, Path::new("a.toml"), "B").unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        assert_eq!(snapshot(r), before);
    }

    #[test]
    fn rename_sidecar_equal_to_new_request_file_fails() {
        let d = col();
        let r = d.path();
        // sidecar `a.toml`-style prefix rule: `a.` + `toml` -> `b.toml`.
        w(r, "a.toml", &sidecar_req("A", "a.toml.x"));
        w(r, "a.toml.x", "{}");
        let n = rename_request(r, Path::new("a.toml"), "B").unwrap();
        assert_eq!(n, Path::new("b.toml"));
        assert!(r.join("b.toml.x").exists());
    }

    #[test]
    fn invalid_paths_rejected() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &req("A"));
        w(r, "environments/dev.toml", "[vars]\n");
        w(r, ".wisp/h.jsonl", "");
        w(r, "f/x.toml", &req("X"));
        let before = snapshot(r);
        let bad: Vec<PathBuf> = vec![
            "/abs/a.toml".into(),
            "../a.toml".into(),
            "f/../../a.toml".into(),
            "".into(),
            ".".into(),
            "environments".into(),
            "environments/dev.toml".into(),
            ".wisp".into(),
            "collection.toml".into(),
        ];
        for p in &bad {
            for (what, res) in [
                ("rename_request", rename_request(r, p, "n").map(|_| ())),
                ("rename_folder", rename_folder(r, p, "n").map(|_| ())),
                ("move", move_node(r, p, Path::new("f")).map(|_| ())),
                ("delete", delete_node(r, p)),
            ] {
                assert!(
                    matches!(res, Err(StorageError::Invalid { .. })),
                    "{what} {p:?}: {res:?}"
                );
            }
        }
        for p in ["/abs", "..", "environments", ".wisp", "f/.."] {
            let e = create_request(r, Path::new(p), "n").unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{p}: {e:?}");
            let e = create_folder(r, Path::new(p), "n").unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{p}: {e:?}");
            let e = move_node(r, Path::new("a.toml"), Path::new(p)).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{p}: {e:?}");
        }
        assert_eq!(snapshot(r), before);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_rejected() {
        let d = col();
        let out = tempfile::tempdir().unwrap();
        w(out.path(), "v.toml", &req("V"));
        std::os::unix::fs::symlink(out.path(), d.path().join("link")).unwrap();
        let e = delete_node(d.path(), Path::new("link/v.toml")).unwrap_err();
        assert!(matches!(e, StorageError::Invalid { .. }), "{e:?}");
        let e = create_request(d.path(), Path::new("link"), "n").unwrap_err();
        assert!(matches!(e, StorageError::Invalid { .. }), "{e:?}");
        assert!(out.path().join("v.toml").exists());
        // Deleting the link itself unlinks it and leaves the target.
        delete_node(d.path(), Path::new("link")).unwrap();
        assert!(out.path().join("v.toml").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rename_symlink_onto_its_target_fails() {
        let d = col();
        let r = d.path();
        w(r, "b.toml", &req("Real"));
        std::os::unix::fs::symlink("b.toml", r.join("a.toml")).unwrap();
        let e = rename_request(r, Path::new("a.toml"), "B").unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        assert_eq!(
            stdfs::read_to_string(r.join("b.toml")).unwrap(),
            req("Real")
        );
        assert!(
            stdfs::symlink_metadata(r.join("a.toml"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rename_and_move_symlink_to_outside_file_refused() {
        let d = col();
        let out = tempfile::tempdir().unwrap();
        let victim = req("Victim");
        w(out.path(), "victim.toml", &victim);
        w(d.path(), "f/x.toml", &req("X"));
        std::os::unix::fs::symlink(out.path().join("victim.toml"), d.path().join("v.toml"))
            .unwrap();
        let e = rename_request(d.path(), Path::new("v.toml"), "v").unwrap_err();
        assert!(matches!(e, StorageError::Invalid { .. }), "{e:?}");
        let e = rename_request(d.path(), Path::new("v.toml"), "w").unwrap_err();
        assert!(matches!(e, StorageError::Invalid { .. }), "{e:?}");
        let e = move_node(d.path(), Path::new("v.toml"), Path::new("f")).unwrap_err();
        assert!(matches!(e, StorageError::Invalid { .. }), "{e:?}");
        assert_eq!(
            stdfs::read_to_string(out.path().join("victim.toml")).unwrap(),
            victim
        );
        // Delete may unlink the link and leaves the target alone.
        delete_node(d.path(), Path::new("v.toml")).unwrap();
        assert_eq!(
            stdfs::read_to_string(out.path().join("victim.toml")).unwrap(),
            victim
        );
    }

    fn unsafe_sidecar_setup(sc: &str) -> tempfile::TempDir {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", sc));
        w(r, "b.toml", &req("B"));
        w(r, "f/keep.toml", &req("Keep"));
        match sc {
            "collection.toml" => w(r, "collection.toml", "format = 1\n# precious\n"),
            "shared.json" => {
                w(r, "shared.json", "{}");
                w(r, "c.toml", &sidecar_req("C", sc));
            }
            "b.toml" => {}
            _ => w(r, sc, "precious"),
        }
        d
    }

    #[test]
    fn delete_leaves_unsafe_sidecars() {
        for sc in [
            "collection.toml",
            "b.toml",
            ".x",
            "shared.json",
            "Folder.TOML",
        ] {
            let d = unsafe_sidecar_setup(sc);
            let r = d.path();
            if sc == "Folder.TOML" {
                w(r, "Folder.TOML", "precious");
            }
            let before = stdfs::read(r.join(sc)).unwrap();
            delete_node(r, Path::new("a.toml")).unwrap();
            assert!(!r.join("a.toml").exists(), "{sc}");
            assert_eq!(stdfs::read(r.join(sc)).unwrap(), before, "{sc}");
        }
    }

    #[test]
    fn move_leaves_unsafe_sidecars() {
        for sc in ["collection.toml", "b.toml", ".x", "shared.json"] {
            let d = unsafe_sidecar_setup(sc);
            let r = d.path();
            let before = stdfs::read(r.join(sc)).unwrap();
            let n = move_node(r, Path::new("a.toml"), Path::new("f")).unwrap();
            assert_eq!(n, Path::new("f/a.toml"), "{sc}");
            assert!(r.join("f/a.toml").exists() && !r.join("a.toml").exists());
            assert_eq!(stdfs::read(r.join(sc)).unwrap(), before, "{sc}");
            assert!(!r.join("f").join(sc).exists() || sc == "keep.toml", "{sc}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn delete_leaves_sidecar_symlinked_outside_folder() {
        let d = col();
        let out = tempfile::tempdir().unwrap();
        w(out.path(), "s.json", "precious");
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", "s.json"));
        std::os::unix::fs::symlink(out.path().join("s.json"), r.join("s.json")).unwrap();
        delete_node(r, Path::new("a.toml")).unwrap();
        assert!(r.join("s.json").symlink_metadata().is_ok());
        assert_eq!(
            stdfs::read_to_string(out.path().join("s.json")).unwrap(),
            "precious"
        );
    }

    #[test]
    fn delete_refuses_dot_and_reserved_names_any_case() {
        let d = col();
        let r = d.path();
        w(r, ".git/config", "g");
        w(r, ".github/w.yml", "g");
        w(r, ".wisp/h.jsonl", "h");
        w(r, "environments/dev.toml", "[vars]\n");
        w(r, "f/folder.toml", "order = []\n");
        let before = snapshot(r);
        for p in [
            ".git",
            ".github",
            ".WISP",
            "Environments",
            "Collection.toml",
            "f/Folder.toml",
        ] {
            let e = delete_node(r, Path::new(p)).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{p}: {e:?}");
            assert_eq!(snapshot(r), before, "{p}");
        }
    }

    #[test]
    fn move_into_reserved_any_case_rejected() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &req("A"));
        w(r, "environments/dev.toml", "[vars]\n");
        let before = snapshot(r);
        for p in ["ENVIRONMENTS", ".WISP", ".git"] {
            let e = move_node(r, Path::new("a.toml"), Path::new(p)).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{p}: {e:?}");
        }
        assert_eq!(snapshot(r), before);
    }

    #[test]
    fn dot_names_rejected() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &req("A"));
        w(r, "f/x.toml", &req("X"));
        let before = snapshot(r);
        for n in [".hidden", ".git", " .x"] {
            let e = create_request(r, Path::new(""), n).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{n}: {e:?}");
            let e = create_folder(r, Path::new(""), n).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{n}: {e:?}");
            let e = rename_request(r, Path::new("a.toml"), n).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{n}: {e:?}");
            let e = rename_folder(r, Path::new("f"), n).unwrap_err();
            assert!(matches!(e, StorageError::Invalid { .. }), "{n}: {e:?}");
        }
        assert_eq!(snapshot(r), before);
    }

    #[test]
    fn folder_and_request_stems_collide_in_all_ops() {
        let d = col();
        let r = d.path();
        w(r, "users.toml", &req("U"));
        w(r, "x/users/y.toml", &req("Y"));
        w(r, "users3.toml", &req("U3"));
        w(r, "users2/z.toml", &req("Z"));
        w(r, "q.toml", &req("Q"));
        w(r, "folder-q/z.toml", &req("Z"));
        w(r, "x/q.toml", &req("Q2"));
        let before = snapshot(r);
        let e = move_node(r, Path::new("x/users"), Path::new("")).unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        let e = rename_folder(r, Path::new("users2"), "users3").unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        let e = rename_request(r, Path::new("q.toml"), "folder q").unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        w(r, "x/folder-q/z.toml", &req("Z"));
        let before2 = snapshot(r);
        let e = move_node(r, Path::new("q.toml"), Path::new("x")).unwrap_err();
        assert!(matches!(e, StorageError::Exists { .. }), "{e:?}");
        let _ = before;
        assert_eq!(snapshot(r), before2);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_into_reserved_places_refused() {
        use std::os::unix::fs::symlink;
        let d = col();
        let r = d.path();
        w(r, ".git/objects/o", "g");
        w(r, "environments/prod.secrets.toml", "secret = 1\n");
        w(r, "environments/dev.toml", "[vars]\n");
        w(r, "a.toml", &req("A"));
        symlink(".git", r.join("g")).unwrap();
        symlink("environments", r.join("envlink")).unwrap();
        symlink("collection.toml", r.join("v.toml")).unwrap();
        symlink("environments/prod.secrets.toml", r.join("s.toml")).unwrap();
        let before = snapshot(r);
        let inv = |e: StorageError, what: &str| {
            assert!(matches!(e, StorageError::Invalid { .. }), "{what}: {e:?}");
        };
        inv(
            delete_node(r, Path::new("g/objects")).unwrap_err(),
            "delete via g",
        );
        inv(
            move_node(r, Path::new("envlink/dev.toml"), Path::new("")).unwrap_err(),
            "move out of envlink",
        );
        inv(
            move_node(r, Path::new("a.toml"), Path::new("envlink")).unwrap_err(),
            "move into envlink",
        );
        inv(
            rename_request(r, Path::new("v.toml"), "x").unwrap_err(),
            "rename v",
        );
        inv(
            rename_request(r, Path::new("s.toml"), "x").unwrap_err(),
            "rename s",
        );
        inv(
            create_request(r, Path::new("envlink"), "n").unwrap_err(),
            "create",
        );
        assert_eq!(snapshot(r), before);
        assert_eq!(
            stdfs::read_to_string(r.join("collection.toml")).unwrap(),
            "format = 1\n"
        );
    }

    #[test]
    fn delete_refuses_wide_case_fold_alias() {
        let d = col();
        let r = d.path();
        w(r, "environments/dev.toml", "[vars]\n");
        let before = snapshot(r);
        // On a case-folding file system U+017F opens `environments`; elsewhere
        // the entry does not exist. Either way nothing may be removed.
        let e = delete_node(r, Path::new("environment\u{17f}"));
        if r.join("environment\u{17f}").exists() {
            assert!(matches!(e, Err(StorageError::Invalid { .. })), "{e:?}");
        }
        assert_eq!(snapshot(r), before);
    }

    #[test]
    fn case_alias_sidecar_counts_as_shared() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", "a.body.json"));
        w(r, "b.toml", &sidecar_req("B", "A.body.json"));
        w(r, "d.toml", &sidecar_req("D", "a.body.json"));
        w(r, "f/x.toml", &req("X"));
        w(r, "a.body.json", "{}");
        let folds = r.join("A.body.json").exists();
        if folds {
            rename_request(r, Path::new("a.toml"), "c").unwrap();
            assert!(r.join("a.body.json").exists());
            assert!(r.join("c.toml").exists());
            move_node(r, Path::new("c.toml"), Path::new("f")).unwrap();
            assert!(r.join("a.body.json").exists());
            delete_node(r, Path::new("d.toml")).unwrap();
            assert!(r.join("a.body.json").exists());
            assert!(open_collection(r).is_ok());
        }
    }

    #[test]
    fn rename_leaves_unsafe_sidecars() {
        for sc in ["a.body.json", "a.x.toml", "a.collection.toml", "a.b.toml"] {
            let d = col();
            let r = d.path();
            w(r, "a.toml", &sidecar_req("A", sc));
            w(r, "b.toml", &req("B"));
            match sc {
                "a.body.json" => {
                    w(r, sc, "{}");
                    w(r, "b.toml", &sidecar_req("B", sc));
                }
                "a.x.toml" => w(r, sc, &req("AX")),
                _ => w(r, sc, "precious"),
            }
            let before = stdfs::read(r.join(sc)).unwrap();
            let n = rename_request(r, Path::new("a.toml"), "c").unwrap();
            assert_eq!(n, Path::new("c.toml"), "{sc}");
            assert_eq!(stdfs::read(r.join(sc)).unwrap(), before, "{sc}");
            let text = stdfs::read_to_string(r.join("c.toml")).unwrap();
            assert!(text.contains(&format!("file = \"{sc}\"")), "{sc}: {text}");
            if sc == "a.body.json" {
                assert!(
                    stdfs::read_to_string(r.join("b.toml"))
                        .unwrap()
                        .contains(sc)
                );
            }
            if sc == "a.x.toml" {
                assert!(
                    tree(r).iter().any(|l| l.ends_with("a.x.toml AX")),
                    "{:?}",
                    tree(r)
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn move_undoes_sidecar_when_request_rename_fails() {
        let d = col();
        let r = d.path();
        w(r, "a.toml", &sidecar_req("A", "a.json"));
        w(r, "a.json", "{}");
        w(r, "f/keep.toml", &req("K"));
        // Only the request rename fails: the immutable flag blocks moving
        // `a.toml` while the sidecar and the folder stay writable.
        let flag = |arg: &str| {
            assert!(
                std::process::Command::new("chflags")
                    .arg(arg)
                    .arg(r.join("a.toml"))
                    .status()
                    .unwrap()
                    .success()
            );
        };
        flag("uchg");
        let res = move_node(r, Path::new("a.toml"), Path::new("f"));
        flag("nouchg");
        assert!(res.is_err(), "{res:?}");
        assert!(r.join("a.json").exists());
        assert!(!r.join("f/a.json").exists());
        assert!(r.join("a.toml").exists());
    }

    #[test]
    fn reserved_names_fold_wide_case() {
        let d = col();
        let r = d.path();
        w(r, "x/y.toml", &req("Y"));
        let made = create_folder(r, Path::new(""), "environment\u{17f}").unwrap();
        assert_ne!(made, Path::new("environment\u{17f}"));
        assert!(reserved_at_root(Path::new(""), "environment\u{17f}"));
        assert!(rename_folder(r, Path::new("x"), "environment\u{17f}").is_err());
        assert!(r.join("x").exists());
    }
}
