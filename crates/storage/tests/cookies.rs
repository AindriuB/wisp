use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wisp_storage::StorageError;
use wisp_storage::collection::CollectionId;
use wisp_storage::cookies::{CookieFile, MAX_COOKIES, StoredCookie};
use wisp_storage::data_dir::DataDir;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_000_000)
}

fn cookie(name: &str, expires_in: i64) -> StoredCookie {
    let secs = 1_000_000 + expires_in;
    StoredCookie {
        name: name.into(),
        value: "v".into(),
        domain: "example.com".into(),
        host_only: true,
        path: "/".into(),
        secure: false,
        http_only: true,
        expires: UNIX_EPOCH + Duration::from_secs(secs as u64),
    }
}

fn setup() -> (tempfile::TempDir, DataDir, CookieFile) {
    let t = tempfile::tempdir().unwrap();
    let d = DataDir::at(t.path());
    let id = CollectionId::parse("abcdef0123456789").unwrap();
    let f = CookieFile::new(&d, &id);
    (t, d, f)
}

fn path(t: &tempfile::TempDir) -> std::path::PathBuf {
    t.path().join("cookies").join("abcdef0123456789.json")
}

#[test]
fn off_by_default_and_toggle() {
    let (t, _d, f) = setup();
    assert!(!f.is_enabled());
    f.disable().unwrap();
    f.enable().unwrap();
    assert!(f.is_enabled());
    assert!(path(&t).exists());
    assert!(f.load(now()).unwrap().is_empty());
    f.disable().unwrap();
    assert!(!f.is_enabled());
}

#[test]
fn round_trip() {
    let (_t, _d, f) = setup();
    f.enable().unwrap();
    let c = vec![cookie("a", 100)];
    f.save(&c, now()).unwrap();
    assert_eq!(f.load(now()).unwrap(), c);
}

#[test]
fn save_while_disabled_creates_nothing() {
    let (t, _d, f) = setup();
    f.save(&[cookie("a", 100)], now()).unwrap();
    assert!(!path(&t).exists());
    assert!(!t.path().join("cookies").exists());
}

#[test]
fn expired_dropped_on_save_and_load() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    f.save(
        &[cookie("old", 0), cookie("gone", -5), cookie("ok", 10)],
        now(),
    )
    .unwrap();
    let raw = std::fs::read_to_string(path(&t)).unwrap();
    assert!(!raw.contains("old") && !raw.contains("gone"));
    let later = now() + Duration::from_secs(20);
    assert!(f.load(later).unwrap().is_empty());
    assert_eq!(f.load(now()).unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn modes_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (t, _d, f) = setup();
    f.enable().unwrap();
    f.save(&[cookie("a", 10)], now()).unwrap();
    let m = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(m(&path(&t)), 0o600);
    assert_eq!(m(&t.path().join("cookies")), 0o700);
}

#[test]
fn oversize_file_refused() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let big = vec![b' '; 4 * 1024 * 1024 + 1];
    std::fs::write(path(&t), &big).unwrap();
    assert!(matches!(f.load(now()), Err(StorageError::Invalid { .. })));
    assert_eq!(std::fs::read(path(&t)).unwrap().len(), big.len());
}

#[test]
fn caps_count_to_3000() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let entries: Vec<String> = (0..MAX_COOKIES + 50)
        .map(|i| {
            format!(
                r#"{{"name":"n{i}","value":"v","domain":"a.com","host_only":true,"path":"/","secure":false,"http_only":false,"expires":2000000}}"#
            )
        })
        .collect();
    std::fs::write(
        path(&t),
        format!(r#"{{"version":1,"cookies":[{}]}}"#, entries.join(",")),
    )
    .unwrap();
    assert_eq!(f.load(now()).unwrap().len(), MAX_COOKIES);
}

#[test]
fn skips_oversize_pair_and_empty_domain() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let e = |n: &str, v: &str, d: &str| {
        format!(
            r#"{{"name":"{n}","value":"{v}","domain":"{d}","host_only":true,"path":"/","secure":false,"http_only":false,"expires":2000000}}"#
        )
    };
    let big = "x".repeat(4_100);
    let ok = "y".repeat(4_100);
    let body = [
        e("big", &big, "a.com"),
        e("nodom", "v", ""),
        e("ok", &ok[..4_000], "a.com"),
    ]
    .join(",");
    std::fs::write(path(&t), format!(r#"{{"version":1,"cookies":[{body}]}}"#)).unwrap();
    let got = f.load(now()).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "ok");
}

