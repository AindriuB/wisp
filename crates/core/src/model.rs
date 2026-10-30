//! Plain-data command, event and error types. No locks, handles or runtime types.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A fully resolved request, ready for I/O.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: PreparedBody,
    pub timeout: Option<Duration>,
    pub client: ClientSettings,
    pub warnings: Vec<ResolveWarning>,
}

/// Body of a prepared request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PreparedBody {
    None,
    Bytes(Vec<u8>),
    File(PathBuf),
    Multipart(Vec<PreparedPart>),
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientSettings {
    pub verify_tls: bool,
    pub follow_redirects: bool,
    pub proxy: Option<String>,
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
