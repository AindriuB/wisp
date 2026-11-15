use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use wisp_storage::validate::{Report, Severity, validate};

fn w(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

const GOOD: &str = "name = \"ok\"\nmethod = \"GET\"\nurl = \"http://x\"\n";

fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(r, "good.toml", GOOD);
    w(r, "broken/folder.toml", "vars = [[[\n");
    w(r, "broken/inner.toml", GOOD);
    w(
        r,
        "badreq.toml",
        "name = \"x\"\nmethod = \"NOPE\"\nurl = \"u\"\n",
    );
    w(
        r,
        "asserts.toml",
        &format!(
            "{GOOD}\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = \"200\"\n\n\
             [[assert]]\nkind = \"future\"\nop = \"teleports\"\n\n\
             [[assert]]\nkind = \"header\"\nname = \"x\"\nop = \"equals\"\nvalue = \"{{{{tok}}}}\"\n"
        ),
    );
    w(r, "environments/prod.toml", "secrets = \"keychain\"\n");
    w(r, "environments/dev.toml", "[vars]\n");
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        w(outside.path(), "x.toml", GOOD);
        let p = outside.path().to_path_buf();
        std::mem::forget(outside);
        std::os::unix::fs::symlink(p, r.join("escape")).unwrap();
    }
    d
}

fn has(rep: &Report, path: &str, sev: Severity) -> bool {
    rep.findings
        .iter()
        .any(|f| f.path == Path::new(path) && f.severity == sev)
}

#[test]
fn one_call_reports_every_problem() {
    let d = fixture();
    let rep = validate(d.path());
    assert!(!rep.truncated);
    assert!(has(&rep, "broken/folder.toml", Severity::Error), "{rep:?}");
    assert!(has(&rep, "badreq.toml", Severity::Error));
    let a: Vec<_> = rep
        .findings
        .iter()
        .filter(|f| f.path == Path::new("asserts.toml"))
        .map(|f| f.severity)
        .collect();
    // malformed, unknown kind, `{{`
    assert_eq!(
        a,
        vec![Severity::Error, Severity::Warning, Severity::Warning]
    );
    assert!(has(&rep, "environments/prod.toml", Severity::Warning));
    #[cfg(unix)]
    assert!(has(&rep, "escape", Severity::Error));
    assert!(!has(&rep, "good.toml", Severity::Error));
}

#[test]
fn malformed_assertion_is_error_and_many_is_warning() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(
        r,
        "m.toml",
        &format!("{GOOD}\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = \"200\"\n"),
    );
    let many = "[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 200\n\n".repeat(51);
    w(r, "many.toml", &format!("{GOOD}\n{many}"));
    let rep = validate(r);
    assert!(has(&rep, "m.toml", Severity::Error));
    assert!(has(&rep, "many.toml", Severity::Warning));
}

#[test]
fn unsupported_format_and_unreadable_root() {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 9\n");
    assert!(has(&validate(d.path()), "collection.toml", Severity::Error));
    let rep = validate(&d.path().join("missing"));
    assert_eq!(rep.findings.len(), 1);
    assert_eq!(rep.findings[0].severity, Severity::Error);
}

fn snapshot(root: &Path, out: &mut BTreeMap<PathBuf, (SystemTime, u64)>) {
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        let m = fs::symlink_metadata(&p).unwrap();
        out.insert(p.clone(), (m.modified().unwrap(), m.len()));
        if m.is_dir() {
            snapshot(&p, out);
        }
    }
}

fn set_readonly(root: &Path, ro: bool) {
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        let m = fs::symlink_metadata(&p).unwrap();
        if m.file_type().is_symlink() {
            continue;
        }
        if m.is_dir() && !ro {
            set_perm(&p, false);
        }
        if m.is_dir() {
            set_readonly(&p, ro);
        }
        if m.is_dir() && ro {
            set_perm(&p, true);
        } else if m.is_file() {
            set_perm(&p, ro);
        }
    }
}

