//! Every serialisable output type. Nothing outside this file derives `Serialize`.

use serde::Serialize;

mod sealed {
    pub trait Sealed {}
}

/// An output document. Sealed: only types in this file implement it.
pub trait Dto: sealed::Sealed + Serialize {}

macro_rules! dto {
    ($t:ty) => {
        impl sealed::Sealed for $t {}
        impl Dto for $t {}
    };
}

pub const SCHEMA: &str = "wisp-cli/1";

#[derive(Debug, Serialize)]
pub struct InspectDoc {
    pub schema: &'static str,
    pub command: &'static str,
    pub collection: CollectionInfo,
    pub environments: Vec<String>,
    pub environment: Option<String>,
    pub truncated: bool,
    pub nodes: Vec<NodeInfo>,
}
dto!(InspectDoc);

#[derive(Debug, Serialize)]
pub struct CollectionInfo {
    pub id: Option<String>,
    pub format: u32,
    pub folders: usize,
    pub requests: usize,
    /// `None` when the node cap was hit: requests past it are not loaded.
    pub invalid: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub path: String,
    pub kind: &'static str,
    pub name: String,
    pub method: Option<String>,
    pub url: Option<String>,
    pub auth: Option<&'static str>,
    pub headers: Vec<String>,
    pub valid: bool,
    pub has_description: bool,
    pub expected_status: Vec<u16>,
}
dto!(NodeInfo);

#[derive(Debug, Serialize)]
pub struct ValidateDoc {
    pub schema: &'static str,
    pub command: &'static str,
    pub errors: usize,
    pub warnings: usize,
    pub truncated: bool,
    pub findings: Vec<FindingInfo>,
}
dto!(ValidateDoc);

