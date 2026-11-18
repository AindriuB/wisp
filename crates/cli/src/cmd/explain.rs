//! `explain`: the resolution report of one request, mapped to DTOs. No secret value
//! is ever loaded: file secrets are replaced by a placeholder before the scope is built
//! and keychain-backed environments are not opened at all.

use std::path::{Component, Path, PathBuf};

use wisp_core::resolve::{
    AuthApplication, HideReason, Origin, ReportWarning, ResolutionReport, SettingSource, Shown,
    ShownPart, Synthetic, explain,
};
use wisp_core::resolve::{Location, ParamKind, Problem, Setting};
use wisp_storage::collection::open_collection;
use wisp_storage::environment::{
    SecretsMode, environment_info, list_environments, load_environment,
};
use wisp_storage::request::load_request;

use super::inspect::{Failure, cap, rel};
use crate::dto::{
    AuthDoc, AuthFieldDoc, BodyDoc, EnvDoc, ExplainDoc, HeaderDoc, ParamDoc, PartDoc, ProblemDoc,
    SCHEMA, SecretDoc, SettingDoc, VarDoc,
};

/// Stands in for every secret value before the scope is built.
const PLACEHOLDER: &str = "<secret>";
const KEYCHAIN_NOTE: &str = "keychain: not loaded by the CLI";

fn unreadable(e: impl std::fmt::Display) -> Failure {
    Failure::Unreadable(e.to_string())
}

/// `rel` must be relative, free of `..`, and name a regular file with no symlink on the way.
fn confined(root: &Path, request: &str) -> Result<PathBuf, Failure> {
    let rel = Path::new(request);
    if request.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(unreadable(
            "request path must be relative to the collection",
        ));
    }
    let mut cur = root.to_path_buf();
    let n = rel.components().count();
    for (i, c) in rel.components().enumerate() {
        cur.push(c);
        let m = std::fs::symlink_metadata(&cur).map_err(|_| unreadable("request not found"))?;
        let ok = !m.file_type().is_symlink() && if i + 1 == n { m.is_file() } else { m.is_dir() };
        if !ok {
            return Err(unreadable(
                "request path must be a regular file, no symlinks",
            ));
        }
    }
    Ok(cur)
}

pub fn run(collection: &Path, request: &str, env: Option<&str>) -> Result<ExplainDoc, Failure> {
    let tree = open_collection(collection).map_err(unreadable)?;
    let abs = confined(&tree.root, request)?;
    let req = load_request(&abs).map_err(unreadable)?;
    let rel_path = abs.strip_prefix(&tree.root).unwrap_or(&abs).to_path_buf();

    let mut keychain = None;
    let environment = match env {
        Some(name) => {
            let names = list_environments(&tree.root).map_err(unreadable)?;
            if !names.iter().any(|e| e == name) {
                return Err(Failure::Usage("unknown environment".into()));
            }
            let info = environment_info(&tree.root, name).map_err(unreadable)?;
            if !matches!(info.store, SecretsMode::File) {
                keychain = Some(KEYCHAIN_NOTE);
            }
            let mut e = load_environment(&tree.root, name).map_err(unreadable)?;
            for s in &mut e.secrets {
                if !s.value.is_empty() {
                    s.value = PLACEHOLDER.to_owned();
                }
            }
            // Declared but not loaded (store mode): keep the names as empty
            // secrets so they shadow lower scopes and report as unavailable.
            for n in info.secret_names {
                if !e.secrets.iter().any(|s| s.name == n) {
                    e.secrets.push(wisp_core::request::Var {
                        name: n,
                        value: String::new(),
                    });
                }
            }
            Some(e)
        }
        None => None,
    };
    let scope = tree.scope_for(&rel_path, environment.as_ref());
    let report = explain(&req, &scope);

    // Folder names for `Origin::Folder(i)`, outermost first, as `scope_for` builds them.
    let mut folder_paths: Vec<String> = rel_path
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|p| !p.as_os_str().is_empty() && tree.folders.contains_key(*p))
        .map(rel)
        .collect();
    folder_paths.reverse();

    Ok(map(&report, request, &folder_paths, keychain))
}

fn origin(o: Origin, folders: &[String]) -> String {
    match o {
        Origin::Request => "request".into(),
        Origin::Folder(i) => format!(
            "folder {}",
            folders.get(i).map_or_else(|| i.to_string(), |f| cap(f))
        ),
        Origin::Collection => "collection".into(),
        Origin::Environment => "environment".into(),
        Origin::Secret => "secret".into(),
        Origin::Dynamic => "dynamic".into(),
        Origin::BodyType => "body type".into(),
        Origin::Auth => "auth".into(),
    }
}

fn shown(s: &Shown) -> Vec<PartDoc> {
    s.0.iter()
        .map(|p| match p {
            ShownPart::Text(t) => PartDoc::Text { text: t.clone() },
            ShownPart::Secret { name, resolved } => PartDoc::Secret {
                name: cap(name),
                resolved: *resolved,
            },
            ShownPart::Dynamic { name } => PartDoc::Dynamic { name: cap(name) },
            ShownPart::Hidden(r) => PartDoc::Hidden {
                reason: match r {
                    HideReason::Credential => "credential",
                    HideReason::Userinfo => "userinfo",
                    HideReason::TooLong => "too-long",
                },
            },
        })
        .collect()
}

