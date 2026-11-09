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
use crate::ops::{create_folder, create_request, slug};
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
    let mut taken: HashSet<String> = existing.iter().map(|n| fold(&node_name(n))).collect();

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
        let mut name = base.clone();
        let mut n = 2;
        while env_taken.contains(&name.to_ascii_lowercase()) {
            name = format!("{base}-{n}");
            n += 1;
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

/// A name the collection will accept: no control characters or leading dots,
/// not empty, not a reserved device name, and not unreasonably long.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).collect();
    let s: String = s
        .trim()
        .trim_start_matches('.')
        .trim()
        .chars()
        .take(80)
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

fn unique(name: &str, taken: &mut HashSet<String>) -> String {
    let base = clean_name(name);
    let mut out = base.clone();
    let mut n = 2;
    while taken.contains(&fold(&out)) {
        out = format!("{base} {n}");
        n += 1;
    }
    taken.insert(fold(&out));
    out
}

fn write_items(
    root: &Path,
    dir: &Path,
    items: &[ImportItem],
    taken: &mut HashSet<String>,
    report: &mut WriteReport,
) -> Result<Vec<String>, ImportError> {
    let mut stems = Vec::new();
    for item in items {
        match item {
            ImportItem::Folder { name, items } => {
                let used = unique(name, taken);
                if used != *name {
                    report.renamed.push((name.clone(), used.clone()));
                }
                let rel = create_folder(root, dir, &used)?;
                report.folders += 1;
                let mut inner = HashSet::new();
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
                let used = unique(name, taken);
                if used != *name {
                    report.renamed.push((name.clone(), used.clone()));
                }
                let rel = create_request(root, dir, &used)?;
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
        let mut t = HashSet::new();
        assert_eq!(unique("A", &mut t), "A");
        assert_eq!(unique("a", &mut t), "a 2");
        assert_eq!(unique("A", &mut t), "A 3");
    }
}
