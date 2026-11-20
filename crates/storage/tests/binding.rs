//! H3b: the binding registry and its markers (task 185).

use std::path::{Path, PathBuf};

use wisp_storage::collection::CollectionId;
use wisp_storage::data_dir::DataDir;
use wisp_storage::vault::{LoadStatus, MemoryStore, VaultCtx, load_environment_with};
use wisp_storage::vault::{
    SecretsError, bind, bind_additional_checkout, bound_roots, rebind, share_with_folder,
};
use wisp_storage::vault_binding::tracked_marker_warning;

fn id() -> CollectionId {
    CollectionId::parse("abcdef0123456789").unwrap()
}

fn other_id() -> CollectionId {
    CollectionId::parse("0123456789abcdef").unwrap()
}

fn data() -> DataDir {
    DataDir::at(tempfile::tempdir().unwrap().keep())
}

fn carry_id(root: &Path, id: &CollectionId) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("collection.toml"),
        format!("format = 1\nid = \"{}\"\n", id.as_str()),
    )
    .unwrap();
}

fn root() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    carry_id(&p, &id());
    (d, p)
}

fn marker(root: &Path) -> PathBuf {
    root.join("environments/.wisp-binding")
}

fn marker_keys(root: &Path) -> Vec<String> {
    std::fs::read_to_string(marker(root))
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(k, _)| k.to_string()))
        .collect()
}

fn registry_id(d: &DataDir) -> String {
    let text = std::fs::read_to_string(d.root().join("vault-bindings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["registry_id"].as_str().unwrap().to_string()
}

#[test]
fn h3b_rebind_refused_while_other_root_live() {
    let (_a, a) = root();
    let (_b, b) = root();
    let (_c, c) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    share_with_folder(&d, &id(), &a, &b).unwrap();
    // One bound root is gone; the other is still live.
    drop(_b);
    assert!(!b.exists());
    let e = rebind(&d, &id(), &c).unwrap_err();
    match e {
        SecretsError::LooksLikeCopy { bound_root } => assert_eq!(bound_root, a),
        e => panic!("{e:?}"),
    }
    assert_eq!(bound_roots(&d, &id()).unwrap(), vec![a, b]);
}

#[test]
fn h3b_markers_share_no_id() {
    let (_a, a) = root();
    let (_b, b) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    share_with_folder(&d, &id(), &a, &b).unwrap();
    let (ka, kb) = (marker_keys(&a), marker_keys(&b));
    assert_eq!((ka.len(), kb.len()), (1, 1));
    assert_ne!(ka, kb);
    let reg = registry_id(&d);
    assert!(!ka.contains(&reg) && !kb.contains(&reg));
    // Both still pass the binding check.
    bind(&d, &id(), &a).unwrap();
    bind(&d, &id(), &b).unwrap();
}

#[test]
fn h3b_tracked_marker_warns() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    assert!(tracked_marker_warning(&a).is_none());
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&a)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    assert!(tracked_marker_warning(&a).is_none());
    git(&["add", "-f", "environments/.wisp-binding"]);
    assert_eq!(tracked_marker_warning(&a), Some(marker(&a)));
}

#[test]
fn h3b_oversized_marker_fails_closed() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let before = std::fs::read_to_string(marker(&a)).unwrap();
    // Over the byte cap.
    let big = format!("{before}{}", "x".repeat(40 * 1024));
    std::fs::write(marker(&a), &big).unwrap();
    assert!(bind(&d, &id(), &a).is_err());
    assert_eq!(std::fs::read_to_string(marker(&a)).unwrap(), big);
    // Under the byte cap, over the entry cap.
    let many: String = (0..100).map(|i| format!("k{i} n{i}\n")).collect();
    std::fs::write(marker(&a), format!("{before}{many}")).unwrap();
    assert!(bind(&d, &id(), &a).is_err());
    assert_eq!(
        std::fs::read_to_string(marker(&a)).unwrap(),
        format!("{before}{many}")
    );
}

#[test]
fn h3b_malformed_entries_dropped_on_write() {
    let (_a, a) = root();
    let (_b, b) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    std::fs::create_dir_all(b.join("environments")).unwrap();
    std::fs::write(
        marker(&b),
        "garbage\nbad!key nonce1\nforeign123 nonce456\n\n  \nonlykey \n",
    )
    .unwrap();
    share_with_folder(&d, &id(), &a, &b).unwrap();
    let text = std::fs::read_to_string(marker(&b)).unwrap();
    let keys = marker_keys(&b);
    assert_eq!(keys.len(), 2, "{text}");
    assert!(keys.contains(&"foreign123".to_string()));
    assert!(!text.contains("garbage") && !text.contains("bad!key"));
}

