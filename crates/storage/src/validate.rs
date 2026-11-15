//! Tolerant, read-only collection validator: lists every problem instead of stopping at the first.

use std::path::{Path, PathBuf};

use wisp_core::assert::{Assertion, Expected};

use crate::collection::{COLLECTION_FILE, ENV_DIR, FOLDER_FILE, MAX_FOLDERS};
use crate::{STATE_DIR, StorageError, codec, collection, environment, fs, request};

/// Most assertions one request may carry before a warning.
const MAX_ASSERTIONS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One problem. `path` is relative to the collection root; messages never carry file values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: PathBuf,
    pub key: Option<String>,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// The folder visit cap stopped the walk early.
    pub truncated: bool,
    /// Canonical root, used to make error paths relative.
    root: PathBuf,
}

impl Report {
    fn add(&mut self, path: &Path, key: Option<&str>, message: impl Into<String>, s: Severity) {
        self.findings.push(Finding {
            path: path.to_path_buf(),
            key: key.map(str::to_string),
            message: message.into(),
            severity: s,
        });
    }

    fn error(&mut self, path: &Path, e: &StorageError) {
        let (key, message) = match e {
            StorageError::Io { source, .. } => (None, format!("cannot read: {}", source.kind())),
            StorageError::Parse { message, .. } => (None, message.clone()),
            StorageError::Invalid { key, message, .. } => (Some(key.as_str()), message.clone()),
            StorageError::UnsupportedFormat { found, .. } => {
                (Some("format"), format!("unsupported format {found}"))
            }
            StorageError::NotACollection { .. } => (None, "not a collection".to_string()),
            other => (None, other.to_string()),
        };
        let own = match e {
            StorageError::Io { path, .. }
            | StorageError::Parse { path, .. }
            | StorageError::Invalid { path, .. }
            | StorageError::UnsupportedFormat { path, .. } => path
                .strip_prefix(&self.root)
                .ok()
                .filter(|_| !self.root.as_os_str().is_empty()),
            _ => None,
        };
        let path = own.unwrap_or(path).to_path_buf();
        self.add(&path, key, message, Severity::Error);
    }
}

/// Check a whole collection and return every finding. Reads only; writes nothing.
pub fn validate(root: &Path) -> Report {
    let mut report = Report::default();
    let root = match std::fs::canonicalize(root) {
        Ok(r) => r,
        Err(e) => {
            report.add(
                Path::new(""),
                None,
                format!("cannot read collection root: {}", e.kind()),
                Severity::Error,
            );
            return report;
        }
    };
    report.root = root.clone();
    if !root.join(COLLECTION_FILE).is_file() {
        report.error(
            Path::new(COLLECTION_FILE),
            &StorageError::NotACollection { path: root.clone() },
        );
    } else if let Err(e) = collection::load_collection_defaults(&root) {
        report.error(Path::new(COLLECTION_FILE), &e);
    }
    let mut ancestors = vec![root.clone()];
    walk(&root, Path::new(""), &mut ancestors, &mut 0, &mut report);
    environments(&root, &mut report);
    report
}

