//! Throwaway spike 109: keychain backends. Never merged.
//! Every entry uses service `wisp-spike-109`; nothing under `wisp` is touched.

#![allow(dead_code, unused_imports)]

use std::time::Instant;

const SERVICE: &str = "wisp-spike-109";
const ACCOUNT: &str = "spike-account";
const VALUE: &[u8] = b"not-a-real-secret";

fn report(step: &str, start: Instant, res: Result<String, String>) {
    let ms = start.elapsed().as_millis();
    match res {
        Ok(v) => println!("spike109 {step}: ok {v} ({ms} ms)"),
        Err(e) => println!("spike109 {step}: ERR {e} ({ms} ms)"),
    }
}

/// Run candidate `which` (b = windows Cred*, c = keyring, o = oo7, m = macOS
/// security-framework). Returns false when the candidate is not compiled in.
pub fn run(which: &str) -> bool {
    match which {
        #[cfg(all(feature = "spike-win", target_os = "windows"))]
        "b" => {
            win::run();
            true
        }
        #[cfg(feature = "spike-keyring")]
        "c" => {
            keyring_run();
            true
        }
        #[cfg(all(feature = "spike-oo7", target_os = "linux"))]
        "o" => {
            oo7_run();
            true
        }
        #[cfg(all(feature = "spike-mac", target_os = "macos"))]
        "m" => {
            mac::run();
            true
        }
        _ => false,
    }
}

#[cfg(feature = "spike-keyring")]
fn keyring_run() {
    let t = Instant::now();
    let entry = match keyring::Entry::new(SERVICE, ACCOUNT) {
        Ok(e) => e,
        Err(e) => return report("new", t, Err(format!("{e:?}"))),
    };
    report("new", t, Ok(String::new()));
    let t = Instant::now();
    report("write", t, entry.set_secret(VALUE).map(|_| String::new()).map_err(|e| format!("{e:?}")));
    let t = Instant::now();
    report(
        "read",
        t,
        entry
            .get_secret()
            .map(|v| format!("match={}", v == VALUE))
            .map_err(|e| format!("{e:?}")),
    );
    let t = Instant::now();
    report("delete", t, entry.delete_credential().map(|_| String::new()).map_err(|e| format!("{e:?}")));
    let t = Instant::now();
    report(
        "read-after-delete",
        t,
        entry.get_secret().map(|_| "STILL PRESENT".into()).map_err(|e| format!("{e:?}")),
    );
}

#[cfg(all(feature = "spike-oo7", target_os = "linux"))]
fn oo7_run() {
    use std::collections::HashMap;
    futures_lite::future::block_on(async {
        let attrs: HashMap<&str, &str> =
            HashMap::from([("service", SERVICE), ("account", ACCOUNT)]);
        let t = Instant::now();
        let kr = match oo7::Keyring::new().await {
            Ok(k) => k,
            Err(e) => return report("open", t, Err(format!("{e:?}"))),
        };
        report("open", t, Ok(String::new()));
        let t = Instant::now();
        report(
            "write",
            t,
            kr.create_item("wisp-spike-109", &attrs, VALUE, true)
                .await
                .map(|_| String::new())
                .map_err(|e| format!("{e:?}")),
        );
        let t = Instant::now();
        let found = async {
            let items = kr.search_items(&attrs).await.map_err(|e| format!("{e:?}"))?;
            let item = items.first().ok_or("not found".to_string())?;
            let s = item.secret().await.map_err(|e| format!("{e:?}"))?;
            Ok::<_, String>(format!("match={}", s.as_bytes() == VALUE))
        }
        .await;
        report("read", t, found);
        let t = Instant::now();
        report(
            "delete",
            t,
            kr.delete(&attrs).await.map(|_| String::new()).map_err(|e| format!("{e:?}")),
        );
        let t = Instant::now();
        let after = kr.search_items(&attrs).await.map(|v| format!("items={}", v.len()));
        report("search-after-delete", t, after.map_err(|e| format!("{e:?}")));
    });
}

#[cfg(all(feature = "spike-mac", target_os = "macos"))]
mod mac {
    use super::*;
    use security_framework::passwords::*;

    /// Modes via WISP_SPIKE_MAC_STEP: write | read | delete | all (default).
    pub fn run() {
        let step = std::env::var("WISP_SPIKE_MAC_STEP").unwrap_or_else(|_| "all".into());
        if step == "write" || step == "all" {
            let t = Instant::now();
            report("write", t, set_generic_password(SERVICE, ACCOUNT, VALUE).map(|_| String::new()).map_err(|e| format!("{e:?}")));
        }
        if step == "read" || step == "all" {
            let t = Instant::now();
            report("read", t, get_generic_password(SERVICE, ACCOUNT).map(|v| format!("match={}", v == VALUE)).map_err(|e| format!("{e:?}")));
        }
        if step == "delete" || step == "all" {
            let t = Instant::now();
            report("delete", t, delete_generic_password(SERVICE, ACCOUNT).map(|_| String::new()).map_err(|e| format!("{e:?}")));
            let t = Instant::now();
            report("read-after-delete", t, get_generic_password(SERVICE, ACCOUNT).map(|_| "STILL PRESENT".into()).map_err(|e| format!("{e:?}")));
        }
    }
}

#[cfg(all(feature = "spike-win", target_os = "windows"))]
mod win {
    use super::*;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Security::Credentials::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn run() {
        let target = wide(&format!("{SERVICE}/{ACCOUNT}"));
        let user = wide(ACCOUNT);
        let blob: Vec<u8> = match std::env::var("WISP_SPIKE_BLOB_LEN").ok().and_then(|v| v.parse::<usize>().ok()) {
            Some(n) => vec![b'x'; n],
            None => VALUE.to_vec(),
        };
        let t = Instant::now();
        let cred = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_ptr() as *mut u16),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_ptr() as *mut u16),
            ..Default::default()
        };
        let r = unsafe { CredWriteW(&cred, 0) };
        report("write", t, r.map(|_| String::new()).map_err(|e| format!("{e:?}")));
        let t = Instant::now();
        let mut out: *mut CREDENTIALW = std::ptr::null_mut();
        let r = unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut out) };
        let r = r.map(|_| unsafe {
            let c = &*out;
            let got = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
            let m = got == &blob[..];
            CredFree(out as *const _);
            format!("match={m}")
        });
        report("read", t, r.map_err(|e| format!("{e:?}")));
        let t = Instant::now();
        let r = unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) };
        report("delete", t, r.map(|_| String::new()).map_err(|e| format!("{e:?}")));
        let mut out: *mut CREDENTIALW = std::ptr::null_mut();
        let t = Instant::now();
        let r = unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut out) };
        report("read-after-delete", t, r.map(|_| "STILL PRESENT".into()).map_err(|e| format!("{e:?}")));
    }
}
