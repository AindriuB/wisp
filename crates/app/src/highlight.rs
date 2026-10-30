//! Span classification for URL and text inputs.

use std::ops::Range;

use wisp_core::request::Request;
use wisp_core::resolve::{Scope, VarLookup, VarSource, lookup_var};
use wisp_core::template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarState {
    Resolved,
    Dynamic,
    Unresolved,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanKind {
    Text,
    Var { name: String, state: VarState },
    PathParam { name: String, filled: bool },
    Query,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: SpanKind,
}

pub fn url_spans(text: &str, request: &Request, scope: &Scope) -> Vec<Span> {
    let qs = template::query_start(text);
    let mut marks: Vec<Span> = var_marks(text, request, scope);
    for p in template::path_params(text) {
        let filled = request
            .path
            .iter()
            .any(|v| v.name == p.name && !v.value.is_empty());
        marks.push(Span {
            range: p.range,
            kind: SpanKind::PathParam {
                name: p.name,
                filled,
            },
        });
    }
    marks.sort_by_key(|m| m.range.start);
    fill(text, marks, qs)
}

pub fn text_spans(text: &str, request: &Request, scope: &Scope) -> Vec<Span> {
    fill(text, var_marks(text, request, scope), None)
}

fn var_marks(text: &str, request: &Request, scope: &Scope) -> Vec<Span> {
    template::var_refs(text)
        .into_iter()
        .map(|r| {
            let state = if r.terminated {
                match lookup_var(request, scope, &r.name) {
                    VarLookup::Found { .. } => VarState::Resolved,
                    VarLookup::Dynamic => VarState::Dynamic,
                    VarLookup::Unresolved => VarState::Unresolved,
                    VarLookup::Error(_) => VarState::Error,
                }
            } else {
                VarState::Error
            };
            Span {
                range: r.range,
                kind: SpanKind::Var {
                    name: r.name,
                    state,
                },
            }
        })
        .collect()
}

/// Fill the gaps between sorted marks with `Text`, or `Query` from `qs` onward.
fn fill(text: &str, marks: Vec<Span>, qs: Option<usize>) -> Vec<Span> {
    let mut out = Vec::new();
    let gap = |out: &mut Vec<Span>, from: usize, to: usize| {
        if from >= to {
            return;
        }
        match qs {
            Some(q) if q < to => {
                if from < q {
                    out.push(Span {
                        range: from..q,
                        kind: SpanKind::Text,
                    });
                }
                out.push(Span {
                    range: from.max(q)..to,
                    kind: SpanKind::Query,
                });
            }
            _ => out.push(Span {
                range: from..to,
                kind: SpanKind::Text,
            }),
        }
    };
    let mut pos = 0;
    for m in marks {
        if m.range.start < pos {
            continue;
        }
        gap(&mut out, pos, m.range.start);
        pos = m.range.end;
        out.push(m);
    }
    gap(&mut out, pos, text.len());
    out
}

const HOVER_MAX: usize = 200;

pub fn hover_text(name: &str, request: &Request, scope: &Scope, folders: &[String]) -> String {
    match lookup_var(request, scope, name) {
        VarLookup::Found {
            value,
            source,
            secret,
        } => {
            let env = scope.environment.map_or("", |e| e.name.as_str());
            let src = match source {
                VarSource::Request => "request".to_string(),
                VarSource::Folder(i) => match folders.get(i) {
                    Some(f) => format!("folder {f}"),
                    None => "folder".to_string(),
                },
                VarSource::Environment => format!("environment {env}"),
                VarSource::Secret => format!("secret {env}"),
                VarSource::Collection => "collection".to_string(),
            };
            let shown = if secret {
                "••••".to_string()
            } else if value.chars().count() > HOVER_MAX {
                let cut: String = value.chars().take(HOVER_MAX).collect();
                format!("{cut}…")
            } else {
                value
            };
            let src = if secret && !matches!(source, VarSource::Secret) {
                format!("{src} (uses a secret)")
            } else {
                src
            };
            format!("{name} = {shown} · {src}")
        }
        VarLookup::Dynamic => format!("{name} · generated per send"),
        VarLookup::Unresolved => format!("{name} · not defined — blocks sending"),
        VarLookup::Error(_) => format!("{name} · cannot be expanded"),
    }
}

pub fn unresolved_names(text: &str, request: &Request, scope: &Scope) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in text_spans(text, request, scope) {
        if let SpanKind::Var {
            name,
            state: VarState::Unresolved,
        } = s.kind
            && !out.contains(&name)
        {
            out.push(name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::{CollectionDefaults, Environment, FolderDefaults, Var};

    fn var(n: &str, v: &str) -> Var {
        Var {
            name: n.into(),
            value: v.into(),
        }
    }

    fn check_cover(text: &str, spans: &[Span]) {
        let mut pos = 0;
        for s in spans {
            assert_eq!(s.range.start, pos, "{text:?}");
            assert!(s.range.end > s.range.start);
            assert!(text.is_char_boundary(s.range.start) && text.is_char_boundary(s.range.end));
            pos = s.range.end;
        }
        assert_eq!(pos, text.len(), "{text:?}");
    }

    const INPUTS: &[&str] = &[
        "",
        "plain",
        "{{a}}",
        "{{a}}/x?q={{b}}&z=é",
        "http://h/{id}/é{{a}}?x=1#f",
        "héllo {{ünï}} wörld {{",
        "{{a",
        "http://{{host}}/u/{id}/{{$uuid}}?a={b}",
        "日本語/{名前}?日本={{x}}",
        "}}{{}}{",
    ];

    #[test]
    fn coverage_property() {
        let col = CollectionDefaults::default();
        let scope = Scope {
            collection: &col,
            folders: vec![],
            environment: None,
        };
        let req = Request::default();
        for t in INPUTS {
            check_cover(t, &url_spans(t, &req, &scope));
            check_cover(t, &text_spans(t, &req, &scope));
        }
    }

    #[test]
    fn url_kinds() {
        let mut col = CollectionDefaults::default();
        col.vars.push(var("host", "h"));
        let scope = Scope {
            collection: &col,
            folders: vec![],
            environment: None,
        };
        let mut req = Request::default();
        req.path.push(var("id", "7"));
        let t = "http://{{host}}/u/{id}/{n}?a={{gone}}&b=1{{";
        let sp = url_spans(t, &req, &scope);
        check_cover(t, &sp);
        let kinds: Vec<&SpanKind> = sp.iter().map(|s| &s.kind).collect();
        assert!(matches!(
            kinds[1],
            SpanKind::Var {
                state: VarState::Resolved,
                ..
            }
        ));
        assert!(kinds.contains(&&SpanKind::PathParam {
            name: "id".into(),
            filled: true
        }));
        assert!(kinds.contains(&&SpanKind::PathParam {
            name: "n".into(),
            filled: false
        }));
        assert!(kinds.iter().any(
            |k| matches!(k, SpanKind::Var { name, state: VarState::Unresolved } if name == "gone")
        ));
        assert!(matches!(
            kinds.last().unwrap(),
            SpanKind::Var {
                state: VarState::Error,
                ..
            }
        ));
        assert!(kinds.contains(&&SpanKind::Query));
        let q = sp.iter().find(|s| s.kind == SpanKind::Query).unwrap();
        assert_eq!(&t[q.range.clone()], "?a=");
        let d = url_spans("{{$uuid}}", &req, &scope);
        assert!(matches!(
            d[0].kind,
            SpanKind::Var {
                state: VarState::Dynamic,
                ..
            }
        ));
    }

    #[test]
    fn text_has_only_vars() {
        let col = CollectionDefaults::default();
        let scope = Scope {
            collection: &col,
            folders: vec![],
            environment: None,
        };
        let sp = text_spans("/{id}?a={{x}}", &Request::default(), &scope);
        assert_eq!(sp.len(), 2);
        assert_eq!(sp[0].kind, SpanKind::Text);
    }

    #[test]
    fn unresolved_distinct_in_order() {
        let col = CollectionDefaults::default();
        let scope = Scope {
            collection: &col,
            folders: vec![],
            environment: None,
        };
        let n = unresolved_names("{{b}}{{a}}{{b}}{{$uuid}}", &Request::default(), &scope);
        assert_eq!(n, vec!["b", "a"]);
    }

    #[test]
    fn hover() {
        let mut col = CollectionDefaults::default();
        col.vars.push(var("c", "cv"));
        col.vars.push(var("long", &"x".repeat(300)));
        let mut folder = FolderDefaults::default();
        folder.vars.push(var("userId", "42"));
        let env = Environment {
            name: "dev".into(),
            vars: vec![var("e", "ev")],
            secrets: vec![var("tok", "s3cret-value")],
        };
        let scope = Scope {
            collection: &col,
            folders: vec![&folder],
            environment: Some(&env),
        };
        let mut req = Request::default();
        req.vars.push(var("r", "rv"));
        let f = vec!["users".to_string()];
        assert_eq!(
            hover_text("userId", &req, &scope, &f),
            "userId = 42 · folder users"
        );
        assert_eq!(hover_text("r", &req, &scope, &f), "r = rv · request");
        assert_eq!(
            hover_text("e", &req, &scope, &f),
            "e = ev · environment dev"
        );
        assert_eq!(hover_text("c", &req, &scope, &f), "c = cv · collection");
        let sec = hover_text("tok", &req, &scope, &f);
        assert_eq!(sec, "tok = •••• · secret dev");
        assert!(!sec.contains("s3cret"));
        assert_eq!(
            hover_text("$uuid", &req, &scope, &f),
            "$uuid · generated per send"
        );
        assert_eq!(
            hover_text("zz", &req, &scope, &f),
            "zz · not defined — blocks sending"
        );
        let l = hover_text("long", &req, &scope, &f);
        assert!(l.contains(&format!("{}…", "x".repeat(200))));
        assert!(!l.contains(&"x".repeat(201)));
    }

    #[test]
    fn hover_indirect_secret_is_masked() {
        let mut col = CollectionDefaults::default();
        col.vars.push(var("ac", "Bearer {{tok}}"));
        let mut folder = FolderDefaults::default();
        folder.vars.push(var("af", "Bearer {{tok}}"));
        let env = Environment {
            name: "dev".into(),
            vars: vec![var("ae", "Bearer {{tok}}")],
            secrets: vec![var("tok", "s3cret-value")],
        };
        let scope = Scope {
            collection: &col,
            folders: vec![&folder],
            environment: Some(&env),
        };
        let mut req = Request::default();
        req.vars.push(var("ar", "Bearer {{tok}}"));
        let f = vec!["users".to_string()];
        for (name, src) in [
            ("ar", "request"),
            ("af", "folder users"),
            ("ae", "environment dev"),
            ("ac", "collection"),
        ] {
            let h = hover_text(name, &req, &scope, &f);
            assert_eq!(h, format!("{name} = •••• · {src} (uses a secret)"));
            assert!(!h.contains("s3cret"), "{h}");
        }
    }
}