fn walk(
    root: &Path,
    rel: &Path,
    ancestors: &mut Vec<PathBuf>,
    visited: &mut usize,
    report: &mut Report,
) {
    let dir = root.join(rel);
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            report.error(
                rel,
                &StorageError::Io {
                    path: dir,
                    source: e,
                },
            );
            return;
        }
    };
    let mut names: Vec<String> = Vec::new();
    for entry in read {
        match entry {
            Ok(en) => {
                if let Ok(n) = en.file_name().into_string() {
                    names.push(n);
                }
            }
            Err(e) => report.error(
                rel,
                &StorageError::Io {
                    path: dir.clone(),
                    source: e,
                },
            ),
        }
    }
    names.sort();
    for name in names {
        if name.starts_with('.') {
            continue;
        }
        let child_rel = rel.join(&name);
        let abs = root.join(&child_rel);
        if std::fs::symlink_metadata(&abs).is_ok_and(|m| m.file_type().is_symlink()) {
            match std::fs::canonicalize(&abs) {
                Ok(c) if c.starts_with(root) => {}
                Ok(_) => {
                    report.add(
                        &child_rel,
                        None,
                        "symbolic link leaves the collection",
                        Severity::Error,
                    );
                    continue;
                }
                Err(_) => {
                    report.add(&child_rel, None, "broken symbolic link", Severity::Error);
                    continue;
                }
            }
        }
        if abs.is_dir() {
            if rel.as_os_str().is_empty() && (name == ENV_DIR || name == STATE_DIR) {
                continue;
            }
            let Ok(canon) = std::fs::canonicalize(&abs) else {
                continue;
            };
            if ancestors.contains(&canon) {
                continue;
            }
            *visited += 1;
            if *visited > MAX_FOLDERS {
                report.truncated = true;
                report.add(
                    Path::new(""),
                    None,
                    format!("more than {MAX_FOLDERS} folders; the check stopped early"),
                    Severity::Error,
                );
                return;
            }
            if let Err(e) = collection::load_folder_defaults(&abs) {
                report.error(&child_rel.join(FOLDER_FILE), &e);
            }
            ancestors.push(canon);
            walk(root, &child_rel, ancestors, visited, report);
            ancestors.pop();
            if report.truncated {
                return;
            }
            continue;
        }
        if !name.ends_with(".toml")
            || name == COLLECTION_FILE
            || name == FOLDER_FILE
            || name.contains(".body.")
        {
            continue;
        }
        match request::load_request(&abs) {
            Ok(req) => assertions(&child_rel, &req.assertions, report),
            Err(e) => report.error(&child_rel, &e),
        }
    }
}

fn assertions(path: &Path, list: &[Assertion], report: &mut Report) {
    if list.len() > MAX_ASSERTIONS {
        report.add(
            path,
            Some("assert"),
            format!(
                "{} assertions; more than {MAX_ASSERTIONS} is unusual",
                list.len()
            ),
            Severity::Warning,
        );
    }
    for (i, a) in list.iter().enumerate() {
        match a {
            Assertion::Unrecognised { reason, .. } if reason == "malformed" => report.add(
                path,
                Some("assert"),
                format!("assertion {} is malformed", i + 1),
                Severity::Error,
            ),
            Assertion::Unrecognised { .. } => report.add(
                path,
                Some("assert"),
                format!("assertion {} is not supported by this version", i + 1),
                Severity::Warning,
            ),
            Assertion::Status { expected, .. }
            | Assertion::Header { expected, .. }
            | Assertion::Json { expected, .. } => {
                if matches!(expected, Expected::Str(s) if s.contains("{{")) {
                    report.add(
                        path,
                        Some("assert"),
                        format!(
                            "assertion {} expected value has `{{{{`; variables are not expanded there",
                            i + 1
                        ),
                        Severity::Warning,
                    );
                }
            }
        }
    }
}

fn environments(root: &Path, report: &mut Report) {
    let names = match environment::list_environments(root) {
        Ok(n) => n,
        Err(e) => {
            report.error(Path::new(ENV_DIR), &e);
            return;
        }
    };
    for name in names {
        let rel = Path::new(ENV_DIR).join(format!("{name}.toml"));
        if let Err(e) = environment::load_environment(root, &name) {
            report.error(&rel, &e);
            continue;
        }
        let abs = root.join(&rel);
        let Ok(text) = fs::read_to_string(&abs) else {
            continue;
        };
        let Ok(doc) = codec::parse(&abs, &text) else {
            continue;
        };
        match crate::vault::read_mode(&doc, &abs) {
            Ok(crate::vault::Mode::File) => {}
            Ok(crate::vault::Mode::Unknown(_)) => report.add(
                &rel,
                Some("secrets"),
                "unknown secret store kind; the app cannot load this environment",
                Severity::Error,
            ),
            Ok(_) => report.add(
                &rel,
                Some("secrets"),
                "secrets are held in a secret store; they were not checked",
                Severity::Warning,
            ),
            Err(e) => report.error(&rel, &e),
        }
    }
}
