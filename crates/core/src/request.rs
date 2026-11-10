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
    /// HTTP Digest (RFC 7616); applied by the engine after a 401 challenge.
    Digest {
        username: String,
        password: String,
    },
    /// AWS Signature Version 4; applied by the engine just before sending.
    AwsSigV4 {
        access_key: String,
        secret_key: String,
        /// Empty means none.
        session_token: String,
        region: String,
        service: String,
    },
    /// OAuth2 client-credentials grant; the engine fetches the token (task 103).
    OAuth2ClientCredentials {
        token_url: String,
        client_id: String,
        /// Empty means a public client.
        client_secret: String,
        scope: String,
        client_auth: ClientAuth,
    },
    /// OAuth2 authorization-code grant with PKCE; the engine fetches the token (task 103).
    OAuth2AuthCode {
        auth_url: String,
        token_url: String,
        client_id: String,
        /// Empty means a public client.
        client_secret: String,
        scope: String,
        client_auth: ClientAuth,
        /// Loopback redirect port; 0 means ephemeral.
        redirect_port: u16,
    },
}

/// How an OAuth2 client authenticates itself to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ClientAuth {
    /// HTTP Basic with the client id and secret.
    #[default]
    Basic,
    /// `client_id` and `client_secret` in the form body.
    Body,
}

/// `s` unchanged when it is exactly one `{{var}}` reference (whitespace allowed around it
/// and inside the braces); otherwise empty.
fn keep_var_ref(s: &str) -> String {
    let t = s.trim();
    match crate::template::var_refs(t).as_slice() {
        [r] if r.terminated && r.range == (0..t.len()) && !r.name.is_empty() => s.to_owned(),
        _ => String::new(),
    }
}

