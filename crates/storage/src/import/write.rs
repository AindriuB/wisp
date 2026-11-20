//! Write an [`ImportResult`] into an open collection.
//!
//! Uses the ordinary storage write paths (`create_request`, `save_request`,
//! `save_environment`, ...), so names are checked and files are written
//! atomically, with secrets only in `*.secrets.toml`.

use std::collections::HashSet;
use std::path::Path;

use wisp_core::request::{Environment, FolderDefaults};

use crate::collection::{
    Node, load_collection_defaults, open_collection, save_collection_defaults, save_folder_defaults,
};
use crate::environment::{list_environments, save_environment};
use crate::ops::{StemHints, create_folder_hinted, create_request_hinted, slug};
use crate::request::save_request;

use super::openapi::env_name;
use super::{ImportError, ImportItem, ImportResult, SECRETS_ENV_PLACEHOLDER};

/// What [`write`] created.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteReport {
    pub requests: usize,
    pub folders: usize,
    pub environments: usize,
    /// `(imported name, name used)` for every node or environment renamed.
    pub renamed: Vec<(String, String)>,
}

/// Write `result` into the collection at `root`, under `folder_rel`
/// (relative to the root; empty for the root itself). The environment that
/// holds Postman secrets is named `secrets_env`. Environment names cannot
/// hold spaces, so a clashing environment gets `-2`, `-3`, ... instead.
pub fn write(
    root: &Path,
    folder_rel: &Path,
    result: &ImportResult,
    secrets_env: &str,
) -> Result<WriteReport, ImportError> {
    let tree = open_collection(root)?;
    check_folder_rel(root, folder_rel)?;
    let mut report = WriteReport::default();

    let mut existing: &[Node] = &tree.nodes;
    for comp in folder_rel.components() {
        let want = comp.as_os_str();
        let found = existing.iter().find_map(|n| match n {
            Node::Folder { path, children, .. } if path.file_name() == Some(want) => {
                Some(children.as_slice())
            }
            _ => None,
        });
        existing = found.unwrap_or(&[]);
    }
    let mut taken = Names {
        taken: existing.iter().map(|n| fold(&node_name(n))).collect(),
        ..Names::default()
    };

    if !result.collection_vars.is_empty() {
        let mut defaults = load_collection_defaults(root)?;
        let before = defaults.vars.len();
        for v in &result.collection_vars {
            if !defaults.vars.iter().any(|d| d.name == v.name) {
                defaults.vars.push(v.clone());
            }
        }
        if defaults.vars.len() != before {
            save_collection_defaults(root, &defaults)?;
        }
    }

    let mut env_taken: HashSet<String> = list_environments(root)?
        .iter()
        .map(|n| n.to_ascii_lowercase())
        .collect();
    // A leftover `<name>.secrets.toml` (secrets files are gitignored, so they
    // survive branch switches) with no `<name>.toml` still holds secrets.
    if let Ok(entries) = std::fs::read_dir(root.join("environments")) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = file.strip_suffix(".secrets.toml") {
                env_taken.insert(stem.to_ascii_lowercase());
            }
        }
    }
    let mut env_next: std::collections::HashMap<String, usize> = Default::default();
    for env in &result.environments {
        let wanted = if env.name == SECRETS_ENV_PLACEHOLDER {
            secrets_env
        } else {
            &env.name
        };
        let mut base = env_name(wanted);
        if base.is_empty() {
            base = "imported".into();
        }
        // A reserved device name (`con`, `nul`, ...) cannot be a file stem.
        if crate::fs::is_reserved_device_name(&base) {
            base = format!("{base}-env");
        }
        let mut name = base.clone();
        if env_taken.contains(&name.to_ascii_lowercase()) {
            let n = env_next.entry(base.to_ascii_lowercase()).or_insert(2);
            loop {
                name = format!("{base}-{n}");
                *n += 1;
                if !env_taken.contains(&name.to_ascii_lowercase()) {
                    break;
                }
            }
        }
        env_taken.insert(name.to_ascii_lowercase());
        save_environment(
            root,
            &Environment {
                name: name.clone(),
                vars: env.vars.clone(),
                secrets: env.secrets.clone(),
            },
        )?;
        report.environments += 1;
        if name != wanted {
            report.renamed.push((wanted.to_string(), name));
        }
    }

    write_items(root, folder_rel, &result.items, &mut taken, &mut report)?;
    Ok(report)
}

/// `folder_rel` must be plain components naming an existing folder inside the
/// collection; checked before the first file is written.
fn check_folder_rel(root: &Path, folder_rel: &Path) -> Result<(), ImportError> {
    let bad =
        |why: &str| ImportError::Folder(format!("import folder `{}` {why}", folder_rel.display()));
    if !folder_rel
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err(bad("is not a relative path inside the collection"));
    }
    let rootc = root.canonicalize().map_err(|_| bad("cannot be resolved"))?;
    let abs = rootc.join(folder_rel);
    let c = abs.canonicalize().map_err(|_| bad("does not exist"))?;
    if !c.starts_with(&rootc) || !c.is_dir() {
        return Err(bad("is not a folder inside the collection"));
    }
    Ok(())
}

