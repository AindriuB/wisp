//! Plain-data command, event and error types. No locks, handles or runtime types.

use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::auth::EngineAuth;
use crate::body::BodyHandle;
use crate::resolve::ResolveWarning;

/// Identifier of one request, allocated monotonically by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequestId(pub u64);

/// HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

/// One header name/value pair.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value", &format_args!("\"<redacted>\""))
            .finish()
    }
}

/// A fully resolved request, ready for I/O.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: PreparedBody,
    pub timeout: Option<Duration>,
    pub client: ClientSettings,
    pub warnings: Vec<ResolveWarning>,
    /// Auth the engine applies at send time (Digest, SigV4); `resolve` adds no header for it.
    #[serde(default)]
    pub auth: Option<EngineAuth>,
}

/// URL with the query replaced by `<redacted>`.
fn redact_query(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

/// Proxy URL with any userinfo replaced by `<redacted>@`. A URL without
/// `://` is treated as a bare authority (reqwest accepts `user:pw@host:port`).
fn redact_userinfo(url: &str) -> String {
    let (prefix, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (format!("{scheme}://"), rest),
        None => (String::new(), url),
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    match rest[..end].rfind('@') {
        Some(at) => format!("{prefix}<redacted>@{}", &rest[at + 1..]),
        None => url.to_string(),
    }
}

impl fmt::Debug for PreparedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedRequest")
            .field("method", &self.method)
            .field("url", &format_args!("{:?}", redact_query(&self.url)))
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("timeout", &self.timeout)
            .field("client", &self.client)
            .field("warnings", &self.warnings)
            .field("auth", &self.auth)
            .finish()
    }
}

/// Body of a prepared request.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum PreparedBody {
    None,
    Bytes(Vec<u8>),
    File(PathBuf),
    Multipart(Vec<PreparedPart>),
}

impl fmt::Debug for PreparedBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreparedBody::None => f.write_str("None"),
            PreparedBody::Bytes(b) => f
                .debug_tuple("Bytes")
                .field(&format_args!("{} bytes", b.len()))
                .finish(),
            PreparedBody::File(p) => f.debug_tuple("File").field(p).finish(),
            PreparedBody::Multipart(parts) => f.debug_tuple("Multipart").field(parts).finish(),
        }
    }
}

/// One multipart part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedPart {
    pub name: String,
    pub value: PreparedPartValue,
}

/// Value of a multipart part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PreparedPartValue {
    Text(String),
    File(PathBuf),
}

/// Settings that select a reqwest client.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientSettings {
    pub verify_tls: bool,
    pub follow_redirects: bool,
    pub proxy: Option<String>,
}

impl fmt::Debug for ClientSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let proxy = self.proxy.as_deref().map(redact_userinfo);
        f.debug_struct("ClientSettings")
            .field("verify_tls", &self.verify_tls)
            .field("follow_redirects", &self.follow_redirects)
            .field("proxy", &proxy)
            .finish()
    }
}

/// Input to the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Send(PreparedRequest),
    Cancel(RequestId),
}

/// Classification of a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    InvalidUrl,
    Dns,
    Connect,
    Tls,
    Timeout,
    Body,
    BodyFile,
    Other,
}

/// A typed engine failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
}

/// HTTP protocol version of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpVersion {
    Http09,
    Http10,
    Http11,
    Http2,
    Http3,
}