fn location(l: &Location) -> String {
    match l {
        Location::Url => "url".into(),
        Location::PathParam(n) => format!("path param {}", cap(n)),
        Location::Param(n) => format!("param {}", cap(n)),
        Location::Header(n) => format!("header {}", cap(n)),
        Location::Auth => "auth".into(),
        Location::Body => "body".into(),
        Location::Var(n) => format!("var {}", cap(n)),
    }
}

fn setting(name: &'static str, s: &Setting) -> SettingDoc {
    SettingDoc {
        name,
        value: s.value.clone(),
        source: match s.source {
            SettingSource::Request => "request",
            SettingSource::Default => "default",
        },
    }
}

pub(crate) fn problem(p: &Problem) -> ProblemDoc {
    let doc = |kind, name: Option<&String>, loc: Option<&Location>, message: Option<&String>| {
        ProblemDoc {
            kind,
            name: name.map(|n| cap(n)),
            location: loc.map(location),
            message: message.map(|m| cap(m)),
        }
    };
    match p {
        Problem::UnresolvedVar { name, location } => {
            doc("unresolved-var", Some(name), Some(location), None)
        }
        Problem::MissingPathParam { name } => doc("missing-path-param", Some(name), None, None),
        Problem::Syntax { location, message } => doc("syntax", None, Some(location), Some(message)),
        Problem::VarCycle { name } => doc("var-cycle", Some(name), None, None),
    }
}

fn warning(w: &ReportWarning) -> String {
    match w {
        ReportWarning::PatternMismatch { name, pattern } => {
            format!("{} does not match pattern {}", cap(name), cap(pattern))
        }
        ReportWarning::InvalidPattern {
            name,
            pattern,
            message,
        } => format!(
            "{} has an invalid pattern {}: {}",
            cap(name),
            cap(pattern),
            cap(message)
        ),
    }
}

fn map(
    r: &ResolutionReport,
    request: &str,
    folders: &[String],
    keychain: Option<&'static str>,
) -> ExplainDoc {
    let o = |x: Origin| origin(x, folders);
    let (application, target) = match &r.auth.application {
        AuthApplication::None => ("none", None),
        AuthApplication::Header(s) => ("header", Some(shown(s))),
        AuthApplication::Query(s) => ("query", Some(shown(s))),
        AuthApplication::AtSend => ("at-send", None),
    };
    ExplainDoc {
        schema: SCHEMA,
        command: "explain",
        request: cap(request),
        environment: r.environment.as_ref().map(|e| EnvDoc {
            name: cap(&e.name),
            keychain,
            secrets: e
                .secrets
                .iter()
                .map(|s| SecretDoc {
                    name: cap(&s.name),
                    present: s.present,
                })
                .collect(),
        }),
        method: format!("{:?}", r.method).to_ascii_uppercase(),
        url: shown(&r.url),
        params: r
            .params
            .iter()
            .map(|p| ParamDoc {
                kind: match p.kind {
                    ParamKind::Query => "query",
                    ParamKind::Path => "path",
                },
                name: shown(&p.name),
                value: shown(&p.value),
                origin: o(p.origin),
            })
            .collect(),
        headers: r
            .headers
            .iter()
            .map(|h| HeaderDoc {
                name: shown(&h.name),
                value: shown(&h.value),
                origin: o(h.origin),
                overridden: h.overridden.iter().map(|x| o(*x)).collect(),
                synthetic: h.synthetic.map(|s| match s {
                    Synthetic::BodyType(k) => format!("body type {}", k.label()),
                    Synthetic::Auth(k) => format!("auth {}", k.label()),
                }),
            })
            .collect(),
        auth: AuthDoc {
            kind: r.auth.kind.label(),
            origin: r.auth.origin.map(o),
            application,
            target,
            fields: r
                .auth
                .fields
                .iter()
                .map(|f| AuthFieldDoc {
                    name: f.name,
                    value: shown(&f.value),
                })
                .collect(),
        },
        settings: vec![
            setting("timeout", &r.settings.timeout),
            setting("follow_redirects", &r.settings.follow_redirects),
            setting("verify_tls", &r.settings.verify_tls),
        ],
        body: BodyDoc {
            kind: r.body.kind.label(),
            bytes: r.body.bytes,
            vars: r.body.vars.iter().map(|v| cap(v)).collect(),
        },
        variables: r
            .variables
            .iter()
            .map(|v| VarDoc {
                name: cap(&v.name),
                origin: o(v.origin),
                value: shown(&v.value),
                shadowed: v.shadowed.iter().map(|x| o(*x)).collect(),
                locations: v.locations.iter().map(location).collect(),
            })
            .collect(),
        problems: r.problems.iter().map(problem).collect(),
        warnings: r.warnings.iter().map(warning).collect(),
    }
}
