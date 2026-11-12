//! [`SecretStore`] over the OS keychain, through `keyring`.
//!
//! macOS Keychain, Windows Credential Manager, or a Secret Service provider
//! on Linux. Only `set_secret`/`get_secret` are used, on the UTF-8 bytes, so
//! the stored blob is exactly the value. Every OS call runs on the store's one
//! worker thread and the caller waits a bounded time; a call that outlives its
//! bound leaves the worker stuck, and further calls say `TimedOut` at once
//! until it returns. Errors are fixed variants and status codes, never values.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use keyring::Error as KeyringError;

use crate::vault::{SecretStore, SecretStoreError, SecretValue, UnavailableReason};

/// Largest value accepted, in bytes: Windows' confirmed credential blob cap.
pub const MAX_SECRET_BYTES: usize = 2560;
/// Bound on `probe`.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Bound on `get`, `set` and `delete`. Leaves time to answer a keychain prompt.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// The account `probe` reads. It is never written.
pub const PROBE_ACCOUNT: &str = "wisp-probe-does-not-exist";

/// How long a caller waits for each kind of call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    pub probe: Duration,
    pub call: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            probe: PROBE_TIMEOUT,
            call: CALL_TIMEOUT,
        }
    }
}

/// The OS calls, called blocking on the worker thread. Public so tests can
/// supply a fake; production uses [`open`].
pub trait Backend: Send + Sync + 'static {
    /// Read the fixed probe account. `Err(NoEntry)` means the store works.
    fn probe(&self) -> Result<(), KeyringError>;
    fn get(&self, account: &str) -> Result<Vec<u8>, KeyringError>;
    fn set(&self, account: &str, value: &[u8]) -> Result<(), KeyringError>;
    fn delete(&self, account: &str) -> Result<(), KeyringError>;
}

/// `keyring`'s platform store under one service name.
struct OsBackend {
    service: String,
}

impl OsBackend {
    fn entry(&self, account: &str) -> Result<keyring::Entry, KeyringError> {
        keyring::Entry::new(&self.service, account)
    }
}

impl Backend for OsBackend {
    fn probe(&self) -> Result<(), KeyringError> {
        self.entry(PROBE_ACCOUNT)?.get_secret().map(drop)
    }
    fn get(&self, account: &str) -> Result<Vec<u8>, KeyringError> {
        self.entry(account)?.get_secret()
    }
    fn set(&self, account: &str, value: &[u8]) -> Result<(), KeyringError> {
        self.entry(account)?.set_secret(value)
    }
    fn delete(&self, account: &str) -> Result<(), KeyringError> {
        self.entry(account)?.delete_credential()
    }
}

/// Open the OS keychain store for `service`. With `portable` the store does
/// nothing: `probe` is `Unavailable { Portable }` and no OS call is made.
pub fn open(service: &str, portable: bool) -> Box<dyn SecretStore> {
    if portable {
        return Box::new(PortableStore);
    }
    open_with(
        Arc::new(OsBackend {
            service: service.to_owned(),
        }),
        Timeouts::default(),
    )
}

/// A store over any backend, with explicit timeouts.
pub fn open_with(backend: Arc<dyn Backend>, timeouts: Timeouts) -> Box<dyn SecretStore> {
    Box::new(KeychainStore {
        worker: Worker::new(backend),
        timeouts,
    })
}

fn unavailable(reason: UnavailableReason) -> SecretStoreError {
    SecretStoreError::Unavailable { reason }
}

struct PortableStore;

impl fmt::Debug for PortableStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PortableStore")
    }
}

impl SecretStore for PortableStore {
    fn probe(&self) -> Result<(), SecretStoreError> {
        Err(unavailable(UnavailableReason::Portable))
    }
    fn get(&self, _: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        Err(unavailable(UnavailableReason::Portable))
    }
    fn set(&self, _: &str, _: &SecretValue) -> Result<(), SecretStoreError> {
        Err(unavailable(UnavailableReason::Portable))
    }
    fn delete(&self, _: &str) -> Result<(), SecretStoreError> {
        Err(unavailable(UnavailableReason::Portable))
    }
}

type Job = Box<dyn FnOnce(&dyn Backend) + Send>;

/// One worker thread, spawned on first use, running one OS call at a time.
struct Worker {
    backend: Arc<dyn Backend>,
    busy: Arc<AtomicBool>,
    tx: Mutex<Option<mpsc::Sender<Job>>>,
}

impl Worker {
    fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            busy: Arc::new(AtomicBool::new(false)),
            tx: Mutex::new(None),
        }
    }

    fn sender(&self) -> Result<mpsc::Sender<Job>, SecretStoreError> {
        let mut g = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = g.as_ref() {
            return Ok(tx.clone());
        }
        let (tx, rx) = mpsc::channel::<Job>();
        let backend = self.backend.clone();
        thread::Builder::new()
            .name("wisp-keychain".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(backend.as_ref());
                }
            })
            .map_err(|_| SecretStoreError::Other { code: None })?;
        *g = Some(tx.clone());
        Ok(tx)
    }

    /// Run `f` on the worker and wait at most `timeout`. A stuck worker
    /// answers `TimedOut` at once; nothing is queued.
    fn run<T: Send + 'static>(
        &self,
        timeout: Duration,
        f: impl FnOnce(&dyn Backend) -> T + Send + 'static,
    ) -> Result<T, SecretStoreError> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(unavailable(UnavailableReason::TimedOut));
        }
        let (rtx, rrx) = mpsc::channel();
        let busy = self.busy.clone();
        let job: Job = Box::new(move |b| {
            // Idle again before the result is sent, so a caller that got it
            // can call again at once. A late result has no receiver: dropped.
            struct Idle(Arc<AtomicBool>);
            impl Drop for Idle {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::SeqCst);
                }
            }
            let idle = Idle(busy);
            let out = f(b);
            drop(idle);
            let _ = rtx.send(out);
        });
        let sent = self.sender().and_then(|tx| {
            tx.send(job)
                .map_err(|_| SecretStoreError::Other { code: None })
        });
        if let Err(e) = sent {
            self.busy.store(false, Ordering::SeqCst);
            return Err(e);
        }
        match rrx.recv_timeout(timeout) {
            Ok(v) => Ok(v),
            Err(RecvTimeoutError::Timeout) => Err(unavailable(UnavailableReason::TimedOut)),
            // The worker panicked mid-call. Make the next call respawn it.
            Err(RecvTimeoutError::Disconnected) => {
                *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                self.busy.store(false, Ordering::SeqCst);
                Err(SecretStoreError::Other { code: None })
            }
        }
    }
}