/// Output of the engine.
///
/// Per request the order is `Started`, optionally `Headers`, zero or more
/// `Progress`, then exactly one of `Completed`, `Failed` or `Cancelled`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Started {
        id: RequestId,
        at: SystemTime,
    },
    Headers {
        id: RequestId,
        status: u16,
        reason: Option<String>,
        version: HttpVersion,
        headers: Vec<Header>,
        ttfb: Duration,
    },
    Progress {
        id: RequestId,
        bytes: u64,
    },
    Completed {
        id: RequestId,
        total: Duration,
        size: u64,
        body: BodyHandle,
    },
    Failed {
        id: RequestId,
        error: EngineError,
    },
    Cancelled {
        id: RequestId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(url: &str, headers: Vec<Header>, body: PreparedBody) -> PreparedRequest {
        PreparedRequest {
            method: Method::Get,
            url: url.into(),
            headers,
            body,
            timeout: None,
            client: ClientSettings {
                verify_tls: true,
                follow_redirects: true,
                proxy: None,
            },
            warnings: vec![],
            auth: None,
        }
    }

    fn both(v: &impl fmt::Debug) -> [String; 2] {
        [format!("{v:?}"), format!("{v:#?}")]
    }

    #[test]
    fn debug_redacts_prepared_request_engine_auth() {
        let mut p = prepared("http://x.test/", vec![], PreparedBody::None);
        for (auth, secrets) in [
            (
                EngineAuth::Digest {
                    username: "alice".into(),
                    password: "DIGPWMARK".into(),
                },
                vec!["DIGPWMARK"],
            ),
            (
                EngineAuth::AwsSigV4 {
                    access_key: "AKMARK".into(),
                    secret_key: "SKMARK".into(),
                    session_token: "STMARK".into(),
                    region: "us-east-1".into(),
                    service: "s3".into(),
                },
                vec!["AKMARK", "SKMARK", "STMARK"],
            ),
        ] {
            p.auth = Some(auth);
            for text in both(&p) {
                for s in &secrets {
                    assert!(!text.contains(s), "{text}");
                }
            }
        }
    }

    #[test]
    fn debug_redacts_proxy_userinfo() {
        for proxy in [
            "http://user:PROXYMARK@proxy.example:8080",
            "http://PROXYMARK@proxy.example:8080",
            "user:PROXYMARK@proxy.example:8080",
            "PROXYMARK@proxy.example:8080",
        ] {
            let c = ClientSettings {
                verify_tls: true,
                follow_redirects: false,
                proxy: Some(proxy.into()),
            };
            for s in both(&c) {
                assert!(!s.contains("PROXYMARK"));
                assert!(s.contains("proxy.example:8080") && s.contains("<redacted>@"));
                assert!(s.contains("verify_tls") && s.contains("follow_redirects"));
            }
        }
        let plain = ClientSettings {
            verify_tls: true,
            follow_redirects: true,
            proxy: Some("http://proxy.example:8080".into()),
        };
        assert!(format!("{plain:?}").contains("http://proxy.example:8080"));
    }

    #[test]
    fn debug_redacts_header_value() {
        let h = Header {
            name: "Authorization".into(),
            value: "HDRMARK".into(),
        };
        for s in both(&h) {
            assert!(!s.contains("HDRMARK"));
            assert!(s.contains("Authorization") && s.contains("<redacted>"));
        }
    }

    #[test]
    fn debug_redacts_prepared_request_query_and_body() {
        let r = prepared(
            "https://api.example/path?key=QRYMARK",
            vec![],
            PreparedBody::Bytes(b"BODYMARK".to_vec()),
        );
        for s in both(&r) {
            assert!(!s.contains("QRYMARK") && !s.contains("BODYMARK"));
            assert!(s.contains("https://api.example/path?<redacted>"));
            assert!(s.contains("method") && s.contains("8 bytes"));
        }
        let no_query = prepared("https://api.example/p", vec![], PreparedBody::None);
        assert!(format!("{no_query:?}").contains("https://api.example/p\""));
    }

    #[test]
    fn debug_redacts_command_send() {
        let r = prepared(
            "https://api.example/p?token=SENDQRY",
            vec![Header {
                name: "Authorization".into(),
                value: "Bearer SENDHDR".into(),
            }],
            PreparedBody::None,
        );
        for s in both(&Command::Send(r)) {
            assert!(!s.contains("SENDQRY") && !s.contains("SENDHDR"));
            assert!(s.contains("Authorization"));
        }
    }
}
