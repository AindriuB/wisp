//! Plain-data command, event and error types. No locks, handles or runtime types.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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
    Cancelled,
    Other,
}

/// A typed engine failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
}

/// Output of the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Completed {
        id: RequestId,
        /// M0-provisional: moves to a `Headers` event in M1.
        status: u16,
        /// M0-provisional: moves to a `Headers` event in M1.
        reason: Option<String>,
        /// M0-provisional: moves to a `Headers` event in M1.
        headers: Vec<Header>,
        total: Duration,
        size: u64,
        /// M0-provisional: becomes a `BodyHandle` in M1.
        body: Vec<u8>,
    },
    /// M0-provisional: cancellation reports `Failed{kind: Cancelled}` until M1 adds `Event::Cancelled`.
    Failed { id: RequestId, error: EngineError },
}
