use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keyring::Error as KErr;
use wisp_storage::keychain::{self, Backend, MAX_SECRET_BYTES, PROBE_ACCOUNT, Timeouts, map_error};
use wisp_storage::vault::{SecretStore, SecretStoreError, SecretValue, UnavailableReason};

fn unavailable(r: UnavailableReason) -> SecretStoreError {
    SecretStoreError::Unavailable { reason: r }
}

/// A platform error whose Debug mimics a status code.
struct Plat(&'static str);
impl fmt::Debug for Plat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl fmt::Display for Plat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Plat {}

fn pf(s: &'static str) -> KErr {
    KErr::PlatformFailure(Box::new(Plat(s)))
}
fn nsa(s: &'static str) -> KErr {
    KErr::NoStorageAccess(Box::new(Plat(s)))
}

#[test]
fn mapping_covers_every_row() {
    assert_eq!(
        map_error(&nsa("Error { code: -25294 }")),
        unavailable(UnavailableReason::NoService)
    );
    assert_eq!(
        map_error(&KErr::NoDefaultStore),
        unavailable(UnavailableReason::NoService)
    );
    assert_eq!(
        map_error(&pf("zbus error: org.freedesktop.DBus.Error.ServiceUnknown")),
        unavailable(UnavailableReason::NoService)
    );
    assert_eq!(
        map_error(&pf("Error { code: -25308 }")),
        SecretStoreError::Locked
    );
    assert_eq!(map_error(&nsa("Locked")), SecretStoreError::Locked);
    assert_eq!(
        map_error(&pf("Error { code: -128 }")),
        SecretStoreError::Denied
    );
    assert_eq!(
        map_error(&pf("Error { code: -25293 }")),
        SecretStoreError::Denied
    );
    assert_eq!(
        map_error(&pf("PlatformError(1223)")),
        SecretStoreError::Denied
    );
    assert_eq!(map_error(&pf("PlatformError(5)")), SecretStoreError::Denied);
    assert_eq!(map_error(&nsa("Prompt")), SecretStoreError::Denied);
    assert_eq!(
        map_error(&KErr::TooLong("password".into(), 2560)),
        SecretStoreError::TooLarge
    );
    assert_eq!(
        map_error(&pf("PlatformError(2147944183)")),
        SecretStoreError::TooLarge
    );
    assert_eq!(
        map_error(&pf("PlatformError(1314)")),
        SecretStoreError::Other { code: Some(1314) }
    );
    assert_eq!(
        map_error(&pf("Error { code: -9999 }")),
        SecretStoreError::Other { code: Some(-9999) }
    );
    assert_eq!(
        map_error(&KErr::Ambiguous(vec![])),
        SecretStoreError::Other { code: None }
    );
    assert_eq!(
        map_error(&pf("something odd")),
        SecretStoreError::Other { code: None }
    );
}

#[test]
fn mapping_never_formats_bad_encoding_payload() {
    const S: &str = "s3ntinel-value-xyz";
    for e in [
        KErr::BadEncoding(S.as_bytes().to_vec()),
        KErr::BadDataFormat(S.as_bytes().to_vec(), Box::new(Plat("x"))),
    ] {
        let m = map_error(&e);
        assert_eq!(m, SecretStoreError::Other { code: None });
        assert!(!format!("{m:?}").contains(S));
        assert!(!m.to_string().contains(S));
    }
}

#[derive(Default)]
struct Mem {
    map: Mutex<BTreeMap<String, Vec<u8>>>,
    calls: Mutex<usize>,
    next_err: Mutex<Option<KErr>>,
}

impl Mem {
    fn tick(&self) -> Result<(), KErr> {
        *self.calls.lock().unwrap() += 1;
        match self.next_err.lock().unwrap().take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

impl Backend for Mem {
    fn probe(&self) -> Result<(), KErr> {
        self.tick()?;
        Err(KErr::NoEntry)
    }
    fn get(&self, a: &str) -> Result<Vec<u8>, KErr> {
        self.tick()?;
        self.map
            .lock()
            .unwrap()
            .get(a)
            .cloned()
            .ok_or(KErr::NoEntry)
    }
    fn set(&self, a: &str, v: &[u8]) -> Result<(), KErr> {
        self.tick()?;
        self.map.lock().unwrap().insert(a.into(), v.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> Result<(), KErr> {
        self.tick()?;
        self.map
            .lock()
            .unwrap()
            .remove(a)
            .map(drop)
            .ok_or(KErr::NoEntry)
    }
}

fn mem_store() -> (Arc<Mem>, Box<dyn SecretStore>) {
    let m = Arc::new(Mem::default());
    (m.clone(), keychain::open_with(m, Timeouts::default()))
}

#[test]
fn portable_probe_is_unavailable_with_no_os_call() {
    let s = keychain::open("wisp-test-portable", true);
    assert_eq!(s.probe(), Err(unavailable(UnavailableReason::Portable)));
    assert_eq!(s.get("a"), Err(unavailable(UnavailableReason::Portable)));
}

#[test]
fn probe_reads_the_fixed_account_and_never_writes() {
    let (m, s) = mem_store();
    assert_eq!(s.probe(), Ok(()));
    assert_eq!(*m.calls.lock().unwrap(), 1);
    assert!(m.map.lock().unwrap().is_empty());
    assert!(!PROBE_ACCOUNT.is_empty());
    *m.next_err.lock().unwrap() = Some(nsa("code: -25294"));
    assert_eq!(s.probe(), Err(unavailable(UnavailableReason::NoService)));
}

#[test]
fn get_missing_is_none_and_errors_are_not_none() {
    let (m, s) = mem_store();
    assert_eq!(s.get("a"), Ok(None));
    *m.next_err.lock().unwrap() = Some(pf("code: -25308"));
    assert_eq!(s.get("a"), Err(SecretStoreError::Locked));
    s.set("a", &SecretValue::new("v")).unwrap();
    assert_eq!(s.get("a").unwrap().unwrap().expose(), "v");
    assert_eq!(s.delete("a"), Ok(()));
    assert_eq!(s.delete("a"), Ok(()));
}

#[test]
fn non_utf8_read_back_is_other_without_the_bytes() {
    let (m, s) = mem_store();
    m.map.lock().unwrap().insert("a".into(), vec![0xff, 0xfe]);
    assert_eq!(s.get("a"), Err(SecretStoreError::Other { code: None }));
}

#[test]
fn size_guard_counts_bytes_and_skips_the_backend() {
    let (m, s) = mem_store();
    s.set("a", &SecretValue::new("x".repeat(MAX_SECRET_BYTES)))
        .unwrap();
    assert_eq!(*m.calls.lock().unwrap(), 1);
    assert_eq!(
        s.set("a", &SecretValue::new("x".repeat(MAX_SECRET_BYTES + 1))),
        Err(SecretStoreError::TooLarge)
    );
    // 1,281 two-byte chars = 2,562 bytes but only 1,281 chars.
    assert_eq!(
        s.set("a", &SecretValue::new("é".repeat(1281))),
        Err(SecretStoreError::TooLarge)
    );
    assert_eq!(*m.calls.lock().unwrap(), 1);
    // Nothing was deleted or overwritten.
    assert_eq!(m.map.lock().unwrap()["a"].len(), MAX_SECRET_BYTES);
}

/// A backend whose `probe` blocks until released.
struct Blocking {
    release: Mutex<Receiver<()>>,
}

impl Backend for Blocking {
    fn probe(&self) -> Result<(), KErr> {
        let _ = self.release.lock().unwrap().recv();
        Err(KErr::NoEntry)
    }
    fn get(&self, _: &str) -> Result<Vec<u8>, KErr> {
        Ok(b"v".to_vec())
    }
    fn set(&self, _: &str, _: &[u8]) -> Result<(), KErr> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), KErr> {
        Ok(())
    }
}

fn blocking() -> (Sender<()>, Box<dyn SecretStore>) {
    let (tx, rx) = channel();
    let b = Arc::new(Blocking {
        release: Mutex::new(rx),
    });
    let t = Timeouts {
        probe: Duration::from_millis(300),
        call: Duration::from_millis(300),
    };
    (tx, keychain::open_with(b, t))
}

#[test]
fn blocked_probe_times_out_and_never_stalls_later_calls() {
    let (release, s) = blocking();
    let timed_out = Err(unavailable(UnavailableReason::TimedOut));
    let t = Instant::now();
    assert_eq!(s.probe(), timed_out);
    assert!(t.elapsed() < Duration::from_millis(1300));

    let t = Instant::now();
    assert_eq!(s.get("a"), timed_out.clone().map(|_: ()| None));
    assert_eq!(s.set("a", &SecretValue::new("v")), timed_out);
    assert_eq!(s.probe(), timed_out);
    assert_eq!(s.delete("a"), timed_out);
    assert!(t.elapsed() < Duration::from_millis(100));

    release.send(()).unwrap();
    // The worker goes idle shortly after the fake returns.
    let mut got = None;
    for _ in 0..50 {
        match s.get("a") {
            Ok(v) => {
                got = v;
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    assert_eq!(got.unwrap().expose(), "v");
}

/// A backend whose `probe` panics.
struct Panicky;

impl Backend for Panicky {
    fn probe(&self) -> Result<(), KErr> {
        panic!("backend probe panic (expected in this test)");
    }
    fn get(&self, _: &str) -> Result<Vec<u8>, KErr> {
        Ok(b"v".to_vec())
    }
    fn set(&self, _: &str, _: &[u8]) -> Result<(), KErr> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), KErr> {
        Ok(())
    }
}

#[test]
fn panicking_backend_does_not_wedge_the_store() {
    let timed_out = SecretStoreError::Unavailable {
        reason: UnavailableReason::TimedOut,
    };
    for _ in 0..8 {
        let t = Timeouts {
            probe: Duration::from_millis(300),
            call: Duration::from_millis(300),
        };
        let s = keychain::open_with(Arc::new(Panicky), t);
        let _ = s.probe();
        // Immediately after: may land on the dying worker's channel.
        let _ = s.get("a");
        std::thread::sleep(Duration::from_millis(30));
        let r = s.get("a");
        assert_ne!(r, Err(timed_out.clone()), "store stayed busy after a panic");
    }
}

#[test]
fn debug_redacts_stores() {
    const S: &str = "s3ntinel-value-xyz";
    let (m, s) = mem_store();
    s.set("a", &SecretValue::new(S)).unwrap();
    let p = keychain::open("wisp-test-x", true);
    // Debug of the value type and of the backend's error path carry no value.
    assert_eq!(format!("{:?}", SecretValue::new(S)), "<redacted>");
    let e = format!("{:?} {}", s.get("zzz"), map_error(&KErr::NoDefaultStore));
    assert!(!e.contains(S));
    drop((m, p));
}

// ---- real keychain, run by hand or by the headless checks ----

/// Deletes only the accounts this test created, under its own service.
struct Cleanup {
    store: Box<dyn SecretStore>,
    accounts: Vec<String>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        for a in &self.accounts {
            let _ = self.store.delete(a);
        }
    }
}

fn real(accounts: &[&str]) -> (String, Cleanup) {
    let service = format!("wisp-test-{}", uuid::Uuid::new_v4());
    println!("test service: {service}");
    let store = keychain::open(&service, false);
    (
        service,
        Cleanup {
            store,
            accounts: accounts.iter().map(|s| s.to_string()).collect(),
        },
    )
}

#[test]
#[ignore = "touches the real OS keychain"]
fn roundtrip_set_get_overwrite_delete() {
    let (_svc, c) = real(&["a1"]);
    let s = &c.store;
    assert_eq!(s.probe(), Ok(()));
    assert_eq!(s.get("a1"), Ok(None));
    s.set("a1", &SecretValue::new("one")).unwrap();
    assert_eq!(s.get("a1").unwrap().unwrap().expose(), "one");
    s.set("a1", &SecretValue::new("two")).unwrap();
    assert_eq!(s.get("a1").unwrap().unwrap().expose(), "two");
    s.delete("a1").unwrap();
    assert_eq!(s.get("a1"), Ok(None));
}

#[test]
#[ignore = "touches the real OS keychain"]
fn roundtrip_cap_boundary() {
    let (_svc, c) = real(&["big"]);
    let ok = SecretValue::new("x".repeat(MAX_SECRET_BYTES));
    c.store.set("big", &ok).unwrap();
    assert_eq!(c.store.get("big").unwrap().unwrap(), ok);
    assert_eq!(
        c.store
            .set("big", &SecretValue::new("x".repeat(MAX_SECRET_BYTES + 1))),
        Err(SecretStoreError::TooLarge)
    );
}

#[test]
#[ignore = "needs a Linux container with no session bus"]
fn headless_probe_is_unavailable() {
    let (_svc, c) = real(&[]);
    let t = Instant::now();
    let r = c.store.probe();
    println!("headless probe: {r:?} in {:?}", t.elapsed());
    assert!(matches!(r, Err(SecretStoreError::Unavailable { .. })));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
#[ignore = "needs a locked gnome-keyring in docker"]
fn locked_keyring_get_is_refused() {
    // The check seeds this item with `secret-tool store` before locking.
    let store = keychain::open("wisp-test-locked", false);
    // Searching for a missing account does not unlock, so the probe passes.
    assert_eq!(store.probe(), Ok(()));
    // Reading an existing item needs the collection: Locked, or Denied when
    // the unlock prompt is dismissed.
    let r = store.get("wisp-test-locked");
    println!("locked get: {r:?}");
    assert!(matches!(
        r,
        Err(SecretStoreError::Locked | SecretStoreError::Denied)
    ));
}