#[test]
fn corrupt_and_wrong_version_error_and_untouched() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    for body in ["not json", r#"{"version":2,"cookies":[]}"#] {
        std::fs::write(path(&t), body).unwrap();
        assert!(f.load(now()).is_err());
        assert_eq!(std::fs::read_to_string(path(&t)).unwrap(), body);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_file_refused() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let target = t.path().join("elsewhere.json");
    std::fs::write(&target, r#"{"version":1,"cookies":[]}"#).unwrap();
    std::fs::remove_file(path(&t)).unwrap();
    std::os::unix::fs::symlink(&target, path(&t)).unwrap();
    assert!(f.load(now()).is_err());
    assert!(f.save(&[cookie("a", 10)], now()).is_err());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        r#"{"version":1,"cookies":[]}"#
    );
}

#[test]
fn debug_redacts_stored_cookie() {
    let mut c = cookie("sid", 10);
    c.value = "s3cret-value".into();
    for s in [format!("{c:?}"), format!("{c:#?}")] {
        assert!(!s.contains("s3cret-value"));
        assert!(s.contains("<redacted>"));
    }
}

fn entry_json(n: &str, v: &str, expires: u64) -> String {
    format!(
        r#"{{"name":"{n}","value":"{v}","domain":"a.com","host_only":true,"path":"/","secure":false,"http_only":false,"expires":{expires}}}"#
    )
}

#[test]
fn huge_expires_is_skipped_not_a_panic() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let body = [
        entry_json("max", "v", u64::MAX),
        entry_json("big", "v", 10_000_000_000_000),
        entry_json("ok", "v", 2_000_000),
    ]
    .join(",");
    std::fs::write(path(&t), format!(r#"{{"version":1,"cookies":[{body}]}}"#)).unwrap();
    let got = f.load(now()).unwrap();
    assert!(got.iter().any(|c| c.name == "ok"));
    assert!(got.iter().all(|c| c.name != "max"));
}

#[test]
fn name_value_boundary_is_4096() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let body = [
        entry_json("n", &"x".repeat(4_095), 2_000_000),
        entry_json("m", &"x".repeat(4_096), 2_000_001),
    ]
    .join(",");
    std::fs::write(path(&t), format!(r#"{{"version":1,"cookies":[{body}]}}"#)).unwrap();
    let got = f.load(now()).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "n");
}

#[test]
fn save_skips_oversize_domain_and_path() {
    let (_t, _d, f) = setup();
    f.enable().unwrap();
    let mut long_domain = cookie("d", 10);
    long_domain.domain = "a".repeat(254);
    let mut long_path = cookie("p", 10);
    long_path.path = "/".repeat(1_025);
    let mut edge = cookie("e", 10);
    edge.domain = "a".repeat(253);
    edge.path = "/".repeat(1_024);
    f.save(&[long_domain, long_path, edge], now()).unwrap();
    let got = f.load(now()).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "e");
}

#[test]
fn save_trims_to_4_mib_keeping_latest_expiring() {
    let (t, _d, f) = setup();
    f.enable().unwrap();
    let cookies: Vec<StoredCookie> = (0..1_200)
        .map(|i| {
            let mut c = cookie(&format!("c{i}"), 100 + i);
            c.value = "v".repeat(4_000);
            c
        })
        .collect();
    f.save(&cookies, now()).unwrap();
    assert!(std::fs::metadata(path(&t)).unwrap().len() <= 4 * 1024 * 1024);
    let got = f.load(now()).unwrap();
    assert!(got.len() < 1_200 && !got.is_empty());
    assert!(got.iter().any(|c| c.name == "c1199"));
    assert!(got.iter().all(|c| c.name != "c0"));
    let min_kept = got.iter().map(|c| c.expires).min().unwrap();
    let dropped_max = cookies
        .iter()
        .filter(|c| !got.iter().any(|g| g.name == c.name))
        .map(|c| c.expires)
        .max()
        .unwrap();
    assert!(dropped_max < min_kept);
}