fn set_perm(p: &Path, ro: bool) {
    let mut perm = fs::metadata(p).unwrap().permissions();
    perm.set_readonly(ro);
    fs::set_permissions(p, perm).unwrap();
}

#[test]
fn validate_changes_nothing() {
    let d = fixture();
    set_readonly(d.path(), true);
    let mut before = BTreeMap::new();
    snapshot(d.path(), &mut before);
    let _ = validate(d.path());
    let mut after = BTreeMap::new();
    snapshot(d.path(), &mut after);
    set_readonly(d.path(), false);
    assert_eq!(before, after);
}

#[test]
fn findings_never_carry_values() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(r, "environments/e.toml", "[vars]\nplain = \"PLAINMARK\"\n");
    w(
        r,
        "environments/e.secrets.toml",
        "[vars]\nsec = \"SECRETMARK\"\n",
    );
    w(
        r,
        "bad.toml",
        "name = \"b\"\nmethod = \"NOPE\"\nurl = \"u\"\n[headers]\nAuthorization = \"Bearer AUTHMARK\"\n[vars]\nv = \"PLAINMARK\"\n",
    );
    w(
        r,
        "worse.toml",
        "name = \"b\"\nmethod = \"GET\"\nurl = \"u\"\nheaders = [{ name = \"Authorization\", value = \"AUTHMARK\" }, 5]\nvars = [{ name = \"v\", value = \"PLAINMARK\" }, 5]\n",
    );
    let rep = validate(r);
    assert!(!rep.findings.is_empty());
    let dbg = format!("{rep:?}");
    for m in ["SECRETMARK", "PLAINMARK", "AUTHMARK"] {
        assert!(!dbg.contains(m), "{m} leaked: {dbg}");
    }
}

fn one(rep: &Report, path: &str) -> Vec<String> {
    rep.findings
        .iter()
        .filter(|f| f.path == Path::new(path))
        .map(|f| f.message.clone())
        .collect()
}

#[test]
fn secrets_file_error_is_filed_under_the_secrets_file() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(r, "environments/dev.toml", "[vars]\na = \"1\"\n");
    w(r, "environments/dev.secrets.toml", "\n\n[[[\n");
    let rep = validate(r);
    assert!(
        has(&rep, "environments/dev.secrets.toml", Severity::Error),
        "{rep:?}"
    );
    assert!(one(&rep, "environments/dev.toml").is_empty(), "{rep:?}");
}

#[test]
fn sidecar_body_error_is_filed_under_the_sidecar() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(
        r,
        "a.toml",
        &format!("{GOOD}\n[body]\ntype = \"text\"\nfile = \"missing.txt\"\n"),
    );
    let rep = validate(r);
    assert!(has(&rep, "missing.txt", Severity::Error), "{rep:?}");
    assert!(one(&rep, "a.toml").is_empty(), "{rep:?}");
}

#[test]
fn unknown_secret_store_kind_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    w(r, "collection.toml", "format = 1\n");
    w(r, "environments/typo.toml", "secrets = \"keychian\"\n");
    let rep = validate(r);
    let f = rep
        .findings
        .iter()
        .find(|f| f.path == Path::new("environments/typo.toml"))
        .expect("finding");
    assert_eq!(f.severity, Severity::Error, "{rep:?}");
    assert_eq!(f.key.as_deref(), Some("secrets"));
    assert!(!f.message.contains("keychian"));
}

/// Run `validate` on a thread so a read that blocks forever fails the test instead of hanging it.
fn validate_in_time(root: &Path) -> Report {
    let (tx, rx) = std::sync::mpsc::channel();
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(validate(&root));
    });
    rx.recv_timeout(std::time::Duration::from_secs(20))
        .expect("validate did not return")
}

fn collection_root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    w(d.path(), "collection.toml", "format = 1\n");
    d
}

