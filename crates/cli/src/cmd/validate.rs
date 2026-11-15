//! `validate`: every finding of a collection, from the tolerant storage walker.

use std::path::Path;

use wisp_storage::validate::{Severity, validate};

use super::inspect::{Failure, cap, rel};
use crate::dto::{FindingInfo, SCHEMA, ValidateDoc};

pub fn run(collection: &Path) -> Result<ValidateDoc, Failure> {
    if std::fs::canonicalize(collection).is_err() || !collection.join("collection.toml").is_file() {
        return Err(Failure::Unreadable("not a collection".into()));
    }
    let report = validate(collection);
    let mut errors = 0;
    let mut warnings = 0;
    let findings = report
        .findings
        .iter()
        .map(|f| {
            let severity = match f.severity {
                Severity::Error => {
                    errors += 1;
                    "error"
                }
                Severity::Warning => {
                    warnings += 1;
                    "warning"
                }
            };
            FindingInfo {
                path: cap(&rel(&f.path)),
                key: f.key.as_deref().map(cap),
                message: cap(&f.message),
                severity,
            }
        })
        .collect();
    Ok(ValidateDoc {
        schema: SCHEMA,
        command: "validate",
        errors,
        warnings,
        truncated: report.truncated,
        findings,
    })
}
