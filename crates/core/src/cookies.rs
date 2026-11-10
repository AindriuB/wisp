//! The cookie jar handle shared by every client the engine builds.
//!
//! A stub: the jar holds nothing and no client uses it until the cookie provider lands.

use std::fmt;
use std::sync::{Arc, Mutex};

/// Cheap-to-clone handle to the shared cookie jar.
#[derive(Clone, Default)]
pub struct CookieJar {
    #[allow(dead_code)]
    inner: Arc<Mutex<()>>,
}

impl fmt::Debug for CookieJar {
    // Cookie values are credentials; never print them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CookieJar")
            .field("cookies", &format_args!("<redacted>"))
            .finish()
    }
}
