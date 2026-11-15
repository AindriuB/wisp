//! `inspect`: a read-only summary of a collection. No value is ever read into output.

use std::path::Path;

use wisp_core::request::Auth;
use wisp_storage::collection::{Node, open_collection, read_collection_id};
use wisp_storage::environment::list_environments;
use wisp_storage::request::load_request;

use crate::dto::{CollectionInfo, InspectDoc, NodeInfo, SCHEMA};

pub const MAX_NODES: usize = 2000;
const MAX_NAME: usize = 200;
const MAX_HEADERS: usize = 100;

pub enum Failure {
    /// Collection could not be read (exit 3).
    Unreadable(String),
    /// Bad argument (exit 2).
    Usage(String),
}

pub(crate) fn cap(s: &str) -> String {
    s.chars().take(MAX_NAME).collect()
}

pub(crate) fn rel(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Origin and path of a URL template: no userinfo, query or fragment.
pub fn safe_url(url: &str) -> String {
    // The WHATWG parser drops tab/LF/CR anywhere and trims C0 controls and
    // space at both ends before it looks for the authority; do the same.
    let norm: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let end = norm.find(['?', '#']).unwrap_or(norm.len());
    let u = &norm[..end];
    let is_sep = |c: char| c == '/' || c == '\\';
    let scheme_len = u
        .find(':')
        .filter(|&i| {
            let sch = &u[..i];
            let literal = sch.starts_with(|c: char| c.is_ascii_alphabetic())
                && sch
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
            (literal || sch.contains("{{")) && u[i + 1..].starts_with(is_sep)
        })
        .map(|i| i + 1);
    let after = scheme_len.unwrap_or(0);
    let slashes = u[after..].len() - u[after..].trim_start_matches(is_sep).len();
    // Without a scheme a single leading slash is a path, not an authority.
    let skip = if scheme_len.is_some() || slashes >= 2 {
        slashes
    } else {
        0
    };
    let (prefix, rest) = u.split_at(after + skip);
    let auth_end = rest.find(['/', '\\']).unwrap_or(rest.len());
    let (auth, path) = rest.split_at(auth_end);
    let host = auth.rsplit('@').next().unwrap_or(auth);
    let out = format!("{prefix}{host}{path}");
    cap(&out)
}

fn auth_kind(a: &Auth) -> &'static str {
    match a {
        Auth::None => "none",
        Auth::Inherit => "inherit",
        Auth::Basic { .. } => "basic",
        Auth::Bearer { .. } => "bearer",
        Auth::ApiKey { .. } => "apikey",
        Auth::Digest { .. } => "digest",
        Auth::AwsSigV4 { .. } => "awssigv4",
        Auth::OAuth2ClientCredentials { .. } => "oauth2-client-credentials",
        Auth::OAuth2AuthCode { .. } => "oauth2-auth-code",
    }
}

#[derive(Default)]
struct Walk {
    folders: usize,
    requests: usize,
    invalid: usize,
    out: Vec<NodeInfo>,
    truncated: bool,
}

impl Walk {
    fn push(&mut self, n: NodeInfo) {
        if self.out.len() < MAX_NODES {
            self.out.push(n);
        } else {
            self.truncated = true;
        }
    }

