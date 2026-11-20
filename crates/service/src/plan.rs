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
    /// The stored request body was cut at the history cap; it must be edited
    /// before the tab can be sent.
    pub request_truncated: bool,
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
    // The text a whole-body clearing emptied, and the text field-level clearing applies to.
    let (whole_text, text) = match &draft.body {
        Body::Json(t) | Body::Text(t) | Body::Xml(t) => {
            (Some(t.content.as_str()), Some(t.content.as_str()))
        }
        Body::GraphQl(g) => (Some(g.query.as_str()), Some(g.variables.as_str())),
        _ => (None, None),
    };
    let unedited = if whole {
        // A GraphQL body has two fields and either one clears as "body".
        let graphql_blank = matches!(&draft.body, Body::GraphQl(g)
            if g.variables.trim().is_empty());
        (graphql_blank || whole_text.is_none_or(|t| t.trim().is_empty()))
            && matches!(
                &draft.body,
                Body::None | Body::Json(_) | Body::Text(_) | Body::Xml(_) | Body::GraphQl(_)
            )
    } else {
        match text.map(|t| empty_json_leaves(t, &fields)) {
            Some(Some(found)) => found,
            Some(None) => true,
            None => false,
        }
    };
    // A GraphQL body also redacts variables leaves, whether or not the query was cleared.
    let leaves_unedited = whole
        && matches!(&draft.body, Body::GraphQl(_))
        && !fields.is_empty()
        && match text.map(|t| empty_json_leaves(t, &fields)) {
            Some(Some(found)) => found,
            Some(None) => true,
            None => false,
        };
    if !unedited && !leaves_unedited {
        return None;
    }
    let what = if whole {
        "the body was not stored".to_string()
    } else {
        format!("{} was not stored", fields.join(", "))
    };
    Some(format!("Edit the request body before resending: {what}"))
}