#[derive(Debug, Serialize)]
pub struct FindingInfo {
    pub path: String,
    pub key: Option<String>,
    pub message: String,
    pub severity: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ExplainDoc {
    pub schema: &'static str,
    pub command: &'static str,
    pub request: String,
    /// Authored text, never expanded; capped at 1,000 characters.
    pub description: String,
    pub expected_status: Vec<u16>,
    pub environment: Option<EnvDoc>,
    pub method: String,
    pub url: Vec<PartDoc>,
    pub params: Vec<ParamDoc>,
    pub headers: Vec<HeaderDoc>,
    pub auth: AuthDoc,
    pub settings: Vec<SettingDoc>,
    pub body: BodyDoc,
    pub variables: Vec<VarDoc>,
    pub problems: Vec<ProblemDoc>,
    pub warnings: Vec<String>,
}
dto!(ExplainDoc);

#[derive(Debug, Serialize)]
pub struct EnvDoc {
    pub name: String,
    /// Set when the secrets live in a store the CLI does not open.
    pub keychain: Option<&'static str>,
    pub secrets: Vec<SecretDoc>,
}

#[derive(Debug, Serialize)]
pub struct SecretDoc {
    pub name: String,
    pub present: bool,
}

/// One piece of display text. Never carries a secret value.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PartDoc {
    Text { text: String },
    Secret { name: String, resolved: bool },
    Dynamic { name: String },
    Hidden { reason: &'static str },
}

#[derive(Debug, Serialize)]
pub struct ParamDoc {
    pub kind: &'static str,
    pub name: Vec<PartDoc>,
    pub value: Vec<PartDoc>,
    pub origin: String,
}

#[derive(Debug, Serialize)]
pub struct HeaderDoc {
    pub name: Vec<PartDoc>,
    pub value: Vec<PartDoc>,
    pub origin: String,
    pub overridden: Vec<String>,
    pub synthetic: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthDoc {
    pub kind: &'static str,
    pub origin: Option<String>,
    /// `none`, `header`, `query` or `at-send`.
    pub application: &'static str,
    pub target: Option<Vec<PartDoc>>,
    pub fields: Vec<AuthFieldDoc>,
}

#[derive(Debug, Serialize)]
pub struct AuthFieldDoc {
    pub name: &'static str,
    pub value: Vec<PartDoc>,
}

#[derive(Debug, Serialize)]
pub struct SettingDoc {
    pub name: &'static str,
    pub value: String,
    pub source: &'static str,
}

#[derive(Debug, Serialize)]
pub struct BodyDoc {
    pub kind: &'static str,
    pub bytes: Option<usize>,
    pub vars: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct VarDoc {
    pub name: String,
    pub origin: String,
    pub value: Vec<PartDoc>,
    pub shadowed: Vec<String>,
    pub locations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProblemDoc {
    pub kind: &'static str,
    pub name: Option<String>,
    pub location: Option<String>,
    pub message: Option<String>,
}

pub const SNAPSHOT_SOURCE: &str = "persisted files only; unsaved edits in the app are not included";

#[derive(Debug, Serialize)]
pub struct SnapshotDoc {
    pub schema: &'static str,
    pub command: &'static str,
    pub wisp_cli: &'static str,
    pub source: &'static str,
    pub collection: CollectionInfo,
    pub environments: Vec<EnvSnapshot>,
    pub tree: Vec<NodeInfo>,
    pub tree_truncated: bool,
    pub validation: ValidationSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestSlot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recent: Option<Vec<RecentInfo>>,
    pub truncated: Vec<String>,
}
dto!(SnapshotDoc);

/// The form `snapshot` falls back to when the full document cannot fit.
#[derive(Debug, Serialize)]
pub struct SnapshotMinimal {
    pub schema: &'static str,
    pub command: &'static str,
    pub wisp_cli: &'static str,
    pub source: &'static str,
    pub collection: CollectionInfo,
    pub counts: SnapshotCounts,
    pub truncated: Vec<String>,
}
dto!(SnapshotMinimal);

#[derive(Debug, Serialize)]
pub struct SnapshotCounts {
    pub environments: usize,
    pub tree: usize,
    pub findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct EnvSnapshot {
    pub name: String,
    /// Plain and secret names together; never a value.
    pub variable_count: usize,
    pub variables: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidationSnapshot {
    pub errors: usize,
    pub warnings: usize,
    pub findings: Vec<FindingInfo>,
    pub findings_truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RequestSlot {
    Doc(Box<ExplainDoc>),
    Omitted { omitted: bool },
}

#[derive(Debug, Serialize)]
pub struct RecentInfo {
    pub at: u64,
    pub request: Option<String>,
    pub method: String,
    pub url: String,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<&'static str>,
    pub status: Option<u16>,
    pub environment: Option<String>,
    pub assertions: Vec<RecentCheck>,
}

#[derive(Debug, Serialize)]
pub struct RecentCheck {
    pub label: String,
    pub state: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RunDoc {
    pub schema: &'static str,
    pub command: &'static str,
    pub request: String,
    pub environment: Option<String>,
    pub method: &'static str,
    pub url: String,
    pub result: &'static str,
    pub exit: u8,
    pub delivery: &'static str,
    pub retry_safe: bool,
    pub status: Option<u16>,
    /// Text output only: the JSON field set is fixed by the spec.
    #[serde(skip)]
    pub reason: Option<String>,
    pub expected_status: Vec<u16>,
    pub status_ok: Option<bool>,
    pub timing_ms: TimingDoc,
    pub size: Option<u64>,
    pub error: Option<ErrorDoc>,
    pub assertions: Vec<CheckDoc>,
    pub dynamic: Vec<DynamicDoc>,
    pub history: &'static str,
    pub problems: Vec<ProblemDoc>,
}
dto!(RunDoc);

#[derive(Debug, Serialize)]
pub struct TimingDoc {
    pub ttfb: Option<u64>,
    pub total: Option<u64>,
}

/// A failure kind. The engine's own message is never carried.
#[derive(Debug, Serialize)]
pub struct ErrorDoc {
    pub kind: &'static str,
}

/// One assertion: label, state and skip reason. Never an expected or actual value.
#[derive(Debug, Serialize)]
pub struct CheckDoc {
    pub label: String,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DynamicDoc {
    pub name: String,
    pub value: String,
}
