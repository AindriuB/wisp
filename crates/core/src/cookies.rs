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
    // Comments are stripped by build.rs; the committed data file is unmodified.
    include_str!(concat!(env!("OUT_DIR"), "/psl.dat"))
        .parse()
        .expect("bundled public suffix list parses")
});

/// Browser-like limits: one cookie's name plus value, cookies per domain, cookies per jar.
const MAX_COOKIE_BYTES: usize = 4096;
const MAX_PER_DOMAIN: usize = 50;
const MAX_PER_JAR: usize = 3000;

type Key = (String, String, String);

/// The store plus the order cookies were set in, which the library does not keep.
struct State {
    store: CookieStore,
    order: HashMap<Key, u64>,
    next: u64,
}

impl State {
    fn new() -> Self {
        Self {
            store: new_store(),
            order: HashMap::new(),
            next: 0,
        }
    }

    /// Cheap check, with no allocation, that any entry is expired or a cap is exceeded.
    fn needs_eviction(&self) -> bool {
        let mut total = 0usize;
        let mut per_domain: HashMap<&str, usize> = HashMap::new();
        for c in self.store.iter_any() {
            if c.is_expired() {
                return true;
            }
            total += 1;
            let (CookieDomain::HostOnly(d) | CookieDomain::Suffix(d)) = &c.domain else {
                continue;
            };
            let n = per_domain.entry(d.as_str()).or_default();
            *n += 1;
            if *n > MAX_PER_DOMAIN {
                return true;
            }
        }
        total > MAX_PER_JAR
    }

    fn touch(&mut self, key: Key) {
        self.next += 1;
        self.order.insert(key, self.next);
    }

    /// Drop expired and deleted entries, then evict the oldest beyond the per-domain and
    /// per-jar caps.
    fn enforce_caps(&mut self) {
        if !self.needs_eviction() {
            return;
        }
        let mut live: Vec<(Key, u64)> = Vec::new();
        let mut dead: Vec<Key> = Vec::new();
        for c in self.store.iter_any() {
            let Some(d) = domain_of(c) else { continue };
            let key = (d, String::from(&c.path), c.name().to_owned());
            if c.is_expired() {
                dead.push(key);
            } else {
                let seq = self.order.get(&key).copied().unwrap_or(0);
                live.push((key, seq));
            }
        }
        live.sort_by_key(|(_, seq)| *seq);
        let mut per_domain: HashMap<&str, usize> = HashMap::new();
        for (k, _) in &live {
            *per_domain.entry(k.0.as_str()).or_default() += 1;
        }
        let mut kept: Vec<&(Key, u64)> = Vec::new();
        for entry in &live {
            let n = per_domain.get_mut(entry.0.0.as_str()).expect("counted");
            if *n > MAX_PER_DOMAIN {
                *n -= 1;
                dead.push(entry.0.clone());
            } else {
                kept.push(entry);
            }
        }
        let over = kept.len().saturating_sub(MAX_PER_JAR);
        dead.extend(kept.iter().take(over).map(|e| e.0.clone()));
        for (d, p, n) in dead {
            self.store.remove(&d, &p, &n);
            self.order.remove(&(d, p, n));
        }
        let store = &self.store;
        self.order
            .retain(|(d, p, n), _| store.contains_any(d, p, n));
    }
}

fn domain_of(c: &cookie_store::Cookie<'_>) -> Option<String> {
    match &c.domain {
        CookieDomain::HostOnly(d) | CookieDomain::Suffix(d) => Some(d.clone()),
        CookieDomain::NotPresent | CookieDomain::Empty => None,
    }
}

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
    store: Mutex<State>,
    generation: AtomicU64,
}

impl Default for JarInner {
    fn default() -> Self {
        Self {
            store: Mutex::new(State::new()),
            generation: AtomicU64::new(0),
        }
    }
}

impl JarInner {
    fn store(&self) -> std::sync::MutexGuard<'_, State> {
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
        let state = self.inner.store();
        state
            .store
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
        let mut fresh = State::new();
        for r in records {
            let _ = insert_record(&mut fresh, r, now);
        }
        fresh.enforce_caps();
        *self.inner.store() = fresh;
        self.inner.bump();
    }

    /// Delete one cookie; whether it existed.
    pub fn remove(&self, domain: &str, path: &str, name: &str) -> bool {
        let removed = {
            let mut state = self.inner.store();
            state
                .order
                .remove(&(domain.to_owned(), path.to_owned(), name.to_owned()));
            state.store.remove(domain, path, name).is_some()
        };
        if removed {
            self.inner.bump();
        }
        removed
    }

    /// Delete every cookie.
    pub fn clear(&self) {
        {
            let mut state = self.inner.store();
            state.store.clear();
            state.order.clear();
        }
        self.inner.bump();
    }

    /// Increases on every change, whether from a response or an API call.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }
}

/// Whether `host` is a registrable name, so a `Domain` attribute may scope a cookie to it.
fn is_registrable(host: &str) -> bool {
    use publicsuffix::Psl as _;
    PUBLIC_SUFFIXES.domain(host.as_bytes()).is_some()
}

fn insert_record(state: &mut State, r: CookieRecord, now: SystemTime) -> Option<()> {
    let host = r.domain.trim_start_matches('.');
    if !r.host_only && !is_registrable(host) {
        return None;
    }
    if r.name.len() + r.value.len() > MAX_COOKIE_BYTES {
        return None;
    }
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
    let cookie = cookie.into_owned();
    let key = (
        domain_of(&cookie)?,
        String::from(&cookie.path),
        cookie.name().to_owned(),
    );
    state.store.insert(cookie, &url).ok()?;
    state.touch(key);
    Some(())
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
        {
            let mut state = self.inner.store();
            for raw in cookies {
                if raw.name().len() + raw.value().len() > MAX_COOKIE_BYTES {
                    continue;
                }
                let Ok(cookie) = cookie_store::Cookie::try_from_raw_cookie(&raw, url) else {
                    continue;
                };
                let cookie = cookie.into_owned();
                let Some(domain) = domain_of(&cookie) else {
                    continue;
                };
                let key = (domain, String::from(&cookie.path), cookie.name().to_owned());
                if state.store.insert(cookie, url).is_ok() {
                    state.touch(key);
                }
            }
            state.enforce_caps();
        }
        self.inner.bump();
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let s = self
            .inner
            .store()
            .store
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

/// The `Cookie` header for a request that carries a user-set one: the user's pairs, then the
/// jar's cookies for `url` whose names are not among them (the user's value wins a clash).
/// `None` when the jar adds nothing, so the user's header goes out unchanged.
pub(crate) fn merge_cookie_header(
    jar: &CookieJar,
    url: &Url,
    user_values: &[&str],
) -> Option<String> {
    let jar_header = reqwest::cookie::CookieStore::cookies(jar, url)?;
    let jar_header = jar_header.to_str().ok()?;
    let user_pairs: Vec<&str> = user_values
        .iter()
        .flat_map(|v| v.split(';'))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let name = |p: &str| p.split_once('=').map_or(p, |(n, _)| n).trim().to_owned();
    let taken: std::collections::HashSet<String> = user_pairs.iter().map(|p| name(p)).collect();
    let extra: Vec<&str> = jar_header
        .split("; ")
        .filter(|p| !taken.contains(&name(p)))
        .collect();
    if extra.is_empty() {
        return None;
    }
    let mut all = user_pairs;
    all.extend(extra);
    Some(all.join("; "))
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
