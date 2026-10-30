//! Variable resolution: turns an authored request into a `PreparedRequest`.

use crate::request::{CollectionDefaults, Environment, FolderDefaults};

/// The layers that variables, headers and auth are inherited from.
#[derive(Debug, Clone)]
pub struct Scope<'a> {
    pub collection: &'a CollectionDefaults,
    /// Outermost to nearest.
    pub folders: Vec<&'a FolderDefaults>,
    pub environment: Option<&'a Environment>,
}

/// Where in a request a problem was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Url,
    PathParam(String),
    Param(String),
    Header(String),
    Auth,
    Body,
    Var(String),
}

/// One thing wrong with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    UnresolvedVar { name: String, location: Location },
    MissingPathParam { name: String },
    Syntax { location: Location, message: String },
    VarCycle { name: String },
}

/// Every problem found while resolving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{} problem(s) resolving request", problems.len())]
pub struct ResolveError {
    pub problems: Vec<Problem>,
}

/// A non-fatal finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResolveWarning {
    PatternMismatch {
        name: String,
        pattern: String,
        value: String,
    },
    InvalidPattern {
        name: String,
        pattern: String,
        message: String,
    },
}