impl Auth {
    /// A copy that keeps only variable references, for storing in history. A field whose
    /// value is exactly one `{{var}}` reference is kept; any other value becomes empty.
    pub fn redacted(&self) -> Auth {
        let k = keep_var_ref;
        match self {
            Auth::None => Auth::None,
            Auth::Inherit => Auth::Inherit,
            Auth::Basic { username, password } => Auth::Basic {
                username: k(username),
                password: k(password),
            },
            Auth::Bearer { token } => Auth::Bearer { token: k(token) },
            Auth::ApiKey {
                name,
                value,
                location,
            } => Auth::ApiKey {
                name: k(name),
                value: k(value),
                location: *location,
            },
            Auth::Digest { username, password } => Auth::Digest {
                username: k(username),
                password: k(password),
            },
            Auth::AwsSigV4 {
                access_key,
                secret_key,
                session_token,
                region,
                service,
            } => Auth::AwsSigV4 {
                access_key: k(access_key),
                secret_key: k(secret_key),
                session_token: k(session_token),
                region: k(region),
                service: k(service),
            },
            Auth::OAuth2ClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
            } => Auth::OAuth2ClientCredentials {
                token_url: k(token_url),
                client_id: k(client_id),
                client_secret: k(client_secret),
                scope: k(scope),
                client_auth: *client_auth,
            },
            Auth::OAuth2AuthCode {
                auth_url,
                token_url,
                client_id,
                client_secret,
                scope,
                client_auth,
                redirect_port,
            } => Auth::OAuth2AuthCode {
                auth_url: k(auth_url),
                token_url: k(token_url),
                client_id: k(client_id),
                client_secret: k(client_secret),
                scope: k(scope),
                client_auth: *client_auth,
                redirect_port: *redirect_port,
            },
        }
    }
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
            Auth::Digest { username, .. } => f
                .debug_struct("Digest")
                .field("username", username)
                .field("password", &format_args!("\"<redacted>\""))
                .finish(),
            Auth::AwsSigV4 {
                region, service, ..
            } => f
                .debug_struct("AwsSigV4")
                .field("access_key", &format_args!("\"<redacted>\""))
                .field("secret_key", &format_args!("\"<redacted>\""))
                .field("session_token", &format_args!("\"<redacted>\""))
                .field("region", region)
                .field("service", service)
                .finish(),
            Auth::OAuth2ClientCredentials {
                token_url,
                client_id,
                scope,
                client_auth,
                ..
            } => f
                .debug_struct("OAuth2ClientCredentials")
                .field("token_url", token_url)
                .field("client_id", client_id)
                .field("client_secret", &format_args!("\"<redacted>\""))
                .field("scope", scope)
                .field("client_auth", client_auth)
                .finish(),
            Auth::OAuth2AuthCode {
                auth_url,
                token_url,
                client_id,
                scope,
                client_auth,
                redirect_port,
                ..
            } => f
                .debug_struct("OAuth2AuthCode")
                .field("auth_url", auth_url)
                .field("token_url", token_url)
                .field("client_id", client_id)
                .field("client_secret", &format_args!("\"<redacted>\""))
                .field("scope", scope)
                .field("client_auth", client_auth)
                .field("redirect_port", redirect_port)
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

    fn oauth2_pair(secret: &str) -> [Auth; 2] {
        [
            Auth::OAuth2ClientCredentials {
                token_url: "https://idp.test/token".into(),
                client_id: "cid".into(),
                client_secret: secret.into(),
                scope: "read".into(),
                client_auth: ClientAuth::Basic,
            },
            Auth::OAuth2AuthCode {
                auth_url: "https://idp.test/auth".into(),
                token_url: "https://idp.test/token".into(),
                client_id: "cid".into(),
                client_secret: secret.into(),
                scope: "read".into(),
                client_auth: ClientAuth::Body,
                redirect_port: 0,
            },
        ]
    }

    #[test]
    fn debug_redacts_oauth2_client_secret() {
        for a in oauth2_pair("CSECRETMARK") {
            for text in both(&a) {
                assert!(text.contains("cid"));
                assert!(text.contains("<redacted>"));
                assert!(!text.contains("CSECRETMARK"), "{text}");
            }
        }
    }

    #[test]
    fn redacted_oauth2_empties_literals_and_keeps_var_refs() {
        for (a, kept) in oauth2_pair("SECRETVAL")
            .into_iter()
            .zip(oauth2_pair("{{secret}}"))
        {
            let r = a.redacted();
            let (Auth::OAuth2ClientCredentials { client_secret, .. }
            | Auth::OAuth2AuthCode { client_secret, .. }) = &r
            else {
                panic!("variant changed");
            };
            assert_eq!(client_secret, "");
            let (Auth::OAuth2ClientCredentials { client_secret, .. }
            | Auth::OAuth2AuthCode { client_secret, .. }) = kept.redacted()
            else {
                panic!("variant changed");
            };
            assert_eq!(client_secret, "{{secret}}");
        }
    }

    #[test]
    fn debug_redacts_digest_auth() {
        let a = Auth::Digest {
            username: "alice".into(),
            password: "DIGPWMARK".into(),
        };
        for text in both(&a) {
            assert!(text.contains("alice"));
            assert!(text.contains("<redacted>"));
            assert!(!text.contains("DIGPWMARK"), "{text}");
        }
    }

    #[test]
    fn debug_redacts_aws_sigv4_auth() {
        let a = Auth::AwsSigV4 {
            access_key: "AKMARK".into(),
            secret_key: "SKMARK".into(),
            session_token: "STMARK".into(),
            region: "us-east-1".into(),
            service: "s3".into(),
        };
        for text in both(&a) {
            assert!(text.contains("us-east-1"));
            for s in ["AKMARK", "SKMARK", "STMARK"] {
                assert!(!text.contains(s), "{text}");
            }
        }
    }

    /// Every secret-bearing variant built with the same value in each string field.
    fn with_value(v: &str) -> Vec<Auth> {
        let s = || v.to_owned();
        vec![
            Auth::Basic {
                username: s(),
                password: s(),
            },
            Auth::Bearer { token: s() },
            Auth::ApiKey {
                name: s(),
                value: s(),
                location: ApiKeyLocation::Query,
            },
            Auth::Digest {
                username: s(),
                password: s(),
            },
            Auth::AwsSigV4 {
                access_key: s(),
                secret_key: s(),
                session_token: s(),
                region: s(),
                service: s(),
            },
        ]
    }

    #[test]
    fn redacted_keeps_a_single_var_reference() {
        for v in ["{{pw}}", " {{ pw }} "] {
            for a in with_value(v) {
                assert_eq!(a.redacted(), a, "{v:?}");
            }
        }
    }

    #[test]
    fn redacted_empties_anything_else() {
        for v in ["abc", "{{a}}x", "{{a}}{{b}}", "{{a", "{{}}", ""] {
            for (a, empty) in with_value(v).iter().zip(with_value("")) {
                assert_eq!(a.redacted(), empty, "{v:?}");
            }
        }
        assert_eq!(Auth::Inherit.redacted(), Auth::Inherit);
        assert_eq!(Auth::None.redacted(), Auth::None);
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
