//! Phase timings of one request.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Connection-phase timings. `None` means not measured or a reused connection, never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Timings {
    pub dns: Option<Duration>,
    /// TCP connect and TLS handshake combined.
    pub connect: Option<Duration>,
}