/// A path as `git worktree add` writes it into `.git` and `gitdir` files: the
/// plain `C:\x` disk form, never the `\\?\C:\x` verbatim form `canonicalize`
/// returns on Windows (that form starts with a double separator, which the
/// binding check refuses as a possible network or device path). Identity
/// elsewhere.
fn git_form(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(r) if !r.starts_with("UNC") => r.to_string(),
        _ => s,
    }
}

/// A linked worktree entry: `.git` file -> `<main>/.git/worktrees/<name>`.
fn make_worktree(main: &Path, wt: &Path, name: &str) -> PathBuf {
    let gd = main.join(".git/worktrees").join(name);
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        gd.join("gitdir"),
        format!("{}\n", git_form(&wt.join(".git"))),
    )
    .unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", git_form(&gd))).unwrap();
    gd
}

fn unverified(r: Result<(), SecretsError>) {
    assert!(
        matches!(r, Err(SecretsError::UnverifiedCheckout { .. })),
        "{r:?}"
    );
}

/// A worktree entry whose `.git` file is written `delay` after the entry's own
/// files, as when a folder is created at an old entry's path.
fn make_worktree_late(main: &Path, wt: &Path, name: &str, delay: std::time::Duration) -> PathBuf {
    let gd = main.join(".git/worktrees").join(name);
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        gd.join("gitdir"),
        format!("{}\n", git_form(&wt.join(".git"))),
    )
    .unwrap();
    std::thread::sleep(delay);
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", git_form(&gd))).unwrap();
    gd
}

/// Longer than the provenance window (10 s).
const PAST_WINDOW: std::time::Duration = std::time::Duration::from_secs(11);

#[test]
fn h3b_recreated_worktree_not_verified() {
    let (_a, a) = root();
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    // A fresh worktree verifies.
    let (_ok, ok) = root();
    make_worktree(&a, &ok, "fresh");
    bind_additional_checkout(&d, &id(), &ok).unwrap();
    // An entry whose folder was deleted by hand; a folder written long after
    // sits at that path now.
    let (_w, w) = root();
    make_worktree_late(&a, &w, "stale", PAST_WINDOW);
    unverified(bind_additional_checkout(&d, &id(), &w));
}

