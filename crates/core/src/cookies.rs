//! In-memory cookie jars, one per collection, plugged into reqwest as its cookie provider.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use cookie_store::{CookieDomain, CookieExpiration, CookieStore, RawCookie};
use reqwest::Url;
use reqwest::header::HeaderValue;

/// Longest lifetime a loaded cookie keeps, so a hostile file cannot overflow date arithmetic.
const MAX_LOAD_LIFETIME_SECS: u64 = 1 << 31;

/// Mozilla Public Suffix List (MPL-2.0), bundled so cookies cannot be scoped to `com`, `co.uk`, ...
/// Snapshot header: `VERSION: 2026-11-09_13-00-17_UTC`. Parsed once per process.
static PUBLIC_SUFFIXES: LazyLock<publicsuffix::List> = LazyLock::new(|| {
    PARSE_COUNT.fetch_add(1, Ordering::SeqCst);
    include_str!("../data/public_suffix_list.dat")
        .parse()
        .expect("bundled public suffix list parses")
});

/// How many times the list was parsed; a test asserts it stays at one.
static PARSE_COUNT: AtomicUsize = AtomicUsize::new(0);

fn new_store() -> CookieStore {
    CookieStore::new_with_public_suffix(Some(PUBLIC_SUFFIXES.clone()))
}

/// One cookie as the app sees it. `expires: None` is a session cookie.
#[derive(Clone, PartialEq, Eq)]
pub struct CookieRecord {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires: Option<SystemTime>,
}

impl fmt::Debug for CookieRecord {
    // Cookie values are credentials; never print them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CookieRecord")
            .field("name", &self.name)
            .field("value", &format_args!("<redacted>"))
            .field("domain", &self.domain)
            .field("host_only", &self.host_only)
            .field("path", &self.path)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("expires", &self.expires)
            .finish()
    }
}

struct JarInner {
    store: Mutex<CookieStore>,
    generation: AtomicU64,
}

impl Default for JarInner {
    fn default() -> Self {
        Self {
            store: Mutex::new(new_store()),
            generation: AtomicU64::new(0),
        }
    }
}

impl JarInner {
    fn store(&self) -> std::sync::MutexGuard<'_, CookieStore> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// Cheap-to-clone handle to one collection's cookie jar.
#[derive(Clone, Default)]
pub struct CookieJar {
    inner: Arc<JarInner>,
}

impl CookieJar {
    /// Unexpired cookies as of `now`, session cookies included (with `expires: None`).
    pub fn snapshot(&self, now: SystemTime) -> Vec<CookieRecord> {
        let store = self.inner.store();
        store
            .iter_any()
            .filter_map(|c| {
                let expires = match &c.expires {
                    CookieExpiration::SessionEnd => None,
                    CookieExpiration::AtUtc(t) => {
                        let t = SystemTime::from(*t);
                        if t <= now {
                            return None;
                        }
                        Some(t)
                    }
                };
                let (domain, host_only) = match &c.domain {
                    CookieDomain::HostOnly(d) => (d.clone(), true),
                    CookieDomain::Suffix(d) => (d.clone(), false),
                    CookieDomain::NotPresent | CookieDomain::Empty => return None,
                };
                Some(CookieRecord {
                    name: c.name().to_owned(),
                    value: c.value().to_owned(),
                    domain,
                    host_only,
                    path: String::from(&c.path),
                    secure: c.secure().unwrap_or(false),
                    http_only: c.http_only().unwrap_or(false),
                    expires,
                })
            })
            .collect()
    }

    /// Replace the jar's contents. Expired and malformed records are dropped.
    pub fn load(&self, records: Vec<CookieRecord>) {
        let now = SystemTime::now();
        let mut fresh = new_store();
        for r in records {
            let _ = insert_record(&mut fresh, r, now);
        }
        *self.inner.store() = fresh;
        self.inner.bump();
    }

    /// Delete one cookie; whether it existed.
    pub fn remove(&self, domain: &str, path: &str, name: &str) -> bool {
        let removed = self.inner.store().remove(domain, path, name).is_some();
        if removed {
            self.inner.bump();
        }
        removed
    }

    /// Delete every cookie.
    pub fn clear(&self) {
        self.inner.store().clear();
        self.inner.bump();
    }

    /// Increases on every change, whether from a response or an API call.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }
}

fn insert_record(store: &mut CookieStore, r: CookieRecord, now: SystemTime) -> Option<()> {
    let host = r.domain.trim_start_matches('.');
    let url = Url::parse(&format!("https://{host}/")).ok()?;
    let mut raw = RawCookie::new(r.name, r.value);
    raw.set_path(r.path);
    if r.secure {
        raw.set_secure(true);
    }
    if r.http_only {
        raw.set_http_only(true);
    }
    if !r.host_only {
        raw.set_domain(host.to_owned());
    }
    let mut cookie = cookie_store::Cookie::try_from_raw_cookie(&raw, &url).ok()?;
    if let Some(t) = r.expires {
        let left = t.duration_since(now).ok().filter(|d| !d.is_zero())?;
        cookie.expires = CookieExpiration::from(left.as_secs().min(MAX_LOAD_LIFETIME_SECS));
    }
    store.insert(cookie.into_owned(), &url).ok().map(|_| ())
}

impl reqwest::cookie::CookieStore for CookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let cookies: Vec<RawCookie<'static>> = cookie_headers
            .filter_map(|v| v.to_str().ok())
            .filter_map(|s| RawCookie::parse(s.to_owned()).ok())
            .collect();
        if cookies.is_empty() {
            return;
        }
        self.inner
            .store()
            .store_response_cookies(cookies.into_iter(), url);
        self.inner.bump();
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let s = self
            .inner
            .store()
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if s.is_empty() {
            return None;
        }
        HeaderValue::from_str(&s).ok()
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

/// Shared store of jars keyed by collection id (`None` is "no collection").
#[derive(Clone, Default)]
pub struct CookieJars {
    jars: Arc<Mutex<HashMap<Option<String>, CookieJar>>>,
}

impl CookieJars {
    /// The jar for a collection, created on first use; the same key gives the same jar.
    pub fn jar(&self, collection: Option<&str>) -> CookieJar {
        let mut jars = self.jars.lock().unwrap_or_else(|e| e.into_inner());
        jars.entry(collection.map(str::to_owned))
            .or_default()
            .clone()
    }
}

impl fmt::Debug for CookieJars {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CookieJars")
            .field("jars", &format_args!("<redacted>"))
            .finish()
    }
}

#[cfg(test)]
impl CookieJar {
    /// Whether both handles point at the same jar.
    pub(crate) fn same_jar(&self, other: &CookieJar) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_suffix_list_is_parsed_once_for_every_jar() {
        let a = CookieJar::default();
        a.load(vec![]);
        let _b = CookieJar::default();
        let _c = CookieJar::default();
        assert_eq!(PARSE_COUNT.load(Ordering::SeqCst), 1);
    }
}
