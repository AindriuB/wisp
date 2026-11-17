//! Send planning: resolve a draft against its collection scope. Pure: everything it reads is passed in.

use std::path::{Path, PathBuf};

use wisp_core::auth::EngineAuth;
use wisp_core::request::{Body, CollectionDefaults, Environment, Request};
use wisp_core::resolve::{
    DynamicSource, DynamicValue, Location, Problem, ResolveError, Scope, resolve_with,
};
use wisp_core::{Command, PreparedRequest};
use wisp_storage::collection::{CollectionTree, Node};

/// What a tab opened from history remembers about its entry.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryOrigin {
    /// Unix milliseconds the entry was recorded.
    pub at_ms: u64,
    pub body_stored: bool,
    pub body_truncated: bool,
    /// Names whose values were not stored; never shown with a value.
    pub redacted: Vec<String>,
    /// What opening the entry removed, disabled or reset so no emptied
    /// credential is sent; shown in the Resend notice.
    pub adjustments: Vec<String>,
    /// The entry's original request path, used only to scope Resend's resolve.
    pub request: Option<String>,
    /// The URL template the tab opened with; the folder scope applies only
    /// while the draft URL still equals it.
    pub url: String,
    /// The body the tab opened with (after sanitising). A redacted body must
    /// differ from it before the tab can be sent.
    pub stored_body: Body,
    /// Whether the recorded run evaluated assertions.
    pub had_assertions: bool,
}

/// Everything `prepare` reads.
pub struct SendInput<'a> {
    pub draft: &'a Request,
    pub tab_path: Option<&'a Path>,
    pub origin: Option<&'a HistoryOrigin>,
    pub tree: Option<&'a CollectionTree>,
    pub empty_defaults: &'a CollectionDefaults,
    pub env: Option<&'a Environment>,
}

/// The facts a Resend notice is made of; the app formats the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResendFacts {
    /// The name of the environment the resend resolved under.
    pub env: Option<String>,
    pub adjustments: Vec<String>,
    pub redacted: Vec<String>,
    pub original_missing: bool,
    pub url_edited: bool,
    pub had_assertions: bool,
}

/// A request ready to send, the scope path it resolved with, the facts of the Resend
/// notice of a from-history tab, and the dynamic values it used.
pub struct SendPlan {
    pub prepared: PreparedRequest,
    pub scope_path: Option<PathBuf>,
    pub resend: Option<ResendFacts>,
    pub dynamics: Vec<DynamicValue>,
}

fn tree_has_request(nodes: &[Node], rel: &Path) -> bool {
    nodes.iter().any(|n| match n {
        Node::Request(m) => m.path == rel,
        Node::Folder { children, .. } => tree_has_request(children, rel),
        Node::Invalid { path, .. } => path == rel,
    })
}

/// A tab's own path wins. A from-history tab has none, so its entry's original
/// request path scopes the resolve, but only while the tab's URL is still the
/// stored one. Flags: the original path is gone; the URL was edited. Either way
/// only the collection defaults apply.
pub fn scope_path_for(
    tree: &CollectionTree,
    tab_path: Option<&Path>,
    origin: Option<&HistoryOrigin>,
    draft_url: &str,
) -> (Option<PathBuf>, bool, bool) {
    if let Some(p) = tab_path {
        return (Some(p.to_path_buf()), false, false);
    }
    let Some(origin) = origin else {
        return (None, false, false);
    };
    let Some(orig) = origin.request.as_deref() else {
        return (None, false, false);
    };
    if origin.url != draft_url {
        return (None, false, true);
    }
    let rel = PathBuf::from(orig);
    if tree_has_request(&tree.nodes, &rel) {
        (Some(rel), false, false)
    } else {
        (None, true, false)
    }
}

/// Whether any of `fields` (dotted paths) names a JSON string leaf that is
/// `""`, or `None` when the text does not scan as JSON or nests past 128. Bare `{{ref}}` values are skipped, as history
/// masks them; arrays do not add to the path.
fn empty_json_leaves(text: &str, fields: &[&str]) -> Option<bool> {
    const MAX_DEPTH: usize = 128;
    struct Scan<'a> {
        b: &'a [u8],
        i: usize,
        fields: &'a [&'a str],
        found: bool,
    }
    impl Scan<'_> {
        fn ws(&mut self) {
            while self.b.get(self.i).is_some_and(u8::is_ascii_whitespace) {
                self.i += 1;
            }
        }
        fn eat(&mut self, c: u8) -> Option<()> {
            self.ws();
            (self.b.get(self.i) == Some(&c)).then(|| self.i += 1)
        }
        fn string(&mut self) -> Option<String> {
            self.eat(b'"')?;
            let start = self.i;
            while let Some(&c) = self.b.get(self.i) {
                match c {
                    b'\\' => self.i += 2,
                    b'"' => {
                        let s = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                        self.i += 1;
                        return Some(s);
                    }
                    _ => self.i += 1,
                }
            }
            None
        }
        fn value(&mut self, path: &mut String, depth: usize) -> Option<()> {
            if depth > MAX_DEPTH {
                return None;
            }
            self.ws();
            match *self.b.get(self.i)? {
                b'{' if self.b[self.i..].starts_with(b"{{") => {
                    let end = self.b[self.i..].windows(2).position(|w| w == b"}}")?;
                    self.i += end + 2;
                }
                b'{' => {
                    self.i += 1;
                    if self.eat(b'}').is_some() {
                        return Some(());
                    }
                    loop {
                        let key = self.string()?;
                        self.eat(b':')?;
                        let keep = path.len();
                        if !path.is_empty() {
                            path.push('.');
                        }
                        path.push_str(&key);
                        self.value(path, depth + 1)?;
                        path.truncate(keep);
                        if self.eat(b',').is_none() {
                            return self.eat(b'}');
                        }
                    }
                }
                b'[' => {
                    self.i += 1;
                    if self.eat(b']').is_some() {
                        return Some(());
                    }
                    loop {
                        self.value(path, depth + 1)?;
                        if self.eat(b',').is_none() {
                            return self.eat(b']');
                        }
                    }
                }
                b'"' => {
                    if self.string()?.is_empty() {
                        self.found |= self.fields.contains(&path.as_str());
                    }
                }
                _ => {
                    let start = self.i;
                    while self
                        .b
                        .get(self.i)
                        .is_some_and(|c| !c.is_ascii_whitespace() && !b",]}".contains(c))
                    {
                        self.i += 1;
                    }
                    if self.i == start {
                        return None;
                    }
                }
            }
            Some(())
        }
    }
    let mut s = Scan {
        b: text.as_bytes(),
        i: 0,
        fields,
        found: false,
    };
    s.value(&mut String::new(), 0)?;
    s.ws();
    (s.i == s.b.len()).then_some(s.found)
}