#[test]
fn oversize_sidecar_is_not_read() {
    let d = collection_root();
    let r = d.path();
    w(
        r,
        "big.toml",
        &format!("{GOOD}\n[body]\ntype = \"text\"\nfile = \"big.body.txt\"\n"),
    );
    // Sparse: large by length, no disk cost.
    let f = fs::File::create(r.join("big.body.txt")).unwrap();
    f.set_len(8 * 1024 * 1024 * 1024).unwrap();
    let rep = validate_in_time(r);
    assert!(has(&rep, "big.toml", Severity::Warning), "{rep:?}");
    assert!(!has(&rep, "big.toml", Severity::Error));
}

#[test]
fn oversize_request_file_is_not_read() {
    let d = collection_root();
    let f = fs::File::create(d.path().join("huge.toml")).unwrap();
    f.set_len(8 * 1024 * 1024 * 1024).unwrap();
    let rep = validate_in_time(d.path());
    assert!(has(&rep, "huge.toml", Severity::Warning), "{rep:?}");
}

#[cfg(unix)]
fn mkfifo(p: &Path) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(p)
            .status()
            .unwrap()
            .success()
    );
}

#[cfg(unix)]
#[test]
fn fifo_request_and_fifo_sidecar_do_not_block() {
    let d = collection_root();
    let r = d.path();
    mkfifo(&r.join("pipe.toml"));
    w(
        r,
        "side.toml",
        &format!("{GOOD}\n[body]\ntype = \"text\"\nfile = \"side.body.txt\"\n"),
    );
    mkfifo(&r.join("side.body.txt"));
    let rep = validate_in_time(r);
    assert!(has(&rep, "pipe.toml", Severity::Warning), "{rep:?}");
    assert!(has(&rep, "side.toml", Severity::Warning), "{rep:?}");
}

#[cfg(unix)]
#[test]
fn ancestor_symlink_loop_is_skipped() {
    let d = collection_root();
    let r = d.path();
    w(r, "a/ok.toml", GOOD);
    std::os::unix::fs::symlink(r, r.join("a/loop")).unwrap();
    let rep = validate_in_time(r);
    assert!(!rep.truncated);
    assert!(rep.findings.is_empty(), "{rep:?}");
}

#[cfg(unix)]
#[test]
fn dangling_link_is_a_finding() {
    let d = collection_root();
    let r = d.path();
    std::os::unix::fs::symlink(r.join("nowhere"), r.join("dangling")).unwrap();
    let rep = validate(r);
    assert!(has(&rep, "dangling", Severity::Error), "{rep:?}");
}

#[cfg(unix)]
#[test]
fn folder_cap_truncates_like_open_collection() {
    let d = collection_root();
    let r = d.path();
    // Two links per level into the next: 2^15 paths to one leaf, far over the folder cap.
    for i in 0..15 {
        fs::create_dir_all(r.join(format!("l{i}"))).unwrap();
        for n in ["a", "b"] {
            std::os::unix::fs::symlink(format!("../l{}", i + 1), r.join(format!("l{i}/{n}")))
                .unwrap();
        }
    }
    fs::create_dir_all(r.join("l15")).unwrap();
    let rep = validate_in_time(r);
    assert!(rep.truncated, "{rep:?}");
    let cut: Vec<_> = rep
        .findings
        .iter()
        .filter(|f| f.message.contains("folders"))
        .collect();
    assert_eq!(cut.len(), 1, "{rep:?}");
    // Same cap as the scan: its error names the same number.
    let cap = "10000";
    assert!(cut[0].message.contains(cap));
    let err = wisp_storage::collection::open_collection(r).unwrap_err();
    assert!(err.to_string().contains(cap));
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_a_finding() {
    use std::os::unix::fs::PermissionsExt;
    let d = collection_root();
    let r = d.path();
    w(r, "locked/x.toml", GOOD);
    let locked = r.join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        // Running as root: the mode does not stop the read.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let rep = validate(r);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(has(&rep, "locked", Severity::Error), "{rep:?}");
}
