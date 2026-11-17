//! What a finished request amounts to: its checks.

use wisp_core::assert::{Assertion, AssertionResult};

/// One evaluated assertion. The definition is kept beside the result so the
/// Checks pane shows the expected value as sent, after the draft changes.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    pub assertion: Assertion,
    pub result: AssertionResult,
}