/// Why a from-history tab may not be sent: a redacted body value has not been
/// filled in since. A redacted JSON leaf is unedited while it is still `""`
/// (a removed or non-empty leaf counts as edited); a dropped body is unedited
/// while it is empty. A body that does not scan is refused. `None` when no
/// body was redacted.
pub fn body_redaction_refusal(draft: &Request, origin: &HistoryOrigin) -> Option<String> {
    let mut whole = false;
    let mut fields: Vec<&str> = Vec::new();
    for n in &origin.redacted {
        if n == "body" {
            whole = true;
        } else if let Some(f) = n.strip_prefix("body.") {
            fields.push(f);
        }
    }
    if !whole && fields.is_empty() {
        return None;
    }
    let text = match &draft.body {
        Body::Json(t) | Body::Text(t) | Body::Xml(t) => Some(t.content.as_str()),
        _ => None,
    };
    let unedited = if whole {
        text.is_none_or(|t| t.trim().is_empty())
            && matches!(
                &draft.body,
                Body::None | Body::Json(_) | Body::Text(_) | Body::Xml(_)
            )
    } else {
        match text.map(|t| empty_json_leaves(t, &fields)) {
            Some(Some(found)) => found,
            Some(None) => true,
            None => false,
        }
    };
    if !unedited {
        return None;
    }
    let what = if whole {
        "the body was not stored".to_string()
    } else {
        format!("{} was not stored", fields.join(", "))
    };
    Some(format!("Edit the request body before resending: {what}"))
}

/// The `AuthorizeStart` for a resolved request, or `None` when its auth is not authorization
/// code. `client` is what Send would use, with `collection` set to the same partition; the
/// environment partitions the cached token.
fn authorize_command(prepared: PreparedRequest, collection: Option<String>) -> Option<Command> {
    let auth = prepared.auth?;
    if !matches!(auth, EngineAuth::OAuth2AuthCode { .. }) {
        return None;
    }
    let mut client = prepared.client;
    client.collection = collection;
    Some(Command::AuthorizeStart {
        auth,
        client,
        environment: prepared.environment,
        timeout: prepared.timeout,
    })
}

/// Resolve a draft for an authorize command, through the same path as Send. `Ok(None)` when the
/// resolved auth is not authorization code. Pure: everything it reads is passed in.
pub fn prepare_authorize(
    input: &SendInput,
    collection: Option<String>,
) -> Result<Option<Command>, ResolveError> {
    let plan = prepare(input, &mut wisp_core::resolve::Fresh)?;
    Ok(authorize_command(plan.prepared, collection))
}

/// Resolve a draft for sending. Pure: everything it reads is passed in. `dynamics` supplies the
/// `{{$uuid}}` and `{{$timestamp}}` values; the plan's `dynamics` starts empty and the caller
/// that passed a `Recording` fills it with the recorded values.
pub fn prepare(
    input: &SendInput,
    dynamics: &mut dyn DynamicSource,
) -> Result<SendPlan, ResolveError> {
    let SendInput {
        draft,
        tab_path,
        origin,
        tree,
        empty_defaults,
        env,
    } = *input;
    let (scope_path, original_missing, url_edited) = match tree {
        Some(t) => scope_path_for(t, tab_path, origin, &draft.url),
        None => (tab_path.map(Path::to_path_buf), false, false),
    };
    let scope = match tree {
        Some(t) => t.scope_for(scope_path.as_deref().unwrap_or(Path::new("")), env),
        None => Scope {
            collection: empty_defaults,
            folders: Vec::new(),
            environment: None,
        },
    };
    if let Some(message) = origin.and_then(|o| body_redaction_refusal(draft, o)) {
        return Err(ResolveError {
            problems: vec![Problem::Syntax {
                location: Location::Body,
                message,
            }],
        });
    }
    let prepared = resolve_with(draft, &scope, dynamics)?;
    let resend = origin.map(|o| ResendFacts {
        env: env.map(|e| e.name.clone()),
        adjustments: o.adjustments.clone(),
        redacted: o.redacted.clone(),
        original_missing,
        url_edited,
        had_assertions: o.had_assertions,
    });
    Ok(SendPlan {
        prepared,
        scope_path,
        resend,
        dynamics: Vec::new(),
    })
}

/// The request path a history entry records: the scope path used to resolve,
/// so resending a from-history entry keeps its folder defaults.
pub fn recorded_request(scope_path: Option<&Path>) -> Option<String> {
    scope_path.map(|p| p.to_string_lossy().into_owned())
}
