//! Span classification for URL and text inputs.

use std::ops::Range;

use wisp_core::request::Request;
use wisp_core::resolve::Scope;

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

pub fn url_spans(text: &str, _request: &Request, _scope: &Scope) -> Vec<Span> {
    text_spans(text, _request, _scope)
}

pub fn text_spans(text: &str, _request: &Request, _scope: &Scope) -> Vec<Span> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![Span {
        range: 0..text.len(),
        kind: SpanKind::Text,
    }]
}

pub fn hover_text(_name: &str, _request: &Request, _scope: &Scope, _folders: &[String]) -> String {
    String::new()
}

pub fn unresolved_names(_text: &str, _request: &Request, _scope: &Scope) -> Vec<String> {
    Vec::new()
}
