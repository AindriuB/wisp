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

#[derive(Debug, Serialize)]
pub struct NodeInfo {
    pub path: String,
    pub kind: &'static str,
    pub name: String,
    pub method: Option<String>,
    pub url: Option<String>,
    pub auth: Option<&'static str>,
    pub headers: Vec<String>,
    pub valid: bool,
}
dto!(NodeInfo);