fn node_name(n: &Node) -> String {
    match n {
        Node::Folder { name, .. } => name.clone(),
        Node::Request(m) => m.name.clone(),
        Node::Invalid { path, .. } => path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

fn fold(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Longest file or folder name stem, in bytes; leaves room for a numeric
/// suffix and an extension under the 255-byte limit.
const MAX_NAME_BYTES: usize = 80;

/// A name the collection will accept: no control characters or leading dots,
/// not empty, not a reserved device name, and not unreasonably long.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).collect();
    let s: String = s
        .trim()
        .trim_start_matches('.')
        .trim()
        .chars()
        .scan(0usize, |n, c| {
            *n += c.len_utf8();
            (*n <= MAX_NAME_BYTES).then_some(c)
        })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        return "Untitled".into();
    }
    if crate::fs::is_reserved_device_name(&s) || crate::fs::is_reserved_device_name(&slug(&s)) {
        return format!("Imported {s}");
    }
    s
}

/// Names in use within one folder, and the next suffix to try for each base,
/// so n equal names cost O(n) rather than O(n^2).
#[derive(Default)]
struct Names {
    taken: HashSet<String>,
    next: std::collections::HashMap<String, usize>,
    hints: StemHints,
}

fn unique(name: &str, names: &mut Names) -> String {
    let base = clean_name(name);
    let mut out = base.clone();
    if names.taken.contains(&fold(&out)) {
        let n = names.next.entry(fold(&base)).or_insert(2);
        loop {
            out = format!("{base} {n}");
            *n += 1;
            if !names.taken.contains(&fold(&out)) {
                break;
            }
        }
    }
    names.taken.insert(fold(&out));
    out
}

fn write_items(
    root: &Path,
    dir: &Path,
    items: &[ImportItem],
    names: &mut Names,
    report: &mut WriteReport,
) -> Result<Vec<String>, ImportError> {
    let mut stems = Vec::new();
    for item in items {
        match item {
            ImportItem::Folder { name, items } => {
                let used = unique(name, names);
                if used != *name {
                    report.renamed.push((name.clone(), used.clone()));
                }
                let rel = create_folder_hinted(root, dir, &used, &mut names.hints)?;
                report.folders += 1;
                let mut inner = Names::default();
                let order = write_items(root, &rel, items, &mut inner, report)?;
                save_folder_defaults(
                    &root.join(&rel),
                    &FolderDefaults {
                        order,
                        ..FolderDefaults::default()
                    },
                )?;
                stems.push(stem(&rel));
            }
            ImportItem::Request { name, request } => {
                let used = unique(name, names);
                if used != *name {
                    report.renamed.push((name.clone(), used.clone()));
                }
                let rel = create_request_hinted(root, dir, &used, &mut names.hints)?;
                let mut req = request.clone();
                req.name = used;
                save_request(&root.join(&rel), &req)?;
                report.requests += 1;
                stems.push(stem(&rel));
            }
        }
    }
    Ok(stems)
}

fn stem(rel: &Path) -> String {
    rel.file_stem()
        .or_else(|| rel.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  .hidden "), "hidden");
        assert_eq!(clean_name(""), "Untitled");
        assert_eq!(clean_name("con"), "Imported con");
        assert_eq!(clean_name("a\u{0}b"), "ab");
        assert_eq!(clean_name(&"x".repeat(200)).len(), 80);
    }

    #[test]
    fn collisions_get_suffixes() {
        let mut t = Names::default();
        assert_eq!(unique("A", &mut t), "A");
        assert_eq!(unique("a", &mut t), "a 2");
        assert_eq!(unique("A", &mut t), "A 3");
    }

    #[test]
    fn many_same_named_environments_are_numbered_quickly() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("collection.toml"), "format = 1\n").unwrap();
        let mut r = ImportResult::default();
        for _ in 0..1500 {
            r.environments.push(super::super::ImportedEnv {
                name: "a".into(),
                vars: Vec::new(),
                secrets: Vec::new(),
            });
        }
        let start = std::time::Instant::now();
        let report = write(d.path(), Path::new(""), &r, "imported").unwrap();
        assert_eq!(report.environments, 1500);
        assert_eq!(report.renamed.last().unwrap().1, "a-1500");
        assert!(start.elapsed().as_secs() < 20);
    }

    #[test]
    fn names_are_byte_capped_on_char_boundaries() {
        for unit in ["é", "😀", "日"] {
            let n = clean_name(&unit.repeat(200));
            assert!(n.len() <= MAX_NAME_BYTES, "{unit}");
            assert!(n.chars().all(|c| unit.starts_with(c)));
            assert!(!n.is_empty());
        }
    }

    #[test]
    fn multibyte_names_write_without_error() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("collection.toml"), "format = 1\n").unwrap();
        let mut r = ImportResult::default();
        r.items.push(ImportItem::Folder {
            name: "😀".repeat(200),
            items: vec![ImportItem::Request {
                name: "é".repeat(200),
                request: Default::default(),
            }],
        });
        write(d.path(), Path::new(""), &r, "imported").unwrap();
    }
}