struct KeychainStore {
    worker: Worker,
    timeouts: Timeouts,
}

impl fmt::Debug for KeychainStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeychainStore { .. }")
    }
}

impl SecretStore for KeychainStore {
    fn probe(&self) -> Result<(), SecretStoreError> {
        match self.worker.run(self.timeouts.probe, |b| b.probe())? {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(map_error(&e)),
        }
    }

    fn get(&self, account: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        let account = account.to_owned();
        match self
            .worker
            .run(self.timeouts.call, move |b| b.get(&account))?
        {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Ok(Some(SecretValue::new(s))),
                // The bytes are dropped unformatted.
                Err(_) => Err(SecretStoreError::Other { code: None }),
            },
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(map_error(&e)),
        }
    }

    fn set(&self, account: &str, value: &SecretValue) -> Result<(), SecretStoreError> {
        let bytes = value.expose().as_bytes();
        if bytes.len() > MAX_SECRET_BYTES {
            return Err(SecretStoreError::TooLarge);
        }
        let (account, bytes) = (account.to_owned(), bytes.to_vec());
        match self
            .worker
            .run(self.timeouts.call, move |b| b.set(&account, &bytes))?
        {
            Ok(()) => Ok(()),
            Err(e) => Err(map_error(&e)),
        }
    }

    fn delete(&self, account: &str) -> Result<(), SecretStoreError> {
        let account = account.to_owned();
        match self
            .worker
            .run(self.timeouts.call, move |b| b.delete(&account))?
        {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(map_error(&e)),
        }
    }
}

/// Map a `keyring` error to a store error. `NoEntry` is not an error and
/// callers handle it first; here it is `Other` for completeness. The payload
/// of `BadEncoding`, `BadDataFormat` and `Ambiguous` is never formatted.
pub fn map_error(e: &KeyringError) -> SecretStoreError {
    match e {
        KeyringError::NoDefaultStore => unavailable(UnavailableReason::NoService),
        KeyringError::TooLong(..) => SecretStoreError::TooLarge,
        KeyringError::NoStorageAccess(p) => {
            classify(&format!("{p:?}"), unavailable(UnavailableReason::NoService))
        }
        KeyringError::PlatformFailure(p) => {
            let text = format!("{p:?}");
            let code = extract_code(&text);
            classify(&text, SecretStoreError::Other { code })
        }
        _ => SecretStoreError::Other { code: None },
    }
}

/// Map a platform error's text (a status code or a provider message, never a
/// value) to a variant, or `default` when nothing matches.
fn classify(text: &str, default: SecretStoreError) -> SecretStoreError {
    if let Some(code) = extract_code(text) {
        match code {
            // errSecInteractionNotAllowed
            -25308 => return SecretStoreError::Locked,
            // errSecUserCanceled, errSecAuthFailed
            -128 | -25293 => return SecretStoreError::Denied,
            _ => {}
        }
        match code as i32 as u32 {
            // ERROR_ACCESS_DENIED, ERROR_CANCELLED
            5 | 1223 => return SecretStoreError::Denied,
            0x8007_06F7 => return SecretStoreError::TooLarge,
            _ => {}
        }
    }
    let t = text.to_ascii_lowercase();
    if t.contains("islocked") || t.contains("is locked") || t.contains("collection locked") {
        return SecretStoreError::Locked;
    }
    if t.contains("dismissed") || t.contains("cancel") {
        return SecretStoreError::Denied;
    }
    const NO_SERVICE: [&str; 6] = [
        "serviceunknown",
        "noserver",
        "no such file",
        "connection refused",
        "not provided by any",
        "session bus",
    ];
    if NO_SERVICE.iter().any(|m| t.contains(m)) {
        return unavailable(UnavailableReason::NoService);
    }
    default
}

/// The status code in a platform error's Debug text: `code: -25293`,
/// `Code(5)` or `0x800706F7`.
fn extract_code(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    if let Some(i) = lower.find("0x") {
        let hex: String = lower[i + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if let Ok(v) = i64::from_str_radix(&hex, 16) {
            return Some(v);
        }
    }
    for key in ["code: ", "code(", "code "] {
        if let Some(i) = lower.find(key) {
            let rest = &lower[i + key.len()..];
            let num: String = rest
                .char_indices()
                .take_while(|(n, c)| c.is_ascii_digit() || (*n == 0 && *c == '-'))
                .map(|(_, c)| c)
                .collect();
            if let Ok(v) = num.parse() {
                return Some(v);
            }
        }
    }
    None
}