    fn walk(&mut self, root: &Path, nodes: &[Node]) {
        for node in nodes {
            match node {
                Node::Folder {
                    path,
                    name,
                    children,
                } => {
                    self.folders += 1;
                    self.push(NodeInfo {
                        path: cap(&rel(path)),
                        kind: "folder",
                        name: cap(name),
                        method: None,
                        url: None,
                        auth: None,
                        headers: Vec::new(),
                        valid: true,
                    });
                    self.walk(root, children);
                }
                Node::Request(meta) => {
                    self.requests += 1;
                    if self.out.len() >= MAX_NODES {
                        self.truncated = true;
                        continue;
                    }
                    let mut info = NodeInfo {
                        path: cap(&rel(&meta.path)),
                        kind: "request",
                        name: cap(&meta.name),
                        method: Some(format!("{:?}", meta.method).to_ascii_uppercase()),
                        url: None,
                        auth: None,
                        headers: Vec::new(),
                        valid: true,
                    };
                    match load_request(&root.join(&meta.path)) {
                        Ok(r) => {
                            info.url = Some(safe_url(&r.url));
                            info.auth = Some(auth_kind(&r.auth));
                            info.headers = r
                                .headers
                                .iter()
                                .take(MAX_HEADERS)
                                .map(|h| cap(&h.name))
                                .collect();
                        }
                        Err(_) => {
                            self.invalid += 1;
                            info.valid = false;
                        }
                    }
                    self.push(info);
                }
                Node::Invalid { path, .. } => {
                    self.invalid += 1;
                    self.push(NodeInfo {
                        path: cap(&rel(path)),
                        kind: "invalid",
                        name: String::new(),
                        method: None,
                        url: None,
                        auth: None,
                        headers: Vec::new(),
                        valid: false,
                    });
                }
            }
        }
    }
}

pub fn run(collection: &Path, env: Option<&str>) -> Result<InspectDoc, Failure> {
    let tree = open_collection(collection).map_err(|e| Failure::Unreadable(e.to_string()))?;
    let id = read_collection_id(&tree.root)
        .map_err(|e| Failure::Unreadable(e.to_string()))?
        .map(|i| i.to_string());
    let environments =
        list_environments(&tree.root).map_err(|e| Failure::Unreadable(e.to_string()))?;
    let environment = match env {
        Some(name) if environments.iter().any(|e| e == name) => Some(cap(name)),
        Some(_) => return Err(Failure::Usage("unknown environment".into())),
        None => None,
    };
    let mut w = Walk::default();
    w.walk(&tree.root, &tree.nodes);
    Ok(InspectDoc {
        schema: SCHEMA,
        command: "inspect",
        collection: CollectionInfo {
            id,
            format: tree.defaults.format,
            folders: w.folders,
            requests: w.requests,
            invalid: if w.truncated { None } else { Some(w.invalid) },
        },
        environments: environments.iter().map(|e| cap(e)).collect(),
        environment,
        truncated: w.truncated,
        nodes: w.out,
    })
}

#[cfg(test)]
mod tests {
    use super::safe_url;

    #[test]
    fn url_is_stripped() {
        assert_eq!(
            safe_url("https://u:p@host:8080/a/b?k=v#f"),
            "https://host:8080/a/b"
        );
        assert_eq!(safe_url("{{base}}/x?y=1"), "{{base}}/x");
    }

    #[test]
    fn userinfo_is_stripped_without_scheme() {
        assert_eq!(safe_url("u:p@h/x"), "h/x");
        assert_eq!(safe_url("//u:p@h/x"), "//h/x");
        assert_eq!(safe_url("u:p@h"), "h");
        assert_eq!(safe_url("/a@b/c"), "/a@b/c");
        assert_eq!(safe_url("http:/u:p@h/x"), "http:/h/x");
        assert_eq!(safe_url("http:///u:p@h/x"), "http:///h/x");
        assert_eq!(safe_url("http:/\\u:p@h/x"), "http:/\\h/x");
        assert_eq!(safe_url("\\\\u:p@h/x"), "\\\\h/x");
    }

    #[test]
    fn ignored_characters_do_not_hide_userinfo() {
        assert_eq!(safe_url(" http://u:p@h/x"), "http://h/x");
        assert_eq!(safe_url("\thttp://u:p@h/x"), "http://h/x");
        assert_eq!(safe_url("\u{1}http://u:p@h/x"), "http://h/x");
        assert_eq!(safe_url("ht\ttp://u:p@h/x"), "http://h/x");
        assert_eq!(safe_url("http:\t//u:p@h/x"), "http://h/x");
        assert_eq!(safe_url("http:/\t/u:p@h/x"), "http://h/x");
        assert_eq!(safe_url("http:/\n\\u:p@h"), "http:/\\h");
    }

    #[test]
    fn templated_scheme_is_recognised() {
        assert_eq!(safe_url("{{scheme}}://u:p@h/x"), "{{scheme}}://h/x");
    }
}
