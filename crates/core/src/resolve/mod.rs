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
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

impl std::fmt::Debug for ResolveWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveWarning::PatternMismatch { name, pattern, .. } => f
                .debug_struct("PatternMismatch")
                .field("name", name)
                .field("pattern", pattern)
                .field("value", &format_args!("\"<redacted>\""))
                .finish(),
            ResolveWarning::InvalidPattern {
                name,
                pattern,
                message,
            } => f
                .debug_struct("InvalidPattern")
                .field("name", name)
                .field("pattern", pattern)
                .field("message", message)
                .finish(),
        }
    }
}

mod engine;

#[cfg(test)]
mod tests;

pub use engine::{VarLookup, VarSource, lookup_var, resolve};

#[cfg(test)]
mod lookup_tests;
