//! The authoring model: requests, folders, collections and environments as the user edits them.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::Method;

/// A named variable.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Var {
    pub name: String,
    pub value: String,
}

/// A param, header or form field that can be switched off.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct KeyValue {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

/// Where an API key is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiKeyLocation {
    Header,
    Query,
}

/// Authentication scheme.
#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Auth {
    None,
    #[default]
    Inherit,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        name: String,
        value: String,
        location: ApiKeyLocation,
    },
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::None => f.write_str("None"),
            Auth::Inherit => f.write_str("Inherit"),
            Auth::Basic { username, .. } => f
                .debug_struct("Basic")
                .field("username", username)
                .field("password", &format_args!("\"<redacted>\""))
                .finish(),
            Auth::Bearer { .. } => f
                .debug_struct("Bearer")
                .field("token", &format_args!("\"<redacted>\""))
                .finish(),
            Auth::ApiKey { name, location, .. } => f
                .debug_struct("ApiKey")
                .field("name", name)
                .field("value", &format_args!("\"<redacted>\""))
                .field("location", location)
                .finish(),
        }
    }
}

/// A text body, optionally stored in a sidecar file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextBody {
    pub content: String,
    /// File name beside the request file, when the body is stored there.
    pub sidecar: Option<String>,
}

/// Value of a multipart part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PartValue {
    Text(String),
    File(PathBuf),
}

/// One multipart part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub name: String,
    pub value: PartValue,
    pub enabled: bool,
}

/// Request body. Paths are absolute in memory.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Body {
    #[default]
    None,
    Json(TextBody),
    Text(TextBody),
    Xml(TextBody),
    Form(Vec<KeyValue>),
    Multipart(Vec<Part>),
    File(PathBuf),
}

/// Per-request transport settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestSettings {
    pub timeout: Option<Duration>,
    pub follow_redirects: bool,
    pub verify_tls: bool,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout: None,
            follow_redirects: true,
            verify_tls: true,
        }
    }
}

/// An authored request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    pub method: Method,
    pub url: String,
    pub path: Vec<Var>,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub auth: Auth,
    pub body: Body,
    pub vars: Vec<Var>,
    pub settings: RequestSettings,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            name: String::new(),
            method: Method::Get,
            url: String::new(),
            path: Vec::new(),
            params: Vec::new(),
            headers: Vec::new(),
            auth: Auth::Inherit,
            body: Body::None,
            vars: Vec::new(),
            settings: RequestSettings::default(),
        }
    }
}

/// Defaults a folder passes to its children.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FolderDefaults {
    pub vars: Vec<Var>,
    pub headers: Vec<KeyValue>,
    pub auth: Auth,
    pub order: Vec<String>,
}

/// Defaults at the collection root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionDefaults {
    pub format: u32,
    pub vars: Vec<Var>,
    pub headers: Vec<KeyValue>,
    pub auth: Auth,
    pub order: Vec<String>,
}

impl Default for CollectionDefaults {
    fn default() -> Self {
        Self {
            format: 1,
            vars: Vec::new(),
            headers: Vec::new(),
            auth: Auth::None,
            order: Vec::new(),
        }
    }
}

/// A named set of variables and secrets.
#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub vars: Vec<Var>,
    pub secrets: Vec<Var>,
}

impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secret_names: Vec<&str> = self.secrets.iter().map(|s| s.name.as_str()).collect();
        f.debug_struct("Environment")
            .field("name", &self.name)
            .field("vars", &self.vars)
            .field("secrets", &secret_names)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both(v: &impl fmt::Debug) -> [String; 2] {
        [format!("{v:?}"), format!("{v:#?}")]
    }

    #[test]
    fn debug_redacts_basic_password() {
        let a = Auth::Basic {
            username: "alice".into(),
            password: "PWMARK".into(),
        };
        for s in both(&a) {
            assert!(!s.contains("PWMARK"));
            assert!(s.contains("alice") && s.contains("password") && s.contains("<redacted>"));
        }
    }

    #[test]
    fn debug_redacts_bearer_token() {
        let a = Auth::Bearer {
            token: "TOKMARK".into(),
        };
        for s in both(&a) {
            assert!(!s.contains("TOKMARK"));
            assert!(s.contains("token") && s.contains("<redacted>"));
        }
    }

    #[test]
    fn debug_redacts_api_key_value() {
        let a = Auth::ApiKey {
            name: "X-Key".into(),
            value: "KEYMARK".into(),
            location: ApiKeyLocation::Query,
        };
        for s in both(&a) {
            assert!(!s.contains("KEYMARK"));
            assert!(s.contains("X-Key") && s.contains("Query") && s.contains("<redacted>"));
        }
    }

    #[test]
    fn debug_redacts_request_with_bearer_auth() {
        let r = Request {
            name: "visible-name".into(),
            auth: Auth::Bearer {
                token: "REQMARK".into(),
            },
            ..Request::default()
        };
        for s in both(&r) {
            assert!(!s.contains("REQMARK"));
            assert!(s.contains("visible-name"));
        }
    }

    #[test]
    fn debug_redacts_collection_defaults_with_basic_auth() {
        let d = CollectionDefaults {
            auth: Auth::Basic {
                username: "bob".into(),
                password: "COLMARK".into(),
            },
            ..CollectionDefaults::default()
        };
        for s in both(&d) {
            assert!(!s.contains("COLMARK"));
            assert!(s.contains("bob"));
        }
    }

    #[test]
    fn environment_debug_hides_secret_values() {
        let env = Environment {
            name: "dev".into(),
            vars: vec![],
            secrets: vec![Var {
                name: "token".into(),
                value: "s3cret".into(),
            }],
        };
        let s = format!("{env:?}");
        assert!(!s.contains("s3cret"));
        assert!(s.contains("token"));
    }

    #[test]
    fn defaults_match_the_contract() {
        assert_eq!(Auth::default(), Auth::Inherit);
        assert_eq!(Request::default().auth, Auth::Inherit);
        assert_eq!(FolderDefaults::default().auth, Auth::Inherit);
        let c = CollectionDefaults::default();
        assert_eq!((c.format, c.auth), (1, Auth::None));
        let s = RequestSettings::default();
        assert_eq!(
            (s.timeout, s.follow_redirects, s.verify_tls),
            (None, true, true)
        );
    }
}