#[cfg(unix)]
#[test]
fn h3b_forged_mtime_worktree_not_verified() {
    let (_a, a) = root();
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let (_w, w) = root();
    let gd = make_worktree_late(&a, &w, "forged", PAST_WINDOW);
    // `touch -r`: the `.git` file's mtime is copied from the entry's file. Its
    // ctime cannot be.
    let mtime = std::fs::metadata(gd.join("gitdir"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(w.join(".git"))
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    unverified(bind_additional_checkout(&d, &id(), &w));
}

#[cfg(unix)]
#[test]
fn h3b_retargeted_component_refused() {
    let (_a, a) = root();
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    // `p/wt` is the new root; the entry names it through the link `l -> p`.
    let p = tempfile::tempdir().unwrap();
    let p_canon = std::fs::canonicalize(p.path()).unwrap();
    let wt = p_canon.join("wt");
    carry_id(&wt, &id());
    let l = tempfile::tempdir().unwrap();
    let link = std::fs::canonicalize(l.path()).unwrap().join("l");
    std::os::unix::fs::symlink(&p_canon, &link).unwrap();
    make_worktree(&a, &link.join("wt"), "w1");
    // `make_worktree` wrote the `.git` file through the link; the entry holds
    // the link form of the path.
    assert!(wt.join(".git").exists());
    unverified(bind_additional_checkout(&d, &id(), &wt));
}

#[test]
fn h3b_planted_common_dir_refused() {
    let (_o, outer) = root();
    // The outer collection (another id) is bound; the inner one sits inside it
    // and carries a planted `.git`.
    carry_id(&outer, &other_id());
    let inner = outer.join("sub");
    carry_id(&inner, &id());
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    let (_n, n) = root();
    make_worktree(&inner, &n, "w");
    let d = data();
    bind(&d, &id(), &inner).unwrap();
    // Control: with no collection root around it, the common dir is accepted.
    let (_n2, n2) = root();
    make_worktree(&inner, &n2, "w2");
    bind_additional_checkout(&d, &id(), &n2).unwrap();
    bind(&d, &other_id(), &outer).unwrap();
    unverified(bind_additional_checkout(&d, &id(), &n));
}

#[cfg(windows)]
fn verbatim_worktree(name: &str) -> (PathBuf, DataDir, PathBuf) {
    let (_a, a) = root();
    std::mem::forget(_a);
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let (_w, w) = root();
    std::mem::forget(_w);
    let gd = a.join(".git").join("worktrees").join(name);
    // `root()` is canonical, so `gd` is already the verbatim form on Windows.
    let verbatim = gd.clone();
    let _ = std::fs::create_dir_all(&verbatim);
    let _ = std::fs::write(verbatim.join("commondir"), "../..\n");
    let _ = std::fs::write(
        verbatim.join("gitdir"),
        format!("{}\n", git_form(&w.join(".git"))),
    );
    std::fs::write(w.join(".git"), format!("gitdir: {}\n", git_form(&gd))).unwrap();
    (a, d, w)
}

#[cfg(windows)]
#[test]
fn h3b_win_com1_refused() {
    let (_a, d, w) = verbatim_worktree("COM1");
    unverified(bind_additional_checkout(&d, &id(), &w));
}

#[cfg(windows)]
#[test]
fn h3b_win_trailing_dot_refused() {
    let (_a, d, w) = verbatim_worktree("w1.");
    unverified(bind_additional_checkout(&d, &id(), &w));
}

#[test]
fn h3b_legacy_row_rekeyed_on_check() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let reg = registry_id(&d);
    // Rewrite the row and marker as a pre-0.5.0 binding left them: no `key`,
    // the marker entry under the registry id.
    let rp = d.root().join("vault-bindings.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rp).unwrap()).unwrap();
    let row = &mut v["bindings"][id().as_str()][0];
    let key = row["key"].as_str().unwrap().to_string();
    row.as_object_mut().unwrap().remove("key");
    std::fs::write(&rp, serde_json::to_string(&v).unwrap()).unwrap();
    let text = std::fs::read_to_string(marker(&a)).unwrap();
    std::fs::write(marker(&a), text.replace(&key, &reg)).unwrap();
    assert_eq!(marker_keys(&a), vec![reg.clone()]);
    // A load runs the binding check, which passes and re-keys.
    std::fs::create_dir_all(a.join("environments")).unwrap();
    std::fs::write(
        a.join("environments/dev.toml"),
        "secrets = \"keychain\"\nsecret_names = [\"token\"]\n",
    )
    .unwrap();
    let ctx = VaultCtx::new(d.clone());
    let l = load_environment_with(&a, "dev", Some(&id()), &MemoryStore::new(), &ctx).unwrap();
    assert!(
        matches!(l.status, LoadStatus::Store { .. }),
        "{:?}",
        l.status
    );
    let keys = marker_keys(&a);
    assert_eq!(keys.len(), 1);
    assert!(!keys.contains(&reg));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rp).unwrap()).unwrap();
    assert_eq!(
        v["bindings"][id().as_str()][0]["key"].as_str(),
        Some(keys[0].as_str())
    );
    // And it still passes.
    let l = load_environment_with(&a, "dev", Some(&id()), &MemoryStore::new(), &ctx).unwrap();
    assert!(matches!(l.status, LoadStatus::Store { .. }));
}

#[test]
fn h3b_full_marker_refuses_write() {
    let (_a, a) = root();
    let (_b, b) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    // 64 other registries' entries fill `b`'s marker.
    std::fs::create_dir_all(b.join("environments")).unwrap();
    let full: String = (0..64).map(|i| format!("foreign{i} nonce{i}\n")).collect();
    std::fs::write(marker(&b), &full).unwrap();
    assert!(share_with_folder(&d, &id(), &a, &b).is_err());
    // Nothing was evicted and nothing bound.
    assert_eq!(std::fs::read_to_string(marker(&b)).unwrap(), full);
    assert_eq!(bound_roots(&d, &id()).unwrap(), vec![a]);
}

#[test]
fn h3b_root_list_capped() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let mut keep = Vec::new();
    for _ in 1..64 {
        let (t, r) = root();
        share_with_folder(&d, &id(), &a, &r).unwrap();
        keep.push(t);
    }
    assert_eq!(bound_roots(&d, &id()).unwrap().len(), 64);
    let (_x, x) = root();
    let e = share_with_folder(&d, &id(), &a, &x).unwrap_err();
    assert!(matches!(e, SecretsError::Storage(_)), "{e:?}");
    assert_eq!(bound_roots(&d, &id()).unwrap().len(), 64);
}

#[test]
fn h3b_dead_row_keys_dropped_for_same_root() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    for i in 0..70u32 {
        let nid = CollectionId::parse(&format!("{:016x}", 0x1000_0000_0000_0000u64 + u64::from(i)))
            .unwrap();
        carry_id(&a, &nid);
        bind(&d, &nid, &a).unwrap();
    }
    assert_eq!(marker_keys(&a).len(), 1);
}