/// Why a from-history tab may not be sent: its stored request body was cut at
/// the history cap and the draft body is still that cut body. Any change to
/// the body, including switching its type, counts as acknowledgement.
pub fn body_truncation_refusal(draft: &Request, origin: &HistoryOrigin) -> Option<String> {
    (origin.request_truncated && draft.body == origin.stored_body).then(|| {
        "Edit the request body before resending: the stored body was cut at 1 MiB".to_string()
    })
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
    if let Some(message) = origin.and_then(|o| {
        body_redaction_refusal(draft, o).or_else(|| body_truncation_refusal(draft, o))
    }) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::{TextBody, Var};
    use wisp_core::resolve::Recording;
    use wisp_storage::collection::open_collection;

    const URL: &str = "http://example.test/";

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn origin(request: Option<&str>) -> HistoryOrigin {
        HistoryOrigin {
            at_ms: 0,
            body_stored: false,
            body_truncated: false,
            request_truncated: false,
            redacted: Vec::new(),
            adjustments: Vec::new(),
            request: request.map(String::from),
            url: URL.into(),
            stored_body: Body::None,
            had_assertions: false,
        }
    }

    fn env(name: &str, token: &str) -> Environment {
        Environment {
            name: name.into(),
            vars: vec![Var {
                name: "token".into(),
                value: token.into(),
            }],
            secrets: Vec::new(),
        }
    }

    fn auth_collection() -> (tempfile::TempDir, CollectionTree) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(r, "collection.toml", "format = 1\n");
        write(
            r,
            "users/folder.toml",
            "[auth]\ntype = \"bearer\"\ntoken = \"{{token}}\"\n",
        );
        write(
            r,
            "users/get.toml",
            "name = \"Get\"\nmethod = \"GET\"\nurl = \"http://example.test/\"\n",
        );
        let tree = open_collection(r).unwrap();
        (d, tree)
    }

    fn plan(
        tree: &CollectionTree,
        draft: &Request,
        o: &HistoryOrigin,
        e: &Environment,
    ) -> SendPlan {
        prepare(
            &SendInput {
                draft,
                tab_path: None,
                origin: Some(o),
                tree: Some(tree),
                empty_defaults: &CollectionDefaults::default(),
                env: Some(e),
            },
            &mut Recording::new(),
        )
        .unwrap()
    }

    fn draft() -> Request {
        Request {
            url: URL.into(),
            ..Request::default()
        }
    }

    fn header<'a>(p: &'a SendPlan, name: &str) -> Option<&'a str> {
        p.prepared
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    fn body_origin(body: Body, redacted: &[&str]) -> HistoryOrigin {
        let mut o = origin(None);
        o.stored_body = body;
        o.redacted = redacted.iter().map(|s| s.to_string()).collect();
        o
    }

    #[test]
    fn graphql_whole_and_leaf_redaction_refuses_empty_leaf() {
        let gql = |q: &str, v: &str| {
            Body::GraphQl(wisp_core::request::GraphQlBody {
                query: q.into(),
                variables: v.into(),
            })
        };
        let o = body_origin(gql("", ""), &["body", "body.password"]);
        let draft = Request {
            body: gql("{ a }", r#"{"password":""}"#),
            ..Request::default()
        };
        assert!(body_redaction_refusal(&draft, &o).is_some());
        let filled = Request {
            body: gql("{ a }", r#"{"password":"x"}"#),
            ..Request::default()
        };
        assert!(body_redaction_refusal(&filled, &o).is_none());
    }

    fn try_send(draft: &Request, o: &HistoryOrigin) -> Result<SendPlan, ResolveError> {
        prepare(
            &SendInput {
                draft,
                tab_path: None,
                origin: Some(o),
                tree: None,
                empty_defaults: &CollectionDefaults::default(),
                env: None,
            },
            &mut Recording::new(),
        )
    }

    fn refusal(e: ResolveError) -> String {
        match e.problems.as_slice() {
            [Problem::Syntax { message, .. }] => message.clone(),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn json(text: &str) -> Body {
        Body::Json(TextBody {
            content: text.into(),
            ..TextBody::default()
        })
    }

    #[test]
    fn redacted_json_leaf_body_is_refused_until_edited() {
        let stored = json(r#"{"name":"x","webhook_secret":""}"#);
        let o = body_origin(stored.clone(), &["body.webhook_secret"]);
        let mut d = draft();
        d.body = stored;
        let msg = refusal(try_send(&d, &o).err().expect("refused"));
        assert_eq!(
            msg,
            "Edit the request body before resending: webhook_secret was not stored"
        );
        d.body = json(r#"{"name":"x","webhook_secret":"s"}"#);
        assert!(try_send(&d, &o).is_ok());
    }

    #[test]
    fn deeply_nested_body_is_refused_without_overflow() {
        let o = body_origin(json("{}"), &["body.s"]);
        let mut d = draft();
        d.body = json(&format!("{}1{}", "[".repeat(200), "]".repeat(200)));
        assert!(try_send(&d, &o).is_err());
        d.body = json(&"{\"a\":".repeat(50_000));
        assert!(try_send(&d, &o).is_err());
    }

    #[test]
    fn long_key_with_many_empty_leaves_is_fast() {
        let o = body_origin(json("{}"), &["body.s"]);
        let mut d = draft();
        let mut t = format!("{{\"{}\":{{", "k".repeat(400_000));
        for i in 0..100_000 {
            t.push_str(&format!("\"{i}\":\"\","));
        }
        t.push_str("\"z\":\"\"}}");
        d.body = json(&t);
        let start = std::time::Instant::now();
        assert!(try_send(&d, &o).is_ok());
        assert!(start.elapsed().as_secs_f32() < 1.0);
    }

    #[test]
    fn dropped_body_is_refused_until_edited() {
        let stored = Body::Xml(TextBody::default());
        let o = body_origin(stored.clone(), &["body"]);
        let mut d = draft();
        d.body = stored;
        let msg = refusal(try_send(&d, &o).err().expect("refused"));
        assert!(msg.ends_with("the body was not stored"));
        d.body = Body::Xml(TextBody {
            content: "<a/>".into(),
            ..TextBody::default()
        });
        assert!(try_send(&d, &o).is_ok());
    }

    #[test]
    fn redaction_refusal_follows_the_leaf_not_the_text() {
        let o = body_origin(json("{}"), &["body.webhook_secret"]);
        let mut d = draft();
        let cases = [
            ("{\n  \"name\": \"x\",\n  \"webhook_secret\": \"\"\n}", true),
            (" {\"name\":\"x\",\"webhook_secret\":\"\"}\n", true),
            ("{\"a\":{{id}},\"webhook_secret\":\"\"}", true),
            ("{\"webhook_secret\":", true),
            ("{\"webhook_secret\":\"s\"}", false),
            ("{\"webhook_secret\":\"{{secret}}\"}", false),
            ("{\"webhook_secret\":{{secret}}}", false),
            ("{\"name\":\"x\"}", false),
        ];
        for (body, refused) in cases {
            d.body = json(body);
            let got = try_send(&d, &o).err().is_some_and(|e| {
                matches!(e.problems.as_slice(),
                    [Problem::Syntax { message, .. }] if message.starts_with("Edit the request body"))
            });
            assert_eq!(got, refused, "{body}");
        }
    }

    #[test]
    fn dropped_body_whitespace_is_still_refused() {
        let o = body_origin(Body::Xml(TextBody::default()), &["body"]);
        let mut d = draft();
        d.body = Body::Xml(TextBody {
            content: " \n".into(),
            ..TextBody::default()
        });
        assert!(try_send(&d, &o).is_err());
    }

    fn form(v: &str) -> Body {
        Body::Form(vec![wisp_core::request::KeyValue {
            name: "k".into(),
            value: v.into(),
            enabled: true,
        }])
    }

    fn cut(body: Body) -> HistoryOrigin {
        let mut o = body_origin(body, &[]);
        o.request_truncated = true;
        o
    }

    #[test]
    fn truncated_text_and_form_bodies_are_refused_until_edited() {
        for stored in [
            Body::Text(TextBody {
                content: "abc".into(),
                ..TextBody::default()
            }),
            form("abc"),
        ] {
            let o = cut(stored.clone());
            let mut d = draft();
            d.body = stored;
            let msg = refusal(try_send(&d, &o).err().expect("refused"));
            assert!(msg.starts_with("Edit the request body before resending:"));
            assert!(msg.contains("cut at 1 MiB"));
        }
    }

    #[test]
    fn truncated_body_is_sendable_after_any_change() {
        let stored = json("{}");
        let o = cut(stored);
        let mut d = draft();
        d.body = json("{ }");
        assert!(try_send(&d, &o).is_ok());
        d.body = Body::None;
        assert!(try_send(&d, &o).is_ok());
        d.body = form("x");
        assert!(try_send(&d, &o).is_ok());
    }

    #[test]
    fn truncated_refusal_blocks_submission_for_a_json_body() {
        let stored = json("{\"a\":1");
        let o = cut(stored.clone());
        let mut d = draft();
        d.body = stored;
        let e = try_send(&d, &o).err().expect("refused");
        assert!(matches!(
            e.problems.as_slice(),
            [Problem::Syntax {
                location: Location::Body,
                ..
            }]
        ));
    }

    #[test]
    fn truncated_flag_false_does_not_refuse_and_combined_case_is_refused() {
        let stored = json("{}");
        let mut o = cut(stored.clone());
        o.request_truncated = false;
        let mut d = draft();
        d.body = stored.clone();
        assert!(try_send(&d, &o).is_ok());
        let mut o = body_origin(Body::Xml(TextBody::default()), &["body"]);
        o.request_truncated = true;
        d.body = Body::Xml(TextBody::default());
        assert!(try_send(&d, &o).is_err());
    }

    #[test]
    fn entry_without_body_redaction_is_not_refused() {
        let o = body_origin(Body::None, &["Authorization"]);
        assert!(try_send(&draft(), &o).is_ok());
    }

    #[test]
    fn folder_bearer_token_follows_the_environment() {
        let (_d, tree) = auth_collection();
        let o = origin(Some("users/get.toml"));
        let dev = plan(&tree, &draft(), &o, &env("dev", "dev-tok"));
        let prod = plan(&tree, &draft(), &o, &env("prod", "prod-tok"));
        assert_eq!(header(&dev, "authorization"), Some("Bearer dev-tok"));
        assert_eq!(header(&prod, "authorization"), Some("Bearer prod-tok"));
        assert_eq!(dev.resend.unwrap().env.as_deref(), Some("dev"));
        assert_eq!(prod.resend.unwrap().env.as_deref(), Some("prod"));
    }

    #[test]
    fn edited_url_drops_the_folder_scope() {
        let (_d, tree) = auth_collection();
        let mut d = draft();
        d.url = "http://other.test/".into();
        let p = plan(
            &tree,
            &d,
            &origin(Some("users/get.toml")),
            &env("prod", "prod-tok"),
        );
        assert_eq!(header(&p, "authorization"), None);
        assert_eq!(p.scope_path, None);
        assert!(p.resend.unwrap().url_edited);
    }

    #[test]
    fn missing_original_falls_back_to_collection_defaults() {
        let (d, _) = auth_collection();
        std::fs::remove_file(d.path().join("users/get.toml")).unwrap();
        let tree = open_collection(d.path()).unwrap();
        let p = plan(
            &tree,
            &draft(),
            &origin(Some("users/get.toml")),
            &env("dev", "t"),
        );
        assert_eq!(header(&p, "authorization"), None);
        assert!(p.resend.unwrap().original_missing);
        // A tab with its own path is unaffected.
        let own = scope_path_for(&tree, Some(Path::new("a.toml")), Some(&origin(None)), URL);
        assert_eq!(own, (Some(PathBuf::from("a.toml")), false, false));
    }

    #[test]
    fn resend_chain_keeps_scope() {
        let (_d, tree) = auth_collection();
        let first = plan(
            &tree,
            &draft(),
            &origin(Some("users/get.toml")),
            &env("dev", "t"),
        );
        let recorded = recorded_request(first.scope_path.as_deref());
        assert_eq!(recorded.as_deref(), Some("users/get.toml"));
        let again = plan(
            &tree,
            &draft(),
            &origin(recorded.as_deref()),
            &env("prod", "p"),
        );
        assert_eq!(header(&again, "authorization"), Some("Bearer p"));
    }
}
