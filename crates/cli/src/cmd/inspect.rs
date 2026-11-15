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

fn cap(s: &str) -> String {
    s.chars().take(MAX_NAME).collect()
}

fn rel(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Origin and path of a URL template: no userinfo, query or fragment.
pub fn safe_url(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let u = &url[..end];
    let out = match u.find("://") {
        Some(i) => {
            let (scheme, rest) = u.split_at(i + 3);
            let auth_end = rest.find('/').unwrap_or(rest.len());
            let (auth, path) = rest.split_at(auth_end);
            let host = auth.rsplit('@').next().unwrap_or(auth);
            format!("{scheme}{host}{path}")
        }
        None => u.to_owned(),
    };
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
            invalid: w.invalid,
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
}
