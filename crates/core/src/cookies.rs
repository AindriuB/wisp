//! The cookie jar handle; the engine keeps one per collection.
//!
//! A stub: the jar holds nothing and no client uses it until the cookie provider lands.

use std::fmt;
use std::sync::{Arc, Mutex};

/// Cheap-to-clone handle to one collection's cookie jar.
#[derive(Clone, Default)]
pub struct CookieJar {
    #[allow(dead_code)]
    inner: Arc<Mutex<()>>,
}

impl CookieJar {
    /// Whether both handles point at the same jar.
    #[cfg(test)]
    pub(crate) fn same_jar(&self, other: &CookieJar) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for CookieJar {
    // Cookie values are credentials; never print them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CookieJar")
            .field("cookies", &format_args!("<redacted>"))
            .finish()
    }
}
